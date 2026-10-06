//! The refusal-site coverage gate's own guards (C9 round 4b, integrate,
//! amendment 64). `scripts/v022_refusal_coverage.py` is the check every
//! protected decision file's evidence rests on, and the freeze consults it:
//! a guard of the gate that nothing exercises is a gate that can be switched
//! off unnoticed.
//!
//! Each test runs the REAL gate, through its command line, over a scratch
//! copy of this tree's in-scope sources, registry and markers, edited in one
//! named way (the ATTACK); the CONTROL is the unedited copy, on which the gate
//! holds.

mod script_spawn;
use script_spawn::Bins;
use std::path::{Path, PathBuf};
use std::process::Output;
use std::sync::atomic::{AtomicU64, Ordering};

const GATE: &str = "scripts/v022_refusal_coverage.py";
/// What the gate reads: its registry, the markers the registry imports, and
/// what its RULE reads (amendment 71): every crate's Cargo.toml (the
/// dependency closure), every crate's sources (the protected crates' and the
/// language region's), and the guest image build script (the packages the
/// guest builds).
const SCRIPTS: [&str; 3] = [
    GATE,
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
];
const SOURCES: [&str; 3] = [
    ":(glob)crates/*/Cargo.toml",
    ":(glob)crates/*/src/**",
    "scripts/build-guest-image.sh",
];
/// A scanned file every refusal site of which has a row (tasks.rs).
const SCANNED: &str = "crates/axon-loop/src/tasks.rs";
const INTERP: &str = "crates/axon-core/src/interp.rs";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn copy(from: &Path, to: &Path) {
    if from.is_dir() {
        for e in std::fs::read_dir(from).unwrap() {
            let e = e.unwrap();
            copy(&e.path(), &to.join(e.file_name()));
        }
    } else {
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from, to).unwrap();
    }
}

fn git(args: &[&str]) -> Vec<u8> {
    let o = std::process::Command::new("/usr/bin/git")
        .arg("-C")
        .arg(repo_root())
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "setup: git {args:?}: {o:?}");
    o.stdout
}

/// A scratch copy of everything the gate reads. The gate, its registry and
/// markers come from the working tree (a mutation of the GATE is what these
/// tests judge); the scanned SOURCES come from the commit (`HEAD`), never the
/// working tree: a mutation or paired-disable cell edits a scanned source in
/// place, and these tests' controls must not judge whichever tree the cell
/// happens to leave (amendment 64, class e).
fn tree(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "axon-refusal-coverage-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    for f in SCRIPTS {
        copy(&repo_root().join(f), &d.join(f));
    }
    std::fs::create_dir_all(&d).unwrap();
    let tar = git(&[&["archive", "--format=tar", "HEAD", "--"][..], &SOURCES[..]].concat());
    let mut x = std::process::Command::new("/usr/bin/tar")
        .arg("-x")
        .arg("-C")
        .arg(&d)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        x.stdin.take().unwrap().write_all(&tar).unwrap();
    }
    assert!(
        x.wait().unwrap().success(),
        "setup: tar -x of HEAD's sources"
    );
    d
}

fn edit(r: &Path, f: &str, old: &str, new: &str) {
    let p = r.join(f);
    let s = std::fs::read_to_string(&p).unwrap();
    assert_eq!(s.matches(old).count(), 1, "setup: {old:?} once in {f}");
    std::fs::write(&p, s.replacen(old, new, 1)).unwrap();
}

/// List `f` in the copy's NOT_YET_SCANNED with `count` (every other entry
/// is kept as this tree has it).
fn not_yet_scanned(r: &Path, f: &str, count: usize) {
    edit(
        r,
        GATE,
        "\nSITE = re.compile(",
        &format!("\nNOT_YET_SCANNED[{f:?}] = {count}\nSITE = re.compile("),
    );
}

fn gate(r: &Path, args: &[&str]) -> Output {
    // Through the workspace's script helper (harness_binaries'
    // every_script_spawn_in_the_workspace_goes_through_the_helper): the gate
    // runs no binary this workspace builds.
    script_spawn::script("python3", r.join(GATE), Bins::NoWorkspaceBinary)
        .args(args)
        .current_dir(r)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env_remove("PYTHONPATH")
        .output()
        .unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

fn holds(r: &Path, args: &[&str], what: &str) {
    let o = gate(r, args);
    assert!(
        o.status.success(),
        "control ({what}): the gate must hold: {}",
        text(&o)
    );
}

/// The gate refuses with `why` in its report; ATTACK if it held.
fn refuses(r: &Path, args: &[&str], why: &str, attack: &str) {
    let o = gate(r, args);
    let t = text(&o);
    if o.status.success() {
        panic!("ATTACK: {attack}: the gate held: {t}");
    }
    assert!(t.contains(why), "{attack}: expected {why:?}: {t}");
}

/// A refusal site nothing rows or exempts: a new function in `f` (before its
/// test module, so it is code the gate reads).
const NEW_SITE: &str = "\npub fn integrate_gate_probe(x: u64) -> Result<(), String> {\n    if x > 7 {\n        return Err(format!(\"probe {x}\"));\n    }\n    Ok(())\n}\n";

fn add_site(r: &Path, f: &str) {
    let p = r.join(f);
    let s = std::fs::read_to_string(&p).unwrap();
    let at = s.find("\n#[cfg(test)]\nmod tests").unwrap_or(s.len());
    std::fs::write(&p, format!("{}{NEW_SITE}{}", &s[..at], &s[at..])).unwrap();
}

/// Amendment 61's re-measured count: a NOT YET SCANNED file's listed count
/// is re-measured on every run, so a refusal site added to such a file is
/// reported (the count no longer matches) rather than hidden behind the
/// listing. Control: the listing that matches the measurement holds.
#[test]
fn a_new_site_in_a_not_yet_scanned_file_is_reported() {
    let r = tree("count");
    not_yet_scanned(&r, SCANNED, 0);
    holds(
        &r,
        &[],
        "a NOT YET SCANNED listing whose count is the measured one",
    );
    add_site(&r, SCANNED);
    refuses(
        &r,
        &[],
        "NOT_YET_SCANNED says 0 uncovered sites, measured 1",
        "a refusal site added to a NOT YET SCANNED file went unreported behind a stale count",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 61: under --freeze, a non-empty NOT_YET_SCANNED is itself a
/// failure, so no freeze binds evidence over a decision file nobody scanned.
/// Control: the same tree holds without --freeze (the count is right).
#[test]
fn a_freeze_reading_refuses_a_not_yet_scanned_file() {
    let r = tree("freeze");
    not_yet_scanned(&r, SCANNED, 0);
    holds(&r, &[], "the same listing outside a freeze");
    refuses(
        &r,
        &["--freeze"],
        "NOT YET SCANNED at a freeze",
        "the freeze reading of the gate held with a protected file NOT YET SCANNED",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 64: interp.rs is scanned over the UNION of the seal|conform|cast
/// functions and the anchored region (REGIONS). A refusal added in the region
/// but in a function whose name the rule does not select (rng_guard's
/// neighbourhood) is still a site. Control: the unedited copy holds.
#[test]
fn a_site_in_the_anchored_region_outside_a_seal_fn_is_scanned() {
    let r = tree("region");
    holds(&r, &[], "the unedited copy");
    edit(
        &r,
        INTERP,
        "    fn rng_guard(&self) -> Result<(), Flow> {\n",
        "    fn rng_guard(&self) -> Result<(), Flow> {\n        if self.sealed_frames.get() > 99 {\n            return panic(\"integrate gate probe\");\n        }\n",
    );
    refuses(
        &r,
        &[],
        "refusal site with no row and no exemption",
        "an unrowed refusal in the anchored seal region was not scanned",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 64: a REGIONS anchor must name one place; a second copy of it
/// would let the region start (or end) wherever the first copy is, silently.
/// Control: the unedited copy holds.
#[test]
fn a_region_anchor_that_is_not_unique_is_refused() {
    let r = tree("anchor");
    holds(&r, &[], "the unedited copy");
    let start = "    /// Whether `f` was defined in a sealed (candidate) module.\n";
    edit(
        &r,
        INTERP,
        "    fn rng_guard(&self) -> Result<(), Flow> {\n",
        &format!("{start}    fn rng_guard(&self) -> Result<(), Flow> {{\n"),
    );
    refuses(
        &r,
        &[],
        "a REGIONS anchor is not in the file exactly once",
        "a REGIONS anchor that is not unique was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 4c, r4c-fixes part 2 (amendment 71): the CRATE rule and the
// broadened refusal forms. Each attack adds decision code the old rule could
// not see; the gate must name it.

/// The gate names `site` in a refusal; ATTACK if its report does not name it
/// (whether it held or failed on something else: a gate that stops scanning a
/// form also orphans the exemptions of that form, and that failure must not
/// pass for naming the site).
fn names(r: &Path, site: &str, attack: &str) {
    let o = gate(r, &[]);
    let t = text(&o);
    if !t
        .lines()
        .any(|l| l.contains("refusal site with no row and no exemption") && l.contains(site))
    {
        panic!("ATTACK: {attack}: the gate did not name it: {t}");
    }
    assert!(!o.status.success(), "{attack}: named but held: {t}");
}

/// A new workspace crate `name` whose library holds one unrowed refusal site.
fn add_crate(r: &Path, name: &str) {
    let d = r.join("crates").join(name);
    std::fs::create_dir_all(d.join("src")).unwrap();
    std::fs::write(
        d.join("Cargo.toml"),
        format!("[package]\nname = \"{name}\"\nversion = \"0.1.0\"\nedition = \"2021\"\n"),
    )
    .unwrap();
    std::fs::write(d.join("src/lib.rs"), NEW_SITE).unwrap();
}

/// Amendment 71: a crate a protected crate LINKS (a normal `path`
/// dependency) is in scope the day it appears, whatever its directory.
/// Control: the same new crate, linked by nothing, is not scanned.
#[test]
fn a_crate_a_protected_crate_links_is_scanned() {
    let r = tree("closure");
    add_crate(&r, "axon-gate-probe-dep");
    edit(
        &r,
        "crates/axon-psv/Cargo.toml",
        "[dependencies]\n",
        "[dependencies]\naxon-gate-probe-dep = { path = \"../axon-gate-probe-dep\" }\n",
    );
    names(
        &r,
        "crates/axon-gate-probe-dep/src/lib.rs:",
        "a refusal site in a crate the PSV crate links was not scanned",
    );
    let c = tree("axon-gate-probe-dep-control");
    add_crate(&c, "axon-gate-probe-dep");
    holds(&c, &[], "a new crate nothing protected links");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 71: a package the guest image builds is protected code. Control:
/// the crate exists, unbuilt, and is not scanned.
#[test]
fn a_package_the_guest_image_builds_is_scanned() {
    let r = tree("guestpkg");
    add_crate(&r, "axon-gate-probe-guest");
    edit(
        &r,
        "scripts/build-guest-image.sh",
        "build_initramfs() {\n",
        "build_initramfs() {\n    gcargo -- build -p axon-gate-probe-guest --release || exit 1\n",
    );
    names(
        &r,
        "crates/axon-gate-probe-guest/src/lib.rs:",
        "a refusal site in a package the guest image builds was not scanned",
    );
    let c = tree("axon-gate-probe-guest-control");
    add_crate(&c, "axon-gate-probe-guest");
    holds(&c, &[], "a new crate the guest does not build");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// A fully scanned protected binary (the guest's PID 1) and the start of its
/// `main`, where the probes below are planted.
const GUEST_INIT: &str = "crates/axon-guest-init/src/main.rs";
const GUEST_MAIN: &str = "fn main() {\n    let args: Vec<String> = env::args().collect();\n";

/// Amendment 71: a process exit with a non-zero code and a compile refusal
/// (`Diagnostic::error`) are refusal sites. Control: the unedited copy holds.
#[test]
fn an_exit_or_a_compile_refusal_is_a_site() {
    let r = tree("forms");
    edit(
        &r,
        GUEST_INIT,
        GUEST_MAIN,
        &format!("{GUEST_MAIN}    if args.len() > 98 {{\n        std::process::exit(4);\n    }}\n"),
    );
    names(
        &r,
        "std::process::exit(4)",
        "a non-zero process exit in a protected binary was not a refusal site",
    );
    let r2 = tree("forms-diag");
    edit(
        &r2,
        "crates/axon-core/src/resolver.rs",
        "        let is_sealed = |span: crate::span::Span| span_in_sealed(span, sealed);\n",
        "        let is_sealed = |span: crate::span::Span| span_in_sealed(span, sealed);\n        if program.items.len() > 99_999 {\n            self.emit_error(Diagnostic::error(E0004, \"gate probe\"));\n        }\n",
    );
    names(
        &r2,
        "Diagnostic::error(E0004, \"gate probe\")",
        "a compile refusal in the resolver's seal edge was not a refusal site",
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&r2);
    let c = tree("forms-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
}

/// Amendment 71: a call of a file-local refusal constructor (here the
/// guest PID 1's diverging `exec_process`, which exits non-zero) is a site. Control: the unedited copy holds.
#[test]
fn a_call_of_a_local_refusal_constructor_is_a_site() {
    let r = tree("ctor");
    edit(
        &r,
        GUEST_INIT,
        GUEST_MAIN,
        &format!(
            "{GUEST_MAIN}    if args.len() > 97 {{\n        exec_process(\"/gate/probe\", &[]);\n    }}\n"
        ),
    );
    names(
        &r,
        "exec_process(\"/gate/probe\", &[])",
        "a call of a diverging refusal constructor (`exec_process`) was not a refusal site",
    );
    let _ = std::fs::remove_dir_all(&r);
    let c = tree("ctor-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
}

// ── C9 round 4c, GATE (amendment 74): a predicate primitive is a site, a row
// covers only what its edit CHANGES, and the guard block of a site stays inside
// its own function.

/// Append `code` to `f` before its test module (code the gate reads).
fn add_code(r: &Path, f: &str, code: &str) {
    let p = r.join(f);
    let s = std::fs::read_to_string(&p).unwrap();
    let at = s.find("\n#[cfg(test)]\nmod tests").unwrap_or(s.len());
    std::fs::write(&p, format!("{}\n{code}{}", &s[..at], &s[at..])).unwrap();
}

/// Give the copy of the gate one more exemption.
fn exempt(r: &Path, f: &str, anchor: &str, reason: &str) {
    edit(
        r,
        GATE,
        "\n\ndef load_rows():",
        &format!("\nEXEMPT.append(({f:?}, {anchor:?}, {reason:?}))\n\n\ndef load_rows():"),
    );
}

/// The gate holds on `r`; ATTACK (the gate's own failure) if it does not.
fn must_hold(r: &Path, args: &[&str], attack: &str) {
    let o = gate(r, args);
    if !o.status.success() {
        panic!("ATTACK: {attack}: the gate refused: {}", text(&o));
    }
}

/// Amendment 74: a function whose declared return type is `bool` or
/// `Option<..>` DECIDES, and is one site (round 4c found `keyed_outcome`, which
/// refuses by `return None` / `.then_some`, invisible to the gate). Control:
/// the unedited copy holds.
#[test]
fn a_function_that_decides_by_bool_or_option_is_a_site() {
    let r = tree("pred");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_decides(x: u64) -> bool {\n    x % 7 == 0\n}\n\npub fn gate_probe_option(x: u64) -> Option<u64> {\n    if x > 9 {\n        return None;\n    }\n    Some(x)\n}\n",
    );
    names(
        &r,
        "fn gate_probe_decides(x: u64) -> bool",
        "a function deciding by bool, with no row and no exemption, was not a site",
    );
    names(
        &r,
        "fn gate_probe_option(x: u64) -> Option<u64>",
        "a function deciding by Option (a `return None` refusal), with no row and no exemption, was not a site",
    );
    let c = tree("pred-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 74: a row covers a site only when a line its edit CHANGES lies in
/// the site's guard block. Round 4c: M123's `old` ended in a newline, so it
/// also "covered" the next line (evl.rs `else if !issuer_ok`), which its edit
/// never touches. Attack 1: with the row that really edits that site removed,
/// the gate must name it although M123's old text overlaps it. Attack 2: a row
/// whose new text equals its old (an edit that changes nothing) covers
/// nothing.
#[test]
fn a_row_covers_only_what_its_edit_changes() {
    let r = tree("changed");
    let o = gate(&r, &["--without=M1726"]);
    let t = text(&o);
    if o.status.success()
        || !t.lines().any(|l| {
            l.contains("refusal site with no row and no exemption")
                && l.contains("crates/axon-loop/src/evl.rs")
                && l.contains("!issuer_ok")
        })
    {
        panic!(
            "ATTACK: a row whose old text merely overlaps a site (M123's trailing newline) covered \
             evl.rs's `!issuer_ok` refusal: {t}"
        );
    }
    edit(
        &r,
        "scripts/v022_g01_mutations.py",
        "'            } else if false && !issuer_ok {',",
        "'            } else if !issuer_ok {',",
    );
    let o = gate(&r, &[]);
    let t = text(&o);
    if !t.lines().any(|l| {
        l.contains("refusal site with no row and no exemption") && l.contains("!issuer_ok")
    }) {
        panic!("ATTACK: a row whose edit changes nothing covered a refusal site: {t}");
    }
    let c = tree("changed-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 74: an exemption of a predicate primitive is anchored on its HEAD
/// line. One anchored in its body is a line site's, and must not exempt the
/// function. Control: the head-line anchor does.
#[test]
fn an_exemption_in_a_predicate_fns_body_does_not_exempt_the_fn() {
    let code = "pub fn gate_probe_body(x: u64) -> Option<u64> {\n    (x > 7).then_some(x)\n}\n";
    let r = tree("exbody");
    add_code(&r, SCANNED, code);
    exempt(
        &r,
        SCANNED,
        "    (x > 7).then_some(x)",
        "probe: the line site in the body",
    );
    names(
        &r,
        "fn gate_probe_body(x: u64) -> Option<u64>",
        "an exemption anchored in a predicate function's body exempted the function itself",
    );
    let c = tree("exhead");
    add_code(&c, SCANNED, code);
    exempt(
        &c,
        SCANNED,
        "pub fn gate_probe_body(x: u64) -> Option<u64> {",
        "probe: the function",
    );
    exempt(
        &c,
        SCANNED,
        "    (x > 7).then_some(x)",
        "probe: the line site in the body",
    );
    holds(
        &c,
        &[],
        "a head-line exemption of the function plus the body's own",
    );
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 74: a `let .. else {` is the opener of its refusal, so a site in
/// its block is exempted by an anchor on the `let` line. Control: the same
/// probe with its anchor holds.
#[test]
fn a_let_else_is_the_opener_of_its_refusal() {
    let r = tree("letelse");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_le(x: Option<u64>) -> Result<u64, String> {\n    let Some(v) = x else {\n        return Err(format!(\"probe\"));\n    };\n    Ok(v)\n}\n",
    );
    exempt(
        &r,
        SCANNED,
        "    let Some(v) = x else {",
        "probe: the let-else line",
    );
    must_hold(
        &r,
        &[],
        "an exemption anchored on a `let .. else {` line did not reach the refusal in its block",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 74: a site's guard block never reaches into the function above
/// (the nearest opener used to be found across a function boundary, so an
/// exemption in the PREVIOUS function exempted this one's refusal).
#[test]
fn a_guard_block_does_not_cross_a_function_boundary() {
    let r = tree("fnstop");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_above(x: u64) -> Result<(), String> {\n    if x > 5 {\n        return Err(format!(\"above {x}\"));\n    }\n    Ok(())\n}\n\npub fn gate_probe_below(x: u64) -> Result<(), String> {\n    Err(format!(\"below {x}\"))\n}\n",
    );
    exempt(
        &r,
        SCANNED,
        "    if x > 5 {",
        "probe: the function above's own refusal",
    );
    names(
        &r,
        "Err(format!(\"below {x}\"))",
        "an exemption in the function above exempted the refusal of the function below it",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 74: only a `#[cfg(test)]` item's own extent is hidden. The rule
/// used to drop everything after the FIRST `#[cfg(test)] mod tests`: production
/// code after the test module (axon-os approval.rs's `authorize`) and anything
/// below an EMPTY stub module were never scanned. Attack (a): an empty test
/// module above a production refusal. Attack (b): a real test module (with
/// braces in strings, chars and comments) with production code after it.
/// Control: the unedited copy holds, and a refusal INSIDE the test module is
/// still not a site.
#[test]
fn production_code_after_a_test_module_is_scanned() {
    let probe = "pub fn gate_probe_hidden(x: u64) -> Result<(), String> {\n    Err(format!(\"hidden production refusal {x}\"))\n}\n";
    let r = tree("cfg-empty");
    add_code(
        &r,
        SCANNED,
        &format!("#[cfg(test)]\nmod tests {{}}\n\n{probe}"),
    );
    names(
        &r,
        "hidden production refusal",
        "a production refusal below an empty `#[cfg(test)] mod tests` was not scanned",
    );
    let r2 = tree("cfg-after");
    add_code(
        &r2,
        SCANNED,
        &format!(
            "#[cfg(test)]\nmod tests {{\n    fn t() -> Result<(), String> {{\n        let _s = \"}}{{\"; let _c = '}}'; /* }} */\n        Err(format!(\"inside the test module\"))\n    }}\n}}\n\n{probe}"
        ),
    );
    names(
        &r2,
        "hidden production refusal",
        "a production refusal after a real test module (braces in strings, chars, comments) was not scanned",
    );
    let o = gate(&r2, &[]);
    assert!(
        !text(&o).contains("inside the test module"),
        "control: a refusal inside the test module is not a site: {}",
        text(&o)
    );
    let c = tree("cfg-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r2);
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 4c, ADMIT (amendment 76): a VERDICT is a decision. The rule used
// to see `Err(`, a bool/Option return and the named constructors; the axon-os
// admission chain refuses by RETURNING an enum value (`Admission::Deny {..}`,
// `Verdict::Denied {..}`) and the gate could not see it. The forms below are
// derived from the in-scope code (every enum with a refusal-named variant or a
// deciding name), never from a list of today's names.

/// The gate does NOT name `site` as a refusal site (a pattern, a comparison, a
/// test, a data enum): a rule that over-reads makes every exemption noise.
fn not_named(r: &Path, site: &str, what: &str) {
    let t = text(&gate(r, &[]));
    if t.lines()
        .any(|l| l.contains("refusal site with no row and no exemption") && l.contains(site))
    {
        panic!("ATTACK: {what}: the gate named it as a refusal site: {t}");
    }
}

const PROBE_ENUM: &str = "pub enum GateProbeVerdict {\n    Allowed,\n    Denied { why: String },\n}\n\npub enum GateProbeBilling {\n    Known(u64),\n    Unknown,\n}\n\npub enum GateProbeOutcome {\n    Granted,\n    Unknown,\n}\n\npub enum GateProbeError {\n    Denied(String),\n}\n";

/// Amendment 76: a NEGATIVE variant of a verdict enum, built, is a site: in a
/// `return`, in a binding, as an arm's value. The enum is a verdict enum
/// because the code says so (a refusal-named variant, or a deciding name with
/// a "no verdict" variant); a data enum with an `Unknown` variant (a billing
/// state) and an `*Error` enum (whose refusals travel in `Err(` and are sites
/// already) are not. Matching a verdict is not deciding one.
#[test]
fn a_built_negative_verdict_variant_is_a_site() {
    let r = tree("verdict-variant");
    add_code(
        &r,
        SCANNED,
        &format!(
            "{PROBE_ENUM}\npub fn gate_probe_stamp(x: u64) -> String {{\n    let v = if x > 7 {{\n        GateProbeVerdict::Denied {{ why: format!(\"stamp {{x}}\") }}\n    }} else {{\n        GateProbeVerdict::Allowed\n    }};\n    let o = if x > 9 {{ GateProbeOutcome::Unknown }} else {{ GateProbeOutcome::Granted }};\n    let b = if x > 11 {{ GateProbeBilling::Unknown }} else {{ GateProbeBilling::Known(x) }};\n    let _ = (o, b);\n    format!(\"{{}}\", matches!(v, GateProbeVerdict::Allowed))\n}}\n\npub fn gate_probe_reads(v: &GateProbeVerdict, o: GateProbeOutcome) -> u64 {{\n    if let GateProbeVerdict::Denied {{ why }} = v {{\n        return why.len() as u64;\n    }}\n    let n = match v {{\n        GateProbeVerdict::Denied {{ .. }} => 1,\n        GateProbeVerdict::Allowed => 2,\n    }};\n    let m = matches!(v, GateProbeVerdict::Denied {{ .. }});\n    if m || o == GateProbeOutcome::Unknown {{\n        return n + 1;\n    }}\n    n\n}}\n\npub fn gate_probe_tuple_pattern(v: &GateProbeVerdict, n: u64) -> u64 {{\n    match (v, n) {{\n        (GateProbeVerdict::Denied {{ .. }}, 0) => 3,\n        _ => 4,\n    }}\n}}\n\npub fn gate_probe_err_enum(x: u64) -> u64 {{\n    let e = GateProbeError::Denied(format!(\"as data {{x}}\"));\n    let _ = e;\n    0\n}}\n"
        ),
    );
    names(
        &r,
        "GateProbeVerdict::Denied { why: format!(\"stamp {x}\") }",
        "a built negative verdict variant (Verdict::Denied {..}) with no row and no exemption was not a site",
    );
    names(
        &r,
        "GateProbeOutcome::Unknown",
        "a built `Unknown` of an enum named for deciding (an Outcome) with no row and no exemption was not a site",
    );
    not_named(
        &r,
        "GateProbeBilling::Unknown",
        "a data enum's `Unknown` (a billing state, no deciding name, no refusal variant) was read as a verdict",
    );
    not_named(
        &r,
        "GateProbeError::Denied(format!",
        "an `*Error` enum's variant (its refusals travel inside Err( and are sites there) was read as a verdict",
    );
    not_named(
        &r,
        "if let GateProbeVerdict::Denied",
        "a PATTERN (`if let Verdict::Denied {..} = v`) was read as a decision",
    );
    not_named(
        &r,
        "GateProbeVerdict::Denied { .. } => 1",
        "a match ARM pattern was read as a decision",
    );
    not_named(
        &r,
        "let m = matches!(v, GateProbeVerdict::Denied",
        "a `matches!` pattern was read as a decision",
    );
    not_named(
        &r,
        "(GateProbeVerdict::Denied { .. }, 0) => 3",
        "a TUPLE pattern in a match arm was read as a decision",
    );
    let c = tree("verdict-variant-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: `Self::Variant` inside the enum's own `impl` is the same
/// construction (axon-loop and axon-os build their verdicts that way).
#[test]
fn a_self_variant_in_the_verdicts_own_impl_is_a_site() {
    let r = tree("verdict-self");
    add_code(
        &r,
        SCANNED,
        &format!(
            "{PROBE_ENUM}\nimpl GateProbeVerdict {{\n    pub fn gate_probe_describe(&self, x: u64) -> String {{\n        let v = if x > 3 {{ Self::Denied {{ why: \"self-built\".into() }} }} else {{ Self::Allowed }};\n        format!(\"{{}}\", matches!(v, Self::Allowed))\n    }}\n}}\n"
        ),
    );
    names(
        &r,
        "Self::Denied { why: \"self-built\".into() }",
        "a negative variant built through `Self::` inside the verdict enum's impl was not a site",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: a function whose declared return type is a verdict (an enum
/// the code derives, or a struct named for deciding), also through one
/// Result/Option or a tuple, DECIDES, and is one site of its own: its "no"
/// may be data flow (`let status = f(); Verdict { status }`), not a built
/// variant. Exempt on its HEAD line, like a predicate primitive.
#[test]
fn a_function_that_returns_a_verdict_is_a_site() {
    let r = tree("verdict-fn");
    add_code(
        &r,
        SCANNED,
        &format!(
            "{PROBE_ENUM}\npub struct GateProbeDecision {{\n    pub ok: bool,\n}}\n\npub fn gate_probe_flow(x: u64) -> GateProbeDecision {{\n    let ok = x % 2 == 0;\n    GateProbeDecision {{ ok }}\n}}\n\npub fn gate_probe_tuple(x: u64) -> (GateProbeVerdict, u64) {{\n    (GateProbeVerdict::Allowed, x)\n}}\n\npub fn gate_probe_wrapped(x: u64) -> Result<GateProbeOutcome, String> {{\n    Ok(if x > 1 {{ GateProbeOutcome::Granted }} else {{ GateProbeOutcome::Granted }})\n}}\n"
        ),
    );
    names(
        &r,
        "fn gate_probe_flow(x: u64) -> GateProbeDecision",
        "a function returning a struct named for deciding (data-flow refusal) with no row and no exemption was not a site",
    );
    names(
        &r,
        "fn gate_probe_tuple(x: u64) -> (GateProbeVerdict, u64)",
        "a function returning a verdict inside a tuple was not a site",
    );
    names(
        &r,
        "fn gate_probe_wrapped(x: u64) -> Result<GateProbeOutcome, String>",
        "a function returning a verdict inside a Result was not a site",
    );
    // Exempting on the head line exempts the function, and only it.
    exempt(
        &r,
        SCANNED,
        "pub fn gate_probe_flow(x: u64) -> GateProbeDecision {",
        "probe: the function",
    );
    not_named(
        &r,
        "fn gate_probe_flow(x: u64)",
        "a head-line exemption did not exempt a verdict function",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: a call of a HELPER CONSTRUCTOR is the site, its definition is
/// not (`fn deny(..) -> Verdict { Verdict::Denied {..} }`, the verdict
/// analogue of `refused(`). A same-named function in ANOTHER file is another
/// function.
#[test]
fn a_call_of_a_verdict_helper_constructor_is_a_site() {
    let r = tree("verdict-helper");
    add_code(
        &r,
        SCANNED,
        &format!(
            "{PROBE_ENUM}\nfn gate_probe_deny(why: &str) -> GateProbeVerdict {{\n    GateProbeVerdict::Denied {{ why: why.into() }}\n}}\n\npub fn gate_probe_caller(x: u64) -> u64 {{\n    if x > 7 {{\n        let _d = gate_probe_deny(\"helper call\");\n        return 1;\n    }}\n    0\n}}\n"
        ),
    );
    names(
        &r,
        "let _d = gate_probe_deny(\"helper call\");",
        "a call of a helper that only builds a negative verdict, with no row and no exemption, was not a site",
    );
    not_named(
        &r,
        "GateProbeVerdict::Denied { why: why.into() }",
        "the helper constructor's own body was named instead of its calls",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: the same for a helper that builds an `Err(..)` and nothing
/// else (`fn check_operator_owned(..)`, `fn no_xattr(..)`): a call is a site.
#[test]
fn a_call_of_an_err_helper_constructor_is_a_site() {
    let r = tree("err-helper");
    add_code(
        &r,
        SCANNED,
        "fn gate_probe_refuse(m: &str) -> Result<(), String> {\n    Err(m.to_string())\n}\n\npub fn gate_probe_calls_refuse(x: u64) -> Result<u64, String> {\n    if x > 7 {\n        gate_probe_refuse(\"err helper call\")?;\n    }\n    Ok(x)\n}\n",
    );
    names(
        &r,
        "gate_probe_refuse(\"err helper call\")?;",
        "a call of a function whose whole body is an Err(..), with no row and no exemption, was not a site",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: `.ok_or(..)` converts an absence some other decision made,
/// and is no site; with a predicate INLINE before it (`.filter(|k| k.len() ==
/// 32).ok_or(..)?`) the predicate IS the decision and nothing else scans it.
#[test]
fn an_inline_predicate_refused_through_ok_or_is_a_site() {
    let r = tree("inline-pred");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_key(k: Option<Vec<u8>>) -> Result<Vec<u8>, String> {\n    let key = k\n        .filter(|k| k.len() == 32)\n        .ok_or(\"not a 32-byte key\")?;\n    Ok(key)\n}\n\npub fn gate_probe_lookup(k: Option<Vec<u8>>) -> Result<Vec<u8>, String> {\n    let key = k.ok_or(\"absent lookup\")?;\n    Ok(key)\n}\n",
    );
    names(
        &r,
        ".filter(|k| k.len() == 32)",
        "a closure predicate refused through ok_or, with no row and no exemption, was not a site",
    );
    not_named(
        &r,
        "k.ok_or(\"absent lookup\")",
        "a bare ok_or (an absence some other decision made) was read as a decision",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: a refusal guarded by `matches!` (`if !matches!(..) { return
/// Err(..) }`, the condition over several lines) is a site whose block starts
/// at the `if`.
#[test]
fn a_matches_guard_opens_its_refusal() {
    let r = tree("matches-guard");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_matches(x: Option<u64>) -> Result<(), String> {\n    if !matches!(\n        x,\n        Some(1) | Some(2)\n    ) {\n        return Err(format!(\"matches guard\"));\n    }\n    Ok(())\n}\n",
    );
    exempt(
        &r,
        SCANNED,
        "    if !matches!(",
        "probe: the matches! guard line",
    );
    must_hold(
        &r,
        &[],
        "an exemption anchored on a `if !matches!(` line did not reach the refusal in its block",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 76: `#[cfg(all(test, ..))]` compiles only in a test build, so its
/// item is test code (the psv crate's xattr module read as production);
/// `#[cfg(any(test, feature = ..))]` and `#[cfg(not(test))]` are production.
#[test]
fn a_cfg_all_test_item_is_test_code_and_a_cfg_any_test_item_is_not() {
    let r = tree("cfg-all");
    add_code(
        &r,
        SCANNED,
        "#[cfg(all(test, target_os = \"linux\"))]\nmod gate_probe_linux_tests {\n    pub fn t() -> Result<(), String> {\n        Err(format!(\"inside the all-test module\"))\n    }\n}\n\n#[cfg(any(test, feature = \"gate-probe\"))]\npub fn gate_probe_any() -> Result<(), String> {\n    Err(format!(\"inside the any-test fn\"))\n}\n",
    );
    names(
        &r,
        "inside the any-test fn",
        "a `cfg(any(test, feature))` item (production code under the feature) was hidden as test code",
    );
    not_named(
        &r,
        "inside the all-test module",
        "a `cfg(all(test, ..))` item (compiled only in a test build) was read as production",
    );
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 4c, EQGATE (amendment 81): a refusal done by the KERNEL through an
// open flag, a decision returned as `Some(reason)`, and a status returned as an
// exit code, `Result<bool>` or `i32`. Each form below is planted in a
// production-shaped line the gate must name; the flag forms are the ones the
// round's EQUIVALENCE review found removable with every root-run suite green.

const FLAG_PROBES: &str = "pub fn gate_probe_flags(p: &std::path::Path) {\n    use std::os::unix::fs::OpenOptionsExt;\n    let gate_probe_nofollow = std::fs::OpenOptions::new().custom_flags(libc::O_NOFOLLOW).open(p);\n    let gate_probe_excl = unsafe { libc::open(c\"/x\".as_ptr(), libc::O_CREAT | libc::O_EXCL, 0o600) };\n    let gate_probe_new = std::fs::OpenOptions::new().write(true).create_new(true).open(p);\n    let gate_probe_noreplace = unsafe { libc::renameat2(libc::AT_FDCWD, c\"/a\".as_ptr(), libc::AT_FDCWD, c\"/b\".as_ptr(), libc::RENAME_NOREPLACE) };\n    let gate_probe_atnofollow = unsafe { libc::fstatat(0, c\"/x\".as_ptr(), std::ptr::null_mut(), libc::AT_SYMLINK_NOFOLLOW) };\n    let gate_probe_dir = unsafe { libc::open(c\"/x\".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };\n    let gate_probe_mount = libc::MS_NOSUID | libc::MS_NODEV;\n    let _ = (gate_probe_nofollow, gate_probe_excl, gate_probe_new, gate_probe_noreplace, gate_probe_atnofollow, gate_probe_dir, gate_probe_mount);\n}\n";

/// Amendment 81: every USE of an atomic-refusal flag in production code is a
/// site: O_NOFOLLOW, O_EXCL (O_CREAT|O_EXCL), `create_new(true)`,
/// RENAME_NOREPLACE, AT_SYMLINK_NOFOLLOW, O_DIRECTORY, and the mount flags
/// (MS_NOSUID ...). Control: `create_new(false)` and a flag named in a comment
/// are not.
#[test]
fn an_open_flag_form_is_a_site() {
    let r = tree("open-flags");
    add_code(&r, SCANNED, FLAG_PROBES);
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_not_flags(p: &std::path::Path) {\n    let gate_probe_plain = std::fs::OpenOptions::new().write(true).create_new(false).open(p); // O_NOFOLLOW is the old way\n    let _ = gate_probe_plain;\n}\n",
    );
    for (site, form) in [
        ("gate_probe_nofollow", "O_NOFOLLOW"),
        ("gate_probe_excl", "O_EXCL"),
        ("gate_probe_new", "create_new(true)"),
        ("gate_probe_noreplace", "RENAME_NOREPLACE"),
        ("gate_probe_atnofollow", "AT_SYMLINK_NOFOLLOW"),
        ("gate_probe_dir", "O_DIRECTORY"),
        ("gate_probe_mount", "MS_NOSUID (a mount flag)"),
    ] {
        names(
            &r,
            site,
            &format!("a use of {form} with no row and no exemption was not a refusal site"),
        );
    }
    not_named(
        &r,
        "gate_probe_plain",
        "`create_new(false)` with a flag named only in a trailing comment was read as an atomic-refusal flag",
    );
    let c = tree("open-flags-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 81: a decision expressed as `Some("reason")` / `Some(format!(..))`
/// is a site (evo::propose's exclusion chain, `problem = Some(..)`); matching
/// or comparing one (`Some("x") =>`, `== Some("x")`, `matches!(.., Some("x"))`)
/// reads a decision somebody else made.
#[test]
fn a_some_reason_value_is_a_site_and_a_pattern_is_not() {
    let r = tree("some-reason");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_chain(x: u64) -> String {\n    let reason = if x > 3 {\n        Some(\"gate probe: too big\")\n    } else {\n        None\n    };\n    format!(\"{reason:?}\")\n}\n\npub fn gate_probe_reads(x: Option<&str>) -> u64 {\n    let a = x == Some(\"gate-probe-eq\");\n    let b = matches!(x, Some(\"gate-probe-matches\"));\n    let c = match x {\n        Some(\"gate-probe-arm\") => 1,\n        _ => 2,\n    };\n    u64::from(a) + u64::from(b) + c\n}\n",
    );
    names(
        &r,
        "Some(\"gate probe: too big\")",
        "a decision returned as Some(\"reason\") with no row and no exemption was not a site",
    );
    not_named(&r, "x == Some(\"gate-probe-eq\")", "a comparison with Some(\"..\") was read as a decision");
    not_named(&r, "matches!(x, Some(\"gate-probe-matches\"))", "a matches! of Some(\"..\") was read as a decision");
    not_named(&r, "Some(\"gate-probe-arm\") => 1", "a match arm on Some(\"..\") was read as a decision");
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 81: a function that decides by `Result<bool, _>`, an `i32` status
/// or an `ExitCode` is a site of its own (its body the guard block), like a
/// `bool`/`Option` predicate.
#[test]
fn a_function_deciding_by_result_bool_i32_or_exitcode_is_a_site() {
    let r = tree("status-fns");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_result_bool(x: u64) -> Result<bool, String> {\n    Ok(x > 3)\n}\n\npub fn gate_probe_status_i32(x: u64) -> i32 {\n    if x > 3 {\n        return 4;\n    }\n    0\n}\n\npub fn gate_probe_exit_code(x: u64) -> std::process::ExitCode {\n    if x > 3 {\n        return std::process::ExitCode::from(8);\n    }\n    std::process::ExitCode::SUCCESS\n}\n",
    );
    names(
        &r,
        "fn gate_probe_result_bool(x: u64) -> Result<bool, String>",
        "a function deciding by Result<bool, _> with no row and no exemption was not a site",
    );
    names(
        &r,
        "fn gate_probe_status_i32(x: u64) -> i32",
        "a function returning an i32 status with no row and no exemption was not a site",
    );
    names(
        &r,
        "fn gate_probe_exit_code(x: u64) -> std::process::ExitCode",
        "a function returning an ExitCode (`return ExitCode::from(8)`) with no row and no exemption was not a site",
    );
    let _ = std::fs::remove_dir_all(&r);
}
