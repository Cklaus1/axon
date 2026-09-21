//! R21 §3.1 — the `.axjob` job manifest: parse + validate. Pure (no I/O).
//!
//! Hand-rolled parser for exactly the flat `.axjob` schema (top-level keys +
//! `[grant]` + `[grant.budget]`), per the spec's "no new heavy deps" rule. A
//! malformed manifest fails closed → `Verdict::Malformed` (exit 2). The program
//! path is resolved relative to the manifest dir but NOT stat'd here (existence
//! is an I/O check the supervisor/runtime performs in a later slice).

use crate::grant::{Budget, ExecPolicy, Grant, Label};
use crate::verdict::Verdict;
use std::path::{Path, PathBuf};

/// A parsed, validated job manifest (R21 §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobManifest {
    pub program: PathBuf,
    pub intent: String,
    pub seed: u64,
    pub grant: Grant,
    /// JOB POLICY: does this job require a valid approval token to run?
    ///
    /// Deliberately separate from whether a token is PRESENT, which is runtime
    /// EVIDENCE. Neither is inferred from the other (triage OSK-P4-H8):
    ///
    ///   not required + no token            → run
    ///   required     + no/invalid token    → refuse (exit 8)
    ///   required     + valid token         → run
    ///
    /// Absent ⇒ `false`, so existing manifests keep working — the same
    /// "easy by default, explicit lockdown when needed" posture as `profile`.
    pub require_approval: bool,
}

/// Convenience for a `Malformed` verdict carrying a reason.
fn bad(reason: impl Into<String>) -> Verdict {
    Verdict::Malformed {
        reason: reason.into(),
    }
}

/// Parse + validate `.axjob` source. `base_dir` is the directory the manifest
/// lives in; the `program` field is resolved relative to it (pure path join).
pub fn parse(src: &str, base_dir: &Path) -> Result<JobManifest, Verdict> {
    // ── tokenize into (section, key, raw_value) lines ───────────────────────
    let mut section = String::new();
    let mut program: Option<String> = None;
    let mut intent: Option<String> = None;
    let mut seed: Option<u64> = None;
    let mut fs_read: Option<Vec<String>> = None;
    let mut fs_write: Option<Vec<String>> = None;
    let mut net: Option<Vec<String>> = None;
    let mut profile: Option<crate::profile::Profile> = None;
    let mut require_approval: Option<bool> = None;
    let mut reproducible_override: Option<bool> = None;
    let mut exec: Option<ExecPolicy> = None;
    let mut max_label: Option<Label> = None;
    let mut calls: Option<i64> = None;
    let mut tokens: Option<i64> = None;
    let mut cost_micro: Option<i64> = None;

    for (lineno, raw) in src.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(sec) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = sec.trim().to_string();
            continue;
        }
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| bad(format!("line {}: expected `key = value`", lineno + 1)))?;
        let (key, val) = (key.trim(), val.trim());
        let where_ = || format!("line {} (`{key}`)", lineno + 1);

        match (section.as_str(), key) {
            ("", "program") => program = Some(parse_str(val).ok_or_else(|| bad(where_()))?),
            ("", "intent") => intent = Some(parse_str(val).ok_or_else(|| bad(where_()))?),
            ("", "seed") => {
                seed = Some(
                    parse_int(val)
                        .ok_or_else(|| bad(where_()))?
                        .try_into()
                        .map_err(|_| {
                            bad(format!("{}: seed must be a non-negative u64", where_()))
                        })?,
                )
            }
            // Accepted at top level or under [grant]: operators reasonably
            // reach for either, and refusing one of them would be a papercut
            // whose only function is to be surprising.
            // An explicit `reproducible` overrides the profile's default.
            // Needed so `to_axjob` can ROUND-TRIP the bit: the manifest stores
            // `grant.reproducible`, not a profile name, and inferring
            // `profile = "hermetic"` from a bool would conflate a posture with
            // the one profile that happens to imply it. Archiving dropped this
            // entirely, so a replayed hermetic job ran under `developer` and
            // inherited the ambient environment — the replay of the one profile
            // whose entire purpose is reproducibility was not reproducible.
            ("", "reproducible") | ("grant", "reproducible") => {
                reproducible_override = Some(match val.trim() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(bad(format!(
                            "{}: reproducible must be true or false, got `{other}`",
                            where_()
                        )))
                    }
                });
            }
            ("", "require_approval") | ("grant", "require_approval") => {
                require_approval = Some(match val.trim() {
                    "true" => true,
                    "false" => false,
                    other => {
                        return Err(bad(format!(
                            "{}: require_approval must be true or false, got `{other}`",
                            where_()
                        )))
                    }
                })
            }
            ("grant", "fs_read") => fs_read = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            ("grant", "fs_write") => fs_write = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            ("grant", "net") => net = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            // The capability POSTURE. Absent = developer (easy by default);
            // a misspelling is refused rather than falling back to it.
            ("", "profile") | ("grant", "profile") => {
                profile = Some(
                    crate::profile::Profile::parse(val.trim().trim_matches('"'))
                        .map_err(|e| bad(format!("{}: {e}", where_())))?,
                )
            }
            ("grant", "exec") => {
                let s = parse_str(val).ok_or_else(|| bad(where_()))?;
                exec = Some(ExecPolicy::parse(&s).ok_or_else(|| {
                    bad(format!("{}: exec must be \"none\" or \"any\"", where_()))
                })?)
            }
            ("grant", "max_label") => {
                let s = parse_str(val).ok_or_else(|| bad(where_()))?;
                max_label = Some(Label::parse(&s).ok_or_else(|| {
                    bad(format!(
                        "{}: max_label must be public|internal|secret",
                        where_()
                    ))
                })?)
            }
            ("grant.budget", "calls") => calls = Some(parse_int(val).ok_or_else(|| bad(where_()))?),
            ("grant.budget", "tokens") => {
                tokens = Some(parse_int(val).ok_or_else(|| bad(where_()))?)
            }
            ("grant.budget", "cost_micro") => {
                cost_micro = Some(parse_int(val).ok_or_else(|| bad(where_()))?)
            }
            (sec, k) => {
                return Err(bad(format!(
                    "{}: unknown key `{k}` in section `[{sec}]`",
                    where_()
                )))
            }
        }
    }

    // ── required fields ─────────────────────────────────────────────────────
    let program = program.ok_or_else(|| bad("missing `program`"))?;
    if !program.ends_with(".ax") {
        return Err(bad("`program` must be a .ax file"));
    }
    let intent = intent.unwrap_or_default();
    let seed = seed.unwrap_or(42);
    // A named profile and an explicit `reproducible = false` can contradict
    // each other, and the contradiction was resolved SILENTLY in favour of the
    // weaker one: `profile = "hermetic"` with `reproducible = false` parsed,
    // explained and ran as non-reproducible, so a job labelled with the one
    // profile whose entire purpose is reproducibility was not reproducible and
    // nothing said so.
    //
    // Only the WEAKENING direction is refused. `reproducible = true` under a
    // non-reproducible profile is a strengthening, and coherent with
    // `Grant::intersect`, which ORs the bit precisely so that either side may
    // demand reproducibility.
    //
    // This cannot break the archive round-trip: `to_axjob` writes
    // `grant.reproducible` and no `profile` line, so the two never co-occur in
    // a generated manifest.
    if let (Some(pf), Some(false)) = (profile, reproducible_override) {
        if pf.is_reproducible() {
            return Err(bad(format!(
                "profile `{}` is reproducible, but `reproducible = false` was \
                 also given — these contradict. Remove one: drop the \
                 `reproducible` line to keep the profile's guarantee, or name a \
                 profile that does not promise it.",
                pf.name()
            )));
        }
    }
    let profile = profile.unwrap_or_default();
    let exec = exec.unwrap_or_else(|| profile.default_exec());
    let max_label = max_label.ok_or_else(|| bad("missing `grant.max_label`"))?;
    // An OMITTED dimension takes the profile's default; an explicitly written
    // one is honoured as-is, including an explicitly empty `[]`.
    //
    // This is where "easy by default" lives. It used to live in the runtime,
    // where an empty list was read as unrestricted — so a manifest that said
    // nothing about the network got unrestricted network, and a manifest that
    // said `net = []` got the same thing. Those are opposite intentions and
    // they produced identical behaviour.
    //
    // Now the default is MATERIALISED: omit `net` under the developer profile
    // and the grant records `["*"]`, which is what `to_axjob` will write back
    // and what the runtime will enforce. Say `net = []` and you get no network.
    let fs_read = validate_prefixes(
        fs_read.unwrap_or_else(|| profile.default_fs_read()),
        "fs_read",
    )?;
    let fs_write = validate_prefixes(
        fs_write.unwrap_or_else(|| profile.default_fs_write()),
        "fs_write",
    )?;
    let net = net.unwrap_or_else(|| profile.default_net());

    let budget = Budget {
        calls: nonneg(calls.unwrap_or(0), "budget.calls")?,
        tokens: nonneg(tokens.unwrap_or(0), "budget.tokens")?,
        cost_micro: nonneg(cost_micro.unwrap_or(0), "budget.cost_micro")?,
    };

    Ok(JobManifest {
        // Absent ⇒ not required. An approval policy nobody stated is not an
        // approval policy, and defaulting it ON would break every existing job.
        require_approval: require_approval.unwrap_or(false),
        program: base_dir.join(program),
        intent,
        seed,
        grant: Grant {
            reproducible: reproducible_override.unwrap_or_else(|| profile.is_reproducible()),
            fs_read,
            fs_write,
            net,
            exec,
            max_label,
            budget,
        },
    })
}

/// Serialize a manifest back to `.axjob`, with `program` as its (already
/// resolved, absolute) path — so a saved copy re-parses to the SAME program
/// regardless of the directory it is later read from (deterministic replay).
/// Lossy on `intent` only (quotes → apostrophes); intent is not hashed.
pub fn to_axjob(m: &JobManifest) -> String {
    // EXHAUSTIVE: a new JobManifest field is a COMPILE ERROR here until it is
    // archived or explicitly bound with a reason. `..` must never be added.
    //
    // This dropped `require_approval` and `reproducible`, and the archived file
    // is what `axon-os replay` reloads. So a replayed job never re-checked
    // sign-off, and a hermetic job replayed as `developer`: REPRODUCED, the
    // replay resolved an operator-ambient module through AXON_PATH that the
    // recorded hermetic run could not see, and diverged. That divergence was
    // luck — for a job whose output does not depend on the injected module,
    // the replay would have passed while running unsealed.
    let JobManifest {
        program,
        intent,
        seed,
        grant,
        require_approval,
    } = m;
    let g = grant;
    let list = |xs: &[String]| {
        xs.iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "program = \"{}\"\nintent = \"{}\"\nseed = {}\nrequire_approval = {}\n\
         [grant]\nreproducible = {}\nfs_read = [{}]\nfs_write = [{}]\nnet = [{}]\n\
         exec = \"{}\"\nmax_label = \"{}\"\n[grant.budget]\ncalls = {}\ntokens = {}\n\
         cost_micro = {}\n",
        program.display(),
        intent.replace('"', "'"),
        seed,
        require_approval,
        g.reproducible,
        list(&g.fs_read),
        list(&g.fs_write),
        list(&g.net),
        g.exec.as_str(),
        g.max_label.as_str(),
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
    )
}

// ── small pure helpers ──────────────────────────────────────────────────────

fn strip_comment(line: &str) -> &str {
    // A `#` outside a quoted string starts a comment. The schema has no `#`
    // inside string values in practice; honor quotes defensively.
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_str(val: &str) -> Option<String> {
    let v = val.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

fn parse_int(val: &str) -> Option<i64> {
    val.trim().parse::<i64>().ok()
}

fn parse_arr(val: &str) -> Option<Vec<String>> {
    let v = val.trim();
    let inner = v.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for elem in inner.split(',') {
        out.push(parse_str(elem.trim())?);
    }
    Some(out)
}

/// Reject path-traversal prefixes (`..` component) — fail closed (mirrors E1001).
fn validate_prefixes(prefixes: Vec<String>, axis: &str) -> Result<Vec<String>, Verdict> {
    for p in &prefixes {
        if p.is_empty() {
            return Err(bad(format!("{axis}: empty path prefix")));
        }
        if p.split(['/', '\\']).any(|c| c == "..") {
            return Err(bad(format!(
                "{axis}: path `{p}` contains a `..` component (traversal denied)"
            )));
        }
    }
    Ok(prefixes)
}

fn nonneg(n: i64, what: &str) -> Result<i64, Verdict> {
    if n < 0 {
        Err(bad(format!("{what} must be \u{2265} 0")))
    } else {
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
program = "summarize.ax"
intent  = "Summarize ./data/report.txt into ./out/; no network."
seed    = 42
[grant]
fs_read   = ["./data/"]
fs_write  = ["./out/"]
net       = []
exec      = "none"
max_label = "internal"
[grant.budget]
calls       = 100
tokens      = 50000
cost_micro  = 1000000
"#;

    fn p(src: &str) -> Result<JobManifest, Verdict> {
        parse(src, Path::new("/jobs"))
    }

    #[test]
    fn parses_the_valid_example() {
        let m = p(VALID).expect("valid manifest parses");
        assert_eq!(m.program, Path::new("/jobs/summarize.ax"));
        assert_eq!(m.seed, 42);
        assert_eq!(m.grant.fs_read, vec!["./data/".to_string()]);
        assert_eq!(m.grant.fs_write, vec!["./out/".to_string()]);
        assert!(m.grant.net.is_empty());
        assert_eq!(m.grant.exec, ExecPolicy::None);
        assert_eq!(m.grant.max_label, Label::Internal);
        assert_eq!(m.grant.budget.tokens, 50000);
        assert!(m.intent.starts_with("Summarize"));
    }

    // The named negative cases from R21 §7 `manifest_rejects_malformed`.
    #[test]
    fn manifest_rejects_malformed() {
        // non-.ax program
        assert!(matches!(
            p(&VALID.replace("summarize.ax", "summarize.py")),
            Err(Verdict::Malformed { .. })
        ));
        // bad exec
        assert!(matches!(
            p(&VALID.replace("\"none\"", "\"root\"")),
            Err(Verdict::Malformed { .. })
        ));
        // path traversal in fs_write
        assert!(matches!(
            p(&VALID.replace("\"./out/\"", "\"../etc/\"")),
            Err(Verdict::Malformed { .. })
        ));
        // negative budget (`50000` appears only on the tokens line)
        assert!(matches!(
            p(&VALID.replace("50000", "-1")),
            Err(Verdict::Malformed { .. })
        ));
        // missing program
        let no_prog: String = VALID
            .lines()
            .filter(|l| !l.trim_start().starts_with("program"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(p(&no_prog), Err(Verdict::Malformed { .. })));
        // bad max_label
        assert!(matches!(
            p(&VALID.replace("\"internal\"", "\"ultra\"")),
            Err(Verdict::Malformed { .. })
        ));
        // unknown key
        assert!(matches!(
            p(&format!("{VALID}\nmystery = \"x\"")),
            Err(Verdict::Malformed { .. })
        ));
    }

    #[test]
    fn omitted_dimensions_take_the_profile_default_explicitly() {
        // Product policy: easy by default. A manifest that says nothing about
        // the filesystem or the network gets the DEVELOPER profile's grant, and
        // that grant is MATERIALISED as `*` rather than left empty.
        //
        // This test used to assert the lists came back empty, which was true and
        // meaningless: an empty list was then read as "unrestricted" by the
        // runtime, so `[]` and `["*"]` behaved identically while saying opposite
        // things. The policy did not change — where it is written down did.
        let src = "program = \"pure.ax\"\n[grant]\nexec = \"none\"\nmax_label = \"public\"\n";
        let m = p(src).expect("empty grant ok");
        assert_eq!(m.seed, 42); // default
        assert_eq!(m.grant.fs_read, vec!["*".to_string()]);
        assert_eq!(m.grant.net, vec!["*".to_string()]);
        // ...but an EXPLICIT setting is still honoured, on its own axis only.
        assert_eq!(m.grant.exec, ExecPolicy::None);
        assert_eq!(m.grant.budget.calls, 0);
    }

    #[test]
    fn an_explicitly_empty_list_denies_rather_than_defaulting() {
        // The distinction the old model could not express: "I did not say" vs
        // "I said none". Omitted takes the profile default; `[]` means deny.
        let src = "program = \"pure.ax\"\n[grant]\nnet = []\nexec = \"none\"\n\
                   max_label = \"public\"\n";
        let m = p(src).expect("explicit empty ok");
        assert!(m.grant.net.is_empty(), "an explicit [] must stay empty");
        assert_eq!(
            m.grant.fs_read,
            vec!["*".to_string()],
            "and must not affect an axis the manifest did not mention"
        );
    }

    #[test]
    fn a_tighter_profile_grants_nothing_implicitly() {
        let src = "program = \"pure.ax\"\nprofile = \"hermetic\"\n[grant]\n\
                   max_label = \"public\"\n";
        let m = p(src).expect("hermetic ok");
        assert!(m.grant.net.is_empty());
        assert!(m.grant.fs_read.is_empty());
        assert!(m.grant.fs_write.is_empty());
        assert_eq!(m.grant.exec, ExecPolicy::None);
    }

    #[test]
    fn a_misspelled_profile_is_refused_not_silently_permissive() {
        let src = "program = \"pure.ax\"\nprofile = \"restrcted\"\n[grant]\n\
                   max_label = \"public\"\n";
        let e = p(src).expect_err("a typo must not yield the developer grant");
        assert!(format!("{e:?}").contains("unknown profile"), "{e:?}");
    }
}
