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
const SOURCES: [&str; 4] = [
    ":(glob)crates/*/Cargo.toml",
    ":(glob)crates/*/src/**",
    "scripts/build-guest-image.sh",
    // Amendment 98: the Python guards of the build environment.
    "scripts/guest_build_env.py",
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
    // Integration (amendment 99): the control is run on a SECOND copy and AFTER the
    // attack. The anchored region now holds an exemption (psv1g's `is_global`), so a
    // gate that does not scan the region ALSO reports that exemption as matching no
    // site, and a control run first would fail on that before the planted site is ever
    // judged: the row would read REFUSED_ELSEWHERE, not killed. The attack names the
    // planted site itself.
    let r = tree("region");
    edit(
        &r,
        INTERP,
        "    fn rng_guard(&self) -> Result<(), Flow> {\n",
        "    fn rng_guard(&self) -> Result<(), Flow> {\n        if self.sealed_frames.get() > 99 {\n            return panic(\"integrate gate probe\");\n        }\n",
    );
    names(
        &r,
        "self.sealed_frames.get() > 99",
        "an unrowed refusal in the anchored seal region was not scanned",
    );
    let c = tree("region-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
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
    // A VALUE site (amendment 103) is a different kind of report line ("...: value (val_x) ..."):
    // this helper asserts what is, or is not, a LINE site. A probe line that holds a literal value
    // is named as a value site too, and its text is quoted in that line.
    if t.lines().any(|l| {
        l.contains("refusal site with no row and no exemption")
            && !l.contains("exemption: value (")
            && l.contains(site)
    }) {
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

/// Amendment 76: with a predicate INLINE before it (`.filter(|k| k.len() ==
/// 32).ok_or(..)?`) the predicate IS the decision and nothing else scans it.
/// Amendment 95 REVERSES the other half of this test: a bare single-line
/// `.ok_or(..)?` is a site too (the absent value is a refusal the function
/// returns; amendment 76 had judged it "an absence some other decision made",
/// which left 71 refusals unseen); see `a_single_line_ok_or_refusal_is_a_site`.
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
    names(
        &r,
        "k.ok_or(\"absent lookup\")",
        "a bare single-line ok_or(..)? (amendment 95) was not a site",
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

const FLAG_PROBES: &str = "pub fn gate_probe_flags(p: &std::path::Path) {\n    use std::os::unix::fs::OpenOptionsExt;\n    let gate_probe_nofollow = std::fs::OpenOptions::new().custom_flags(libc::O_NOFOLLOW).open(p);\n    let gate_probe_excl = libc::O_CREAT | libc::O_EXCL;\n    let gate_probe_new = std::fs::OpenOptions::new().write(true).create_new(true).open(p);\n    let gate_probe_noreplace = libc::RENAME_NOREPLACE;\n    let gate_probe_atnofollow = unsafe { libc::fstatat(0, c\"/x\".as_ptr(), std::ptr::null_mut(), libc::AT_SYMLINK_NOFOLLOW) };\n    let gate_probe_dir = unsafe { libc::open(c\"/x\".as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY) };\n    let gate_probe_mount = libc::MS_NODEV;\n    let _ = (gate_probe_nofollow, gate_probe_excl, gate_probe_new, gate_probe_noreplace, gate_probe_atnofollow, gate_probe_dir, gate_probe_mount);\n}\n";

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
        ("gate_probe_mount", "MS_NODEV (a mount flag)"),
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
    not_named(
        &r,
        "x == Some(\"gate-probe-eq\")",
        "a comparison with Some(\"..\") was read as a decision",
    );
    not_named(
        &r,
        "matches!(x, Some(\"gate-probe-matches\"))",
        "a matches! of Some(\"..\") was read as a decision",
    );
    not_named(
        &r,
        "Some(\"gate-probe-arm\") => 1",
        "a match arm on Some(\"..\") was read as a decision",
    );
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

// ── C9 round 6, EQGATE2 (amendment 87): guards expressed as a permission mode,
// a prctl flag, a privilege drop, a limit, a signal disposition or a pre_exec
// hook. Round 6 weakened four of them one at a time (the completion secret's
// 0o400, the trial cache's 0o700, PR_SET_NO_NEW_PRIVS, PR_SET_PDEATHSIG) with
// every suite green: none builds an `Err`, so no earlier form saw them. Each
// group below is planted in a production-shaped line the gate must name; a
// getter, a definition and a comment must not be.

const PRIV_PROBES: &str = "pub fn gate_probe_priv(p: &std::path::Path, cmd: &mut std::process::Command, gp_m: u32, gp_u: u32, gp_n: usize, gp_g: *const libc::gid_t, gp_name: *const libc::c_char) {\n    use std::os::unix::fs::OpenOptionsExt;\n    let gp_mode = std::fs::OpenOptions::new().write(true).mode(gp_m).open(p);\n    let gp_perm = std::fs::set_permissions(p, std::fs::Permissions::from_mode(gp_m));\n    let gp_setmode = set_mode(p, gp_m);\n    let gp_chmod = unsafe { libc::fchmod(3, gp_m) };\n    let gp_chown = unsafe { libc::fchown(3, gp_u, gp_u) };\n    let gp_mkdirat = unsafe { libc::mkdirat(3, gp_name, gp_m) };\n    let gp_umask = unsafe { libc::umask(gp_m) };\n    let gp_prctl = unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0) };\n    let gp_rlimit = unsafe { libc::setrlimit(libc::RLIMIT_CORE, std::ptr::null()) };\n    let gp_setsid = unsafe { libc::setsid() };\n    let gp_pgroup = cmd.process_group(0);\n    let gp_setuid = unsafe { libc::setuid(gp_u) };\n    let gp_setgroups = unsafe { libc::setgroups(gp_n, gp_g) };\n    let gp_signal = unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) };\n    let gp_kill = unsafe { libc::kill(1, libc::SIGKILL) };\n    let gp_fcntl = unsafe { libc::fcntl(3, libc::F_SETFD, 0) };\n    let gp_preexec = unsafe { cmd.pre_exec(|| Ok(())) };\n    let gp_mount = unsafe { libc::mount(std::ptr::null(), c\"/\".as_ptr(), std::ptr::null(), 0, std::ptr::null()) };\n    let gp_caps = unsafe { libc::capset(std::ptr::null_mut(), std::ptr::null()) };\n    let _ = (gp_mode, gp_perm, gp_setmode, gp_chmod, gp_chown, gp_mkdirat, gp_umask, gp_prctl, gp_rlimit, gp_setsid, gp_pgroup, gp_setuid, gp_setgroups, gp_signal, gp_kill, gp_fcntl, gp_preexec, gp_mount, gp_caps);\n}\n\npub fn gate_probe_reads(m: &std::fs::Metadata) -> bool {\n    use std::os::unix::fs::PermissionsExt;\n    let gp_getter = m.permissions().mode() & 0o022 != 0;\n    gp_getter\n}\n\nfn set_mode(_p: &std::path::Path, _m: u32) {}\n";

/// Amendment 87: every use of a permission, privilege, limit, signal or
/// pre-exec call in production code is a site, by group (each group one row).
/// Control: a mode READ (`.mode()` with no argument), a definition of
/// `set_mode` and the unedited tree do not name one.
#[test]
fn a_permission_privilege_or_process_form_is_a_site() {
    let r = tree("priv-forms");
    add_code(&r, SCANNED, PRIV_PROBES);
    for (site, group, form) in [
        ("gp_mode", "a permission mode", ".mode(0o666)"),
        (
            "gp_perm",
            "a set_permissions or set_mode call",
            "set_permissions / from_mode",
        ),
        (
            "gp_setmode",
            "a set_permissions or set_mode call",
            "set_mode",
        ),
        ("gp_chmod", "a chmod or chown", "fchmod"),
        ("gp_chown", "a chmod or chown", "fchown"),
        ("gp_mkdirat", "a mkdirat or umask", "mkdirat"),
        ("gp_umask", "a mkdirat or umask", "umask"),
        ("gp_prctl", "a prctl", "prctl"),
        ("gp_rlimit", "a resource limit", "setrlimit"),
        ("gp_setsid", "a session or process group", "setsid"),
        ("gp_pgroup", "a session or process group", "process_group"),
        ("gp_setuid", "a privilege drop", "setuid"),
        ("gp_setgroups", "a privilege drop", "setgroups"),
        ("gp_signal", "a signal disposition", "signal"),
        ("gp_kill", "a signal disposition", "kill"),
        ("gp_fcntl", "an fcntl", "fcntl"),
        ("gp_preexec", "a pre_exec hook", "pre_exec"),
        ("gp_mount", "a namespace, mount or capability call", "mount"),
        ("gp_caps", "a namespace, mount or capability call", "capset"),
    ] {
        names(
            &r,
            site,
            &format!(
                "a use of {group} ({form}) with no row and no exemption was not a refusal site"
            ),
        );
    }
    not_named(
        &r,
        "gp_getter",
        "a mode READ (`.permissions().mode()` with no argument) was read as setting a mode",
    );
    not_named(
        &r,
        "fn set_mode(_p",
        "the definition of set_mode was read as a use",
    );
    let c = tree("priv-forms-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 7, EQGATE3 (amendment 91): what a child is BUILT with, what bounds
// a read, and refusals that delegate. Round 7 removed each of these with every
// suite green (the helper's `quiet` Stdio::null, git's GIT_OPTIONAL_LOCKS, the
// runner's `n.min(room)` cap, the signer loader's `bad(..)` refusals).

const BUILD_PROBES: &str = "pub fn gate_probe_build(cmd: &mut std::process::Command, buf: &[u8], n: usize, room: usize, p: &std::path::Path) {\n    let gb_envclear = cmd.env_clear();\n    let gb_env = cmd.env(\"GATE_PROBE_VAR\", \"1\");\n    let gb_cwd = cmd.current_dir(p);\n    let gb_stdio = cmd.stdout(std::process::Stdio::null());\n    let gb_gitc = \"-c\";\n    let gb_gitenv = \"--no-includes\";\n    let gb_oscall = unsafe { libc::chdir(c\"/\".as_ptr()) };\n    let gb_cloexec = libc::O_CLOEXEC;\n    let gb_cap = &buf[..n.min(room)];\n    use std::io::Read as _;\n    let gb_take = std::io::empty().take(MAX_GATE_PROBE as u64);\n    let _ = (gb_envclear, gb_env, gb_cwd, gb_stdio, gb_gitc, gb_gitenv, gb_oscall, gb_cloexec, gb_cap, gb_take);\n}\n\nconst MAX_GATE_PROBE: usize = 7;\n\npub fn gate_probe_piped(cmd: &mut std::process::Command) {\n    let gp_piped = cmd.stdout(std::process::Stdio::piped());\n    let _ = gp_piped;\n}\n";

/// Amendment 91: each group of builder and OS-boundary calls is a site: a
/// Command's environment (env_clear, env), working directory, stdio redirection
/// to null, the git `-c` options and GIT_* variables, a process-state libc call,
/// a close-on-exec flag, and a size cap (`.min(room)`, `.take(MAX_*)`).
/// A `Stdio::piped()` capture is NOT one (its absence breaks the handle the
/// caller takes) and a MAX_* definition is not a use.
#[test]
fn a_child_build_and_a_size_cap_are_sites() {
    let r = tree("build-forms");
    add_code(&r, SCANNED, BUILD_PROBES);
    for (site, form) in [
        ("gb_envclear", "env_clear"),
        ("gb_env", "Command::env"),
        ("gb_cwd", "current_dir"),
        ("gb_stdio", "Stdio::null"),
        ("gb_gitc", "a git -c option"),
        ("gb_gitenv", "a GIT_* variable"),
        ("gb_oscall", "a process-state libc call"),
        ("gb_cloexec", "O_CLOEXEC"),
        ("gb_cap", "`.min(room)`"),
        ("gb_take", "`.take(MAX_*)`"),
    ] {
        names(
            &r,
            site,
            &format!("a use of {form} with no row and no exemption was not a refusal site"),
        );
    }
    not_named(
        &r,
        "gp_piped",
        "a Stdio::piped() capture was read as a redirection that removes a guard",
    );
    not_named(
        &r,
        "const MAX_GATE_PROBE: usize",
        "the definition of a MAX_* constant was read as a use",
    );
    let c = tree("build-forms-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 91: a diverging closure or fn whose body DELEGATES to a refusal
/// (here `refuse`, which exits with a variable code) is a refusal constructor
/// too, transitively to a fixed point: the signer loader's `bad(..)` closure
/// calls `refuse`, so its 13 calls were invisible.
#[test]
fn a_diverging_closure_that_delegates_to_a_refusal_is_a_constructor() {
    let r = tree("diverging");
    add_code(
        &r,
        SCANNED,
        "fn gate_probe_refuse(code: i32) -> ! {\n    std::process::exit(code)\n}\n\npub fn gate_probe_signer(x: u64) -> u64 {\n    let gd_bad = |why: String| -> ! { gate_probe_refuse(4) };\n    if x > 9 {\n        gd_bad(format!(\"gate probe {x}\"));\n    }\n    let gd_chain = |why: String| -> ! { gd_bad(why) };\n    if x > 11 {\n        gd_chain(String::from(\"chained\"));\n    }\n    x\n}\n",
    );
    names(
        &r,
        "gd_bad(format!(\"gate probe {x}\"))",
        "a call of a diverging closure that delegates to a refusal was not a site",
    );
    names(
        &r,
        "gd_chain(String::from(\"chained\"))",
        "a call of a diverging closure that delegates through another was not a site",
    );
    let c = tree("diverging-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 91: an exemption whose reason cites a row must cite one that
/// EXISTS. Three exemptions rested on rows nobody could run (M1088 never
/// allocated, M1597 and M2151 misnumbered or withdrawn); the gate now refuses
/// a citation of a row the registry does not hold. Control: the unedited copy.
#[test]
fn an_exemption_citing_a_row_that_does_not_exist_is_refused() {
    let r = tree("cites-missing-row");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_cites(x: u64) -> Result<(), String> {\n    if x > 3 {\n        return Err(format!(\"cites {x}\"));\n    }\n    Ok(())\n}\n",
    );
    exempt(
        &r,
        SCANNED,
        "        return Err(format!(\"cites {x}\"));",
        "dominated by M9999 (a row that is not in the registry)",
    );
    refuses(
        &r,
        &[],
        "which are not registry rows",
        "an exemption citing a row the registry does not hold was accepted",
    );
    let c = tree("cites-missing-row-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 91: an exemption is STALE when every site whose block holds it is
/// covered by a row: the guard was exempted, a row later took it over, and the
/// exemption now claims a reason nobody maintains (the survey found four). The
/// planted exemption sits inside a site `tasks.rs` already rows (M986).
/// Control: the unedited copy holds.
#[test]
fn an_exemption_inside_a_site_a_row_covers_is_stale() {
    let r = tree("stale-exempt");
    exempt(
        &r,
        SCANNED,
        "        if !self.tasks.windows(2).all(|w| w[0] < w[1]) {",
        "probe: a guard a row already covers",
    );
    refuses(
        &r,
        &[],
        "yet every site it lies in is covered by a row",
        "an exemption inside a site a row covers was accepted as still needed",
    );
    let c = tree("stale-exempt-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 8, EQGATE4 (amendment 95): a decision expressed as a VALUE, an
// atomic directory refusal, a single-line `.ok_or(..)?`, one TERM of a
// compound guard, or a constant a guard reads.

const VALUE_PROBES: &str = "pub fn gv_dirs(p: &std::path::Path) {\n    let gv_create = std::fs::create_dir(p);\n    let gv_createall = std::fs::create_dir_all(p);\n    let gv_builder = std::fs::DirBuilder::new();\n    let _ = (gv_create, gv_createall, gv_builder);\n}\n\npub fn gv_values() {\n    let gv_dropsome = GvBox { drop: Some((1, 2)) };\n    let gv_dropnone = GvBox { drop: None };\n    let gv_uidconst = GV_PROBE_UID + 1;\n    let gv_expected = Pin {\n        expected_manifest_sha256: String::new(),\n    };\n    std::env::set_var(\"GV_SETVAR\", \"1\");\n    std::env::remove_var(\"GV_REMOVEVAR\");\n    let _ = (gv_dropsome, gv_dropnone, gv_uidconst, gv_expected);\n}\n\npub fn gv_not_values(expected_sha256: &str, x: Pin) {\n    // create_dir( and drop: Some( are named only in this comment\n    let gv_plain = x.expected_manifest_sha256.len();\n    let _ = (expected_sha256, gv_plain);\n}\n\npub struct Pin {\n    pub expected_manifest_sha256: String,\n}\n";

/// Amendment 95: a guard expressed as a value or an atomic refusal is a site:
/// `create_dir` / `create_dir_all` / `DirBuilder` (an existing directory is
/// refused by the first and accepted by the second), a `drop:` field and a uid
/// or gid constant (the identity handed to a privilege primitive), an
/// `expected_*sha256` field set in a struct literal (the pin a verifier
/// compares against), and `env::set_var` / `remove_var`. A struct FIELD
/// declaration, a fn PARAMETER of that name and a form named in a comment are not.
#[test]
fn a_value_handed_to_a_privilege_primitive_or_a_directory_creation_is_a_site() {
    let r = tree("value-forms");
    add_code(&r, SCANNED, VALUE_PROBES);
    for (site, form) in [
        ("gv_create = ", "create_dir"),
        ("gv_createall = ", "create_dir_all"),
        ("gv_builder = ", "DirBuilder"),
        ("gv_dropsome", "a `drop: Some(..)` field"),
        ("gv_dropnone", "a `drop: None` field"),
        ("gv_uidconst", "a uid constant"),
        (
            "expected_manifest_sha256: String::new()",
            "an `expected_*sha256` field",
        ),
        ("GV_SETVAR", "env::set_var"),
        ("GV_REMOVEVAR", "env::remove_var"),
    ] {
        names(
            &r,
            site,
            &format!("a use of {form} with no row and no exemption was not a refusal site"),
        );
    }
    not_named(
        &r,
        "gv_plain",
        "a read of an `expected_*sha256` field was read as setting one",
    );
    not_named(
        &r,
        "pub expected_manifest_sha256: String",
        "the declaration of an `expected_*sha256` field was read as a use",
    );
    not_named(
        &r,
        "fn gv_not_values",
        "a fn PARAMETER named `expected_sha256` was read as a pin being set",
    );
    let c = tree("value-forms-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 95: a single-line `.ok_or(..)?` / `.ok_or_else(..)?` is a refusal
/// site (the absent value is an error the function returns); an `ok_or` that is
/// not followed by `?` (the error is kept as a value) and one named in a
/// comment are not.
#[test]
fn a_single_line_ok_or_refusal_is_a_site() {
    let r = tree("ok-or");
    add_code(
        &r,
        SCANNED,
        "pub fn gk_probe(x: Option<u64>, y: Option<u64>) -> Result<u64, String> {\n    let gk_q = x.ok_or(\"gk missing\")?;\n    let gk_qe = y.ok_or_else(|| format!(\"gk missing {}\", 1))?;\n    let gk_kept = x.ok_or(\"gk kept\");\n    // gk_comment: z.ok_or(\"no\")?\n    Ok(gk_q + gk_qe + gk_kept.unwrap_or(0))\n}\n",
    );
    names(
        &r,
        "gk_q = ",
        "a single-line `.ok_or(..)?` was not a refusal site",
    );
    names(
        &r,
        "gk_qe = ",
        "a single-line `.ok_or_else(..)?` was not a refusal site",
    );
    not_named(
        &r,
        "gk_kept",
        "an `ok_or` kept as a value was read as a refusal",
    );
    not_named(
        &r,
        "gk_comment",
        "an `ok_or` named in a comment was read as a refusal",
    );
    let c = tree("ok-or-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 95: `libc::syscall(..)` (ioprio_set, ...) and a write to
/// `oom_score_adj` are privilege-form sites.
#[test]
fn a_raw_syscall_and_an_oom_score_write_are_sites() {
    let r = tree("raw-syscall");
    add_code(
        &r,
        SCANNED,
        "pub fn gs_probe() {\n    let gs_sys = unsafe { libc::syscall(libc::SYS_ioprio_set, 1, 0, 0) };\n    let gs_oom = std::fs::write(\"/proc/self/oom_score_adj\", \"-1000\");\n    let _ = (gs_sys, gs_oom);\n}\n",
    );
    names(
        &r,
        "gs_sys = ",
        "a raw `libc::syscall(..)` was not a refusal site",
    );
    names(
        &r,
        "gs_oom = ",
        "a write of oom_score_adj was not a refusal site",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 95: the exemption audit reads a row id of ANY number of digits
/// (`\bM\d{1,4}\b` could not see M99999, so an exemption citing a five-digit row
/// that does not exist was never checked).
#[test]
fn an_exemption_citing_a_five_digit_row_that_does_not_exist_is_refused() {
    let r = tree("cites-5-digit-row");
    add_code(
        &r,
        SCANNED,
        "pub fn gate_probe_cites5(x: u64) -> Result<(), String> {\n    if x > 3 {\n        return Err(format!(\"cites5 {x}\"));\n    }\n    Ok(())\n}\n",
    );
    exempt(
        &r,
        SCANNED,
        "        return Err(format!(\"cites5 {x}\"));",
        "dominated by M99999 (a five-digit row that is not in the registry)",
    );
    refuses(
        &r,
        &[],
        "which are not registry rows",
        "an exemption citing a five-digit row the registry does not hold was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
}

const TERM_PROBE: &str = "pub fn gt_probe(a: u64, b: u64, c: u64) -> Result<(), String> {\n    if a > 1 || b > 2 || c > 3 {\n        return Err(format!(\"gt probe\"));\n    }\n    Ok(())\n}\n";

/// A row appended to the copy's registry: `(id, file, old, new)`.
fn add_row(r: &Path, id: &str, f: &str, old: &str, new: &str) {
    let p = r.join("scripts/v022_g01_mutations.py");
    let s = std::fs::read_to_string(&p).unwrap();
    std::fs::write(
        &p,
        format!(
            "{s}\nMUTATIONS.append(({id:?}, 'gate probe', {f:?}, {old:?}, {new:?}, 'axon-loop', '--lib', 'probe'))\n"
        ),
    )
    .unwrap();
}

/// Amendment 95: a compound guard is judged PER TERM. A row whose edit reaches
/// only the first term of `a || b || c` does not credit `b` or `c` (the round-8
/// review: custodian.rs's `!m.is_file()` and uid terms, observer_service.rs's
/// `!is_file()`); an edit that makes the WHOLE condition a constant
/// (`false && (a || b || c)`) credits every term, and `false && a || b || c`,
/// which leaves `b || c`, does not; a row per term, or a term exemption stating
/// a fact, credits it.
#[test]
fn a_compound_guard_is_judged_per_term() {
    let old = "a > 1 || b > 2 || c > 3";
    // ATTACK: the row removes only the first term.
    let r = tree("terms-first");
    add_code(&r, SCANNED, TERM_PROBE);
    add_row(&r, "MGT1", SCANNED, old, "false && a > 1 || b > 2 || c > 3");
    let t = text(&gate(&r, &[]));
    if !t.contains("compound guard term with no row and no exemption") {
        panic!("ATTACK: a row on the first term of `a || b || c` credited the whole guard: {t}");
    }
    assert!(
        t.contains("b > 2") && t.contains("c > 3"),
        "the report names the other terms: {t}"
    );
    assert!(!t.contains("term with no row and no exemption (the row on another term of the same `||`/`&&` does not credit it): a > 1"), "{t}");
    let _ = std::fs::remove_dir_all(&r);
    // CONTROL 1: the row makes the whole condition a constant.
    let r = tree("terms-const");
    add_code(&r, SCANNED, TERM_PROBE);
    add_row(
        &r,
        "MGT1",
        SCANNED,
        old,
        "false && (a > 1 || b > 2 || c > 3)",
    );
    holds(
        &r,
        &[],
        "an edit that makes the whole condition constant credits every term",
    );
    let _ = std::fs::remove_dir_all(&r);
    // CONTROL 2: one row per term.
    let r = tree("terms-each");
    add_code(&r, SCANNED, TERM_PROBE);
    add_row(&r, "MGT1", SCANNED, old, "false && a > 1 || b > 2 || c > 3");
    add_row(&r, "MGT2", SCANNED, old, "a > 1 || false && b > 2 || c > 3");
    add_row(&r, "MGT3", SCANNED, old, "a > 1 || b > 2 || false && c > 3");
    holds(&r, &[], "a row per term credits every term");
    let _ = std::fs::remove_dir_all(&r);
    // CONTROL 3: a term exemption for each uncredited term; a stale one is refused.
    let r = tree("terms-exempt");
    add_code(&r, SCANNED, TERM_PROBE);
    add_row(&r, "MGT1", SCANNED, old, "false && a > 1 || b > 2 || c > 3");
    for t in ["b > 2", "c > 3"] {
        edit(
            &r,
            GATE,
            "\n\ndef load_rows():",
            &format!(
                "\nTERM_EXEMPT.append(({SCANNED:?}, {t:?}, 'probe: a fact'))\n\n\ndef load_rows():"
            ),
        );
    }
    holds(&r, &[], "a term exemption credits its term");
    edit(
        &r,
        GATE,
        "\n\ndef load_rows():",
        &format!("\nTERM_EXEMPT.append(({SCANNED:?}, 'a > 1', 'probe: a credited term'))\n\n\ndef load_rows():"),
    );
    refuses(
        &r,
        &[],
        "term exemption matches no uncredited guard term",
        "a term exemption for a term a row credits was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
}

const CONST_PROBE: &str = "const GC_MAX_AGE: u64 = 300;\nconst GC_MESSAGE: &str = \"gc refused\";\nconst GC_UNUSED: u64 = 9;\n\npub fn gc_probe(x: u64) -> Result<(), String> {\n    let limit = GC_MAX_AGE;\n    if x > limit {\n        return Err(format!(\"gc {x} {GC_MESSAGE}\"));\n    }\n    Ok(())\n}\n\npub fn gc_other(x: u64) -> u64 {\n    x + GC_UNUSED\n}\n";

/// Amendment 95: a `const` whose name a refusing function reads (outside the
/// refusal's own message) is a site: its definition must be changed by a row or
/// carry an exemption. A const used only in a message, and one no refusing
/// function reads, are not.
#[test]
fn a_constant_a_guard_reads_is_a_site() {
    let r = tree("consts");
    add_code(&r, SCANNED, CONST_PROBE);
    exempt(
        &r,
        SCANNED,
        "        return Err(format!(\"gc {x} {GC_MESSAGE}\"));",
        "probe: the refusal itself is exempt; only the constant is judged",
    );
    names(
        &r,
        "const GC_MAX_AGE",
        "a constant read by a refusing function was not a site",
    );
    not_named(
        &r,
        "const GC_MESSAGE",
        "a constant used only in a refusal's message was a site",
    );
    not_named(
        &r,
        "const GC_UNUSED",
        "a constant no refusing function reads was a site",
    );
    // CONTROL: an exemption on the definition credits it.
    exempt(&r, SCANNED, "const GC_MAX_AGE: u64 = 300;", "probe: a fact");
    holds(&r, &[], "an exempted constant");
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 95: a FORM whose decision is its own line (a directory creation,
/// a privilege call, a build or cap, a value) is credited only by a row whose
/// edit changes THAT line. The `create_dir` inside the block of a uid check, the
/// `.take(MAX_*)` in the block of an owner test were credited by the row on the
/// check above them (the round-8 review found `create_dir_all` surviving at a
/// site whose block held another row). Control: a row that changes the form line
/// credits it.
#[test]
fn a_form_is_credited_only_by_a_row_that_changes_its_own_line() {
    let probe = "pub fn gf_probe(a: u64, p: &std::path::Path) -> Result<(), String> {\n    if a > 1 {\n        let _gf_dir = std::fs::create_dir(p);\n        return Err(format!(\"gf probe\"));\n    }\n    Ok(())\n}\n";
    let r = tree("own-line");
    add_code(&r, SCANNED, probe);
    // A row on the guard above credits the `return Err` site, not the directory.
    add_row(
        &r,
        "MGF1",
        SCANNED,
        "    if a > 1 {\n",
        "    if false && a > 1 {\n",
    );
    names(
        &r,
        "_gf_dir",
        "a row on the guard above credited a form line it never changed",
    );
    let _ = std::fs::remove_dir_all(&r);
    let r = tree("own-line-control");
    add_code(&r, SCANNED, probe);
    add_row(
        &r,
        "MGF1",
        SCANNED,
        "    if a > 1 {\n",
        "    if false && a > 1 {\n",
    );
    add_row(
        &r,
        "MGF2",
        SCANNED,
        "        let _gf_dir = std::fs::create_dir(p);\n",
        "        let _gf_dir = std::fs::create_dir_all(p);\n",
    );
    holds(&r, &[], "a row that changes the form line credits it");
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 9, eqgate5 (amendment 98) ──────────────────────────────────────

/// Amendment 98: a struct-literal FIELD whose value is an ABSOLUTE path literal
/// (`secret: PathBuf::from("/in/job/..")`) is a decision about where a trusted
/// component looks, and a site (the runner's guest_config left five of them
/// unobserved). A relative literal, a computed path, a `let` binding and a
/// field DECLARATION are not.
#[test]
fn an_absolute_path_literal_handed_to_a_config_field_is_a_site() {
    let r = tree("path-field");
    add_code(
        &r,
        SCANNED,
        "pub fn gp_cfg(v: &str) -> GpBox {\n    let gp_local = std::path::PathBuf::from(\"/gp/local\");\n    let _ = gp_local;\n    GpBox {\n        gp_secret: PathBuf::from(\"/in/job/gp-secret\"),\n        gp_new: Path::new(\"/gp/new\"),\n        gp_rel: PathBuf::from(\"rel/gp\"),\n        gp_dyn: PathBuf::from(v),\n    }\n}\n\npub struct GpBox {\n    pub gp_decl: PathBuf,\n}\n",
    );
    for (site, form) in [
        ("gp_secret: PathBuf::from", "a PathBuf::from(\"/..\") field"),
        ("gp_new: Path::new", "a Path::new(\"/..\") field"),
    ] {
        names(
            &r,
            site,
            &format!("a path literal handed to a config field ({form}) was not a refusal site"),
        );
    }
    for (site, what) in [
        ("gp_rel", "a relative path literal"),
        ("gp_dyn", "a computed path"),
        ("gp_local", "a let binding"),
        ("gp_decl", "a field declaration"),
    ] {
        not_named(
            &r,
            site,
            &format!("{what} was read as an absolute path field"),
        );
    }
    let c = tree("path-field-control");
    holds(&c, &[], "the unedited copy");
    let _ = std::fs::remove_dir_all(&c);
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 98: an exemption whose reason says REMAINDER is a guard no test
/// observes alone. The gate COUNTS those by category, prints the counts on
/// every run, lists each with `--remainder`, and never words the result as
/// coverage. A checkable exemption is not one.
#[test]
fn a_remainder_exemption_is_counted_and_never_claimed_covered() {
    let base = tree("remainder-base");
    let b = text(&gate(&base, &["--remainder"]));
    let count = |t: &str| -> usize {
        let l = t
            .lines()
            .find(|l| l.starts_with("REMAINDER: "))
            .unwrap_or_else(|| panic!("ATTACK: the gate printed no REMAINDER count: {t}"));
        l["REMAINDER: ".len()..]
            .split_whitespace()
            .next()
            .unwrap()
            .parse()
            .unwrap()
    };
    let n0 = count(&b);
    let listed0 = b.lines().filter(|l| l.starts_with("REMAINDER ")).count();
    assert_eq!(
        n0, listed0,
        "ATTACK: the REMAINDER count and the REMAINDER list disagree: {b}"
    );
    assert!(
        b.contains("NOT claimed covered")
            && !b.contains("every refusal site in every in-scope file has a row or"),
        "ATTACK: the gate's last line claimed REMAINDER covered: {b}"
    );
    let _ = std::fs::remove_dir_all(&base);
    let r = tree("remainder");
    add_code(&r, SCANNED, "pub fn gq_probe(x: u64) -> Result<(), String> {\n    if x > 7 {\n        return Err(format!(\"gq rem {x}\"));\n    }\n    if x > 9 {\n        return Err(format!(\"gq chk {x}\"));\n    }\n    Ok(())\n}\n");
    exempt(
        &r,
        SCANNED,
        "        return Err(format!(\"gq rem {x}\"));",
        "REMAINDER (no row yet): a probe guard no test observes",
    );
    exempt(
        &r,
        SCANNED,
        "        return Err(format!(\"gq chk {x}\"));",
        "OBSERVED-NOT-ROWED (probe): a test fails without it",
    );
    let t = text(&gate(&r, &["--remainder"]));
    let observed = |t: &str| -> usize {
        t.lines()
            .find(|l| l.starts_with("OBSERVED-NOT-ROWED: "))
            .and_then(|l| {
                l["OBSERVED-NOT-ROWED: ".len()..]
                    .split_whitespace()
                    .next()?
                    .parse()
                    .ok()
            })
            .unwrap_or_else(|| panic!("no OBSERVED-NOT-ROWED line: {t}"))
    };
    assert_eq!(
        observed(&t),
        observed(&b) + 1,
        "ATTACK: a Rust OBSERVED-NOT-ROWED exemption was not counted apart from REMAINDER: {t}"
    );
    assert_eq!(
        count(&t),
        n0 + 1,
        "ATTACK: an exempt REMAINDER guard was not counted (and a checkable one was, or neither): {t}"
    );
    assert!(
        t.lines().any(|l| l.starts_with("REMAINDER ")
            && l.contains("tasks.rs")
            && l.ends_with(" other")),
        "ATTACK: the REMAINDER list does not name the probe site: {t}"
    );
    let _ = std::fs::remove_dir_all(&r);
}

const PYG: &str = "scripts/guest_build_env.py";

/// A Python probe: two sites (a guarded `fail(..)`, an unconditional problem
/// string) and four non-sites (a bare re-raise, `return ""`, `sys.exit` of a
/// callee's status, a `raise` named in a string).
const PY_PROBE: &str = "def gp_probe(x, p):\n    if x > 1:\n        fail(\"gp_marker_fail\")\n    if x == 0:\n        raise ValueError(\"gp_marker_raise\")\n    try:\n        p()\n    except OSError:\n        raise\n    sys.exit(p())\n    s = \"fail(gp_not_a_site)\"\n    return \"\"\n\n\ndef gp_problem(x):\n    if x:\n        return f\"gp_marker_problem {x}\"\n    return \"gp_marker_unconditional\"\n";

fn add_py_probe(r: &Path) {
    edit(
        r,
        PYG,
        "\n\nif __name__ == \"__main__\":\n    main()",
        &format!("\n\n{PY_PROBE}\n\nif __name__ == \"__main__\":\n    main()"),
    );
}

/// Give the copy of the gate one more Python exemption.
fn py_exempt(r: &Path, func: &str, n: usize, frag: &str, kind: &str, reason: &str) {
    edit(
        r,
        GATE,
        "PY_EXEMPT = []   # (file, function, n, fragment, kind, reason): the table below\n",
        &format!(
            "PY_EXEMPT = []   # (file, function, n, fragment, kind, reason): the table below\nPY_EXEMPT.append(({PYG:?}, {func:?}, {n}, {frag:?}, {kind:?}, {reason:?}))\n"
        ),
    );
}

/// Amendment 98: a Python refusal (`fail(..)`, `raise <exc>`, a `sys.exit` of a
/// message, a `return` of a non-empty message) is a site judged by its own
/// guard; a bare re-raise, `return ""`, the propagation of a callee's exit
/// status and a word in a string are not. A row that changes a line of the
/// guard credits it; an exemption is keyed by function, ordinal and a fragment
/// of the site, and one whose fragment disagrees is refused.
#[test]
fn a_python_refusal_is_a_site_judged_by_its_own_guard() {
    let r = tree("py-sites");
    add_py_probe(&r);
    for (site, what) in [
        ("gp_marker_fail", "a guarded fail(..)"),
        ("gp_marker_raise", "a raise of an exception"),
        ("gp_marker_problem", "a return of a message under a guard"),
        (
            "gp_marker_unconditional",
            "an unconditional return of a message",
        ),
    ] {
        names(
            &r,
            site,
            &format!(
                "a Python refusal ({what}) with no row and no exemption was not a refusal site"
            ),
        );
    }
    for (site, what) in [
        ("gp_not_a_site", "a word in a string"),
        ("sys.exit(p())", "the propagation of a callee's exit status"),
    ] {
        not_named(
            &r,
            site,
            &format!("{what} was read as a Python refusal site"),
        );
    }
    // CONTROL: a row on the guard line credits its site.
    add_row(
        &r,
        "MGP1",
        PYG,
        "    if x > 1:\n        fail(\"gp_marker_fail\")",
        "    if False and x > 1:\n        fail(\"gp_marker_fail\")",
    );
    not_named(
        &r,
        "gp_marker_fail",
        "a row that changes the guard did not credit a Python site",
    );
    // An exemption keyed by function, ordinal and fragment credits the others;
    // one whose fragment is not in the site's text is refused.
    py_exempt(
        &r,
        "gp_probe",
        2,
        "raise ValueError(",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        1,
        "return f\"gp_marker_problem",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        2,
        "return \"gp_marker_unconditional\"",
        "OBSERVED",
        "probe: observed",
    );
    holds(&r, &[], "every Python probe site rowed or exempt");
    let _ = std::fs::remove_dir_all(&r);
    let r = tree("py-sites-frag");
    add_py_probe(&r);
    py_exempt(
        &r,
        "gp_probe",
        1,
        "fail(\"not the fragment\")",
        "OBSERVED",
        "probe: observed",
    );
    // Every OTHER site is exempt, so the fragment is the only thing wrong.
    py_exempt(
        &r,
        "gp_probe",
        2,
        "raise ValueError(",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        1,
        "return f\"gp_marker_problem",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        2,
        "return \"gp_marker_unconditional\"",
        "OBSERVED",
        "probe: observed",
    );
    refuses(
        &r,
        &[],
        "names the fragment",
        "an exemption whose fragment is not in its site's text held (a site moved or was added)",
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 98: a Python exemption of kind REMAINDER is COUNTED (category
/// `py_guard`) and one of kind OBSERVED is counted apart; neither is a row.
#[test]
fn a_python_guard_exemption_is_counted_by_kind() {
    let r = tree("py-count");
    add_py_probe(&r);
    py_exempt(
        &r,
        "gp_probe",
        1,
        "fail(\"gp_marker_fail\")",
        "REMAINDER",
        "REMAINDER (no test observes it): probe",
    );
    py_exempt(
        &r,
        "gp_probe",
        2,
        "raise ValueError(",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        1,
        "return f\"gp_marker_problem",
        "OBSERVED",
        "probe: observed",
    );
    py_exempt(
        &r,
        "gp_problem",
        2,
        "return \"gp_marker_unconditional\"",
        "OBSERVED",
        "probe: observed",
    );
    let t = text(&gate(&r, &["--remainder"]));
    assert!(
        t.lines()
            .any(|l| l.starts_with("REMAINDER ") && l.contains(PYG) && l.ends_with(" py_guard")),
        "ATTACK: a Python REMAINDER exemption was not listed under py_guard: {t}"
    );
    assert!(
        t.lines().any(|l| l.starts_with("OBSERVED-NOT-ROWED: ")),
        "ATTACK: the gate does not report the guards observed without a row: {t}"
    );
    let n = |t: &str, k: &str| -> usize {
        t.lines()
            .find(|l| l.starts_with(k))
            .and_then(|l| l[k.len()..].split_whitespace().next()?.parse().ok())
            .unwrap_or_else(|| panic!("no {k} line: {t}"))
    };
    let base = tree("py-count-base");
    let b = text(&gate(&base, &["--remainder"]));
    assert_eq!(
        n(&t, "OBSERVED-NOT-ROWED: "),
        n(&b, "OBSERVED-NOT-ROWED: ") + 3,
        "ATTACK: the observed-without-a-row count did not follow the exemptions: {t}"
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&base);
}

// ── C9 round 11, eqgate6 (amendment 103): a VALUE is a site ─────────────────

/// Give the copy of the gate one more value exemption.
fn value_exempt(r: &Path, f: &str, func: &str, n: usize, frag: &str, kind: &str, reason: &str) {
    edit(
        r,
        GATE,
        "\n\ndef load_rows():",
        &format!(
            "\nVALUE_EXEMPT.append(({f:?}, {func:?}, {n}, {frag:?}, {kind:?}, {reason:?}))\n\n\ndef load_rows():"
        ),
    );
}

/// The gate names the value `frag` of kind `label` as a site with no row and no exemption.
fn names_value(r: &Path, label: &str, frag: &str, attack: &str) {
    let o = gate(r, &[]);
    let t = text(&o);
    let want = format!("refusal site with no row and no exemption: value ({label}) {frag}  [fn");
    if !t.lines().any(|l| l.contains(&want)) {
        panic!("ATTACK: {attack}: the gate did not name `{want}`: {t}");
    }
    assert!(!o.status.success(), "{attack}: named but held: {t}");
}

fn not_names_value(r: &Path, frag: &str, attack: &str) {
    let t = text(&gate(r, &[]));
    let want = format!(" {frag}  [fn");
    if t.lines().any(|l| {
        l.contains("refusal site with no row and no exemption: value (") && l.contains(&want)
    }) {
        panic!("ATTACK: {attack}: the gate named {frag} as a value site: {t}");
    }
}

const SPAWN_PROBE: &str = "pub fn vs_spawn(vs_x: &str) {\n    let _vs_c = std::process::Command::new(\"vs_tool\")\n        .env(\"VS_KEY_A\", \"vs_val_a\")\n        .env(\"VS_KEY_B\", \"vs_val_b\")\n        .arg(\"vs_flag\")\n        .args([\"vs_arg1\", \"vs_arg2\"])\n        .env(\"VS_DYN\", vs_x)\n        .arg(vs_x)\n        .current_dir(\"/vs/cwd\")\n        .stdin(std::process::Stdio::piped());\n}\n\n#[cfg(test)]\nfn vs_test_only() {\n    let _vt = std::process::Command::new(\"x\").env(\"VT_K\", \"vt_val\").arg(\"vt_arg\");\n}\n";

/// SPAWN_PROBE builds a `Command`: amendment 107's table of constructors must list it (a builder
/// whose own `.env/.arg` are the sites), or the gate refuses for the unlisted constructor.
fn add_spawn_probe(r: &Path) {
    add_code(r, SCANNED, SPAWN_PROBE);
    edit(
        r,
        GATE,
        "\nEXEC_CONSTRUCTORS = {\n",
        &format!(
            "\nEXEC_CONSTRUCTORS = {{\n    ({SCANNED:?}, \"vs_spawn\"): \"builder: probe\",\n"
        ),
    );
}

/// Amendment 103 (a): a LITERAL or CONSTANT handed to a process-spawn builder is a
/// site of its own: the value of `.env(K, V)`, `.arg(V)`, each literal element of
/// `.args([..])`, `.current_dir(V)`, a `Stdio::..` handed to a stream. The key is not
/// the value (a row that renames the key credited the whole `.env(` line in round 10,
/// while `"AXON_PATH_EXCLUSIVE", "1"` -> `"0"` kept the suite green). A computed value
/// and a `#[cfg(test)]` item are not sites.
#[test]
fn a_literal_handed_to_a_spawn_builder_is_a_site_of_its_own() {
    let r = tree("value-spawn");
    add_spawn_probe(&r);
    for (label, frag) in [
        ("val_env", "\"vs_val_a\""),
        ("val_env", "\"vs_val_b\""),
        ("val_arg", "\"vs_flag\""),
        ("val_arg", "\"vs_arg1\""),
        ("val_arg", "\"vs_arg2\""),
        ("val_cwd", "\"/vs/cwd\""),
        ("val_stdio", "std::process::Stdio::piped()"),
    ] {
        names_value(
            &r,
            label,
            frag,
            &format!("{frag} handed to a spawn builder was not a value site"),
        );
    }
    for (frag, what) in [
        ("vs_x", "a computed value"),
        ("\"vt_val\"", "a value in a cfg(test) item"),
        ("\"vt_arg\"", "an arg in a cfg(test) item"),
        ("\"VS_KEY_A\"", "an env KEY"),
    ] {
        not_names_value(&r, frag, &format!("{what} was read as a literal value"));
    }
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 103: a row credits a value only when its edit CHANGES that value's
/// text. The key rename does not credit the value, and the edit of one value does
/// not credit its neighbour; a value exemption keyed by (function, n, fragment)
/// credits it as REMAINDER (counted by `val_*` category, never claimed covered) and a
/// stale one (the fragment moved, or a row now changes the value) is refused.
#[test]
fn a_value_is_credited_only_by_an_edit_of_that_value() {
    let r = tree("value-credit");
    add_spawn_probe(&r);
    add_row(
        &r,
        "MVC1",
        SCANNED,
        ".env(\"VS_KEY_A\", \"vs_val_a\")",
        ".env(\"VS_KEY_A2\", \"vs_val_a\")",
    );
    names_value(
        &r,
        "val_env",
        "\"vs_val_a\"",
        "a row that renamed the KEY credited the value",
    );
    add_row(
        &r,
        "MVC2",
        SCANNED,
        ".env(\"VS_KEY_B\", \"vs_val_b\")",
        ".env(\"VS_KEY_B\", \"vs_val_b0\")",
    );
    not_names_value(
        &r,
        "\"vs_val_b\"",
        "a row that changed the value did not credit it",
    );
    names_value(
        &r,
        "val_env",
        "\"vs_val_a\"",
        "the row on the neighbouring value credited this one",
    );
    let _ = std::fs::remove_dir_all(&r);
    // Exemptions: a REMAINDER entry counts by category; a wrong fragment is refused.
    let r = tree("value-exempt");
    // The REMAINDER count of the UNEDITED tree (the planted sites below have no exemption yet,
    // so the gate would refuse and print no list).
    let clean = tree("value-exempt-base");
    let base = text(&gate(&clean, &["--remainder"]));
    let _ = std::fs::remove_dir_all(&clean);
    add_spawn_probe(&r);
    let n0: usize = base
        .lines()
        .filter(|l| l.starts_with("REMAINDER ") && !l.starts_with("REMAINDER:"))
        .count();
    for (n, frag, label) in [
        (1, "vs_val_a", "val_env"),
        (2, "vs_val_b", "val_env"),
        (3, "vs_flag", "val_arg"),
        (4, "vs_arg1", "val_arg"),
        (5, "vs_arg2", "val_arg"),
        (6, "/vs/cwd", "val_cwd"),
        (7, "Stdio::piped()", "val_stdio"),
    ] {
        let _ = label;
        value_exempt(
            &r,
            SCANNED,
            "vs_spawn",
            n,
            frag,
            "REMAINDER",
            "REMAINDER (probe)",
        );
    }
    // The probe's `.env(..)` and `.current_dir(..)` lines are LINE sites of their own (amendment 91's
    // child-build form); a value exemption credits only the value.
    for anchor in [
        ".env(\"VS_KEY_A\", \"vs_val_a\")",
        ".env(\"VS_KEY_B\", \"vs_val_b\")",
        ".env(\"VS_DYN\", vs_x)",
        ".current_dir(\"/vs/cwd\")",
    ] {
        exempt(
            &r,
            SCANNED,
            anchor,
            "DOMINATED (probe): a line site, not a value",
        );
    }
    let t = text(&gate(&r, &["--remainder"]));
    assert!(
        !t.contains("refusal site with no row and no exemption: value ("),
        "ATTACK: a value site with an exemption was still named: {t}"
    );
    assert_eq!(
        t.lines()
            .filter(|l| l.starts_with("REMAINDER ") && !l.starts_with("REMAINDER:"))
            .count(),
        n0 + 7,
        "ATTACK: value exemptions of kind REMAINDER were not counted: {t}"
    );
    assert!(
        t.lines()
            .any(|l| l.starts_with("REMAINDER ") && l.ends_with(" val_env"))
            && t.lines()
                .any(|l| l.starts_with("REMAINDER ") && l.ends_with(" val_stdio")),
        "ATTACK: the REMAINDER list has no val_* category: {t}"
    );
    let _ = std::fs::remove_dir_all(&r);
    let r = tree("value-exempt-stale");
    add_spawn_probe(&r);
    for (n, frag) in [
        (1, "vs_val_a"),
        (2, "vs_val_b"),
        (3, "vs_flag"),
        (4, "vs_arg1"),
        (5, "vs_arg2"),
        (6, "/vs/cwd"),
        (7, "Stdio::piped()"),
    ] {
        let wrong = if n == 2 { "vs_val_x" } else { frag };
        value_exempt(
            &r,
            SCANNED,
            "vs_spawn",
            n,
            wrong,
            "REMAINDER",
            "REMAINDER (probe)",
        );
    }
    refuses(
        &r,
        &[],
        "names the fragment 'vs_val_x', which is not the value's text",
        "a value exemption naming a fragment that is not the site's text was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
}

const PRIV_PROBE: &str = "pub fn vp_priv(h: &VpHost, fd: i32, p: &std::path::Path) {\n    let _vp_o = vp_open(&h.vp_helper, Some(h.vp_owner));\n    let _vp_pat = match vp_x() { Some(vp_uid) => 1, None => 0 };\n    unsafe { libc::fchown(fd, 4242, u32::MAX); }\n    unsafe { libc::setuid(7); }\n    let _ = std::fs::DirBuilder::new().mode(0o731).create(p);\n    unsafe { libc::mkdirat(fd, std::ptr::null(), 0o705) };\n    let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o640));\n    let _vp_m = vp_stat(p) & 0o7777;\n    unsafe { libc::fchmod(fd, VP_MODE); }\n    let _vp_msg = format!(\"mode {:o}\", vp_stat(p) & 0o6666);\n}\n";

/// Amendment 103 (b): an OWNER argument (`Some(<x>.owner|uid|gid)`), a uid/gid handed
/// to a privilege primitive and a permission MODE literal are sites of their own, so
/// `Some(h.owner)` -> `None` (round 10, executed: the whole axon-fabric suite stayed
/// green) and `0o700` -> `0o777` cannot hide behind a row on the primitive's line. A
/// pattern (`Some(uid) =>`) is not an argument.
#[test]
fn an_owner_argument_a_privilege_argument_and_a_mode_literal_are_sites() {
    let r = tree("value-priv");
    add_code(&r, SCANNED, PRIV_PROBE);
    for (label, frag) in [
        ("val_owner", "Some(h.vp_owner)"),
        ("val_priv", "4242"),
        ("val_priv", "u32::MAX"),
        ("val_priv", "7"),
        ("val_mode", "0o731"),
        ("val_mode", "0o705"),
        ("val_mode", "0o640"),
        ("val_mode", "0o7777"),
        ("val_mode", "VP_MODE"),
    ] {
        names_value(
            &r,
            label,
            frag,
            &format!("{frag} ({label}) was not a value site"),
        );
    }
    not_names_value(
        &r,
        "Some(vp_uid)",
        "a `Some(uid) =>` pattern was read as an owner argument",
    );
    not_names_value(
        &r,
        "0o6666",
        "a mode inside a message macro was read as a decision",
    );
    let _ = std::fs::remove_dir_all(&r);
}

const FIELD_PROBE: &str = "pub fn vc_cfg(x: u64) -> VcConfig {\n    VcConfig {\n        vc_limit: 77,\n        vc_name: \"vc_lit\".to_string(),\n        vc_flag: true,\n        vc_dyn: vc_compute(3),\n        vc_var: x,\n    }\n}\n\npub fn vc_other() -> VcOther {\n    VcOther { vo_limit: 55 }\n}\n\npub struct VcConfig {\n    pub vc_limit: u64,\n}\n\nimpl VcConfig {\n    pub fn vc_f(&self) -> u64 { self.vc_limit }\n}\n";

/// Amendment 103 (c): a struct-literal FIELD of a Config/Cfg/Authority/Policy/
/// Manifest/Trust type whose value is a literal or a constant is a site; a field of
/// another type, a computed value, a variable, a declaration and an `impl` head are
/// not.
#[test]
fn a_literal_field_of_a_config_or_policy_struct_is_a_site() {
    let r = tree("value-field");
    add_code(&r, SCANNED, FIELD_PROBE);
    for frag in ["77", "\"vc_lit\".to_string()", "true"] {
        names_value(
            &r,
            "val_field",
            frag,
            &format!("{frag} in a VcConfig literal was not a value site"),
        );
    }
    for (frag, what) in [
        ("55", "a field of a type that is not a Config/Policy type"),
        ("vc_compute(3)", "a computed field value"),
        ("x", "a variable"),
    ] {
        not_names_value(&r, frag, &format!("{what} was read as a literal field"));
    }
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 11, eqgate7 (amendment 107): values FOLLOWED to their sinks ────────

const FLOW_PROBE: &str = "pub fn fw_run(fw_x: &str, fw_p: &FwProg, fw_cfg: &FwCfg) {\n    let mut fw_args: Vec<std::ffi::OsString> = vec![\"--fw-flag\".into(), fw_x.into()];\n    fw_args.push(\"--fw-pushed\".into());\n    let fw_env = [(\"FW_KEY\", FW_PATH_CONST)];\n    let _ = sealed_exec::command(fw_p, None, &fw_args, &fw_env, &[]);\n    let _ = sealed_exec::command(fw_p, None, &[\"--fw-inline\".into()], &[(\"FW_INLINE_KEY\", \"/fw/inline\")], &[]);\n    let _ = fw_open(fw_p, fw_cfg.fw_exec_owner);\n    let _ = fw_open(fw_p, None);\n    let fw_o = Some(7);\n    let _ = fw_open(fw_p, fw_o);\n    let _ = FwRoute { owner: fw_cfg.fw_route_owner };\n    let _ = openat(3, fw_x, FW_FLAGS);\n}\n\nconst FW_PATH_CONST: &str = \"/fw/bin\";\nconst FW_FLAGS: i32 = libc::O_RDONLY | libc::O_DIRECTORY;\nconst FW_UNUSED: &str = \"/fw/never-used-at-a-sink\";\n\npub fn fw_open(_p: &FwProg, _owner: Option<u32>) -> u8 { 0 }\npub fn openat(_d: i32, _n: &str, _f: i32) -> u8 { 0 }\n";

/// Amendment 107 (a): a literal, a const, a collection of them, and a local bound to one,
/// handed to an EXEC WRAPPER (`sealed_exec::command(program, interpreter, args, env, keep)`)
/// are sites of their own, whatever they were built from: `vec![..]`, an array of tuples, a
/// `let` that was grown by `push`. The wrapper's arguments were the biggest blind form in
/// round 11 (the root helper's PATH, its flag names and the argv of every protected child);
/// the const a sink names is a site at its DEFINITION, and a const nothing hands to a sink is
/// not.
#[test]
fn a_value_followed_to_an_exec_wrapper_or_an_open_flag_is_a_site() {
    let r = tree("flow-wrapper");
    add_code(&r, SCANNED, FLOW_PROBE);
    for (label, frag) in [
        ("flow_argv", "\"--fw-flag\".into()"),
        ("flow_argv", "\"--fw-pushed\".into()"),
        ("flow_argv", "\"--fw-inline\".into()"),
        ("flow_env", "\"FW_KEY\""),
        ("flow_env", "FW_PATH_CONST"),
        ("flow_env", "\"FW_INLINE_KEY\""),
        ("flow_env", "\"/fw/inline\""),
        ("flow_const", "\"/fw/bin\""),
        ("flow_const", "libc::O_RDONLY | libc::O_DIRECTORY"),
    ] {
        names_value(
            &r,
            label,
            frag,
            &format!("{frag} followed to a sink was not a flow site"),
        );
    }
    for (frag, what) in [
        ("fw_x.into()", "a computed argument"),
        ("\"/fw/never-used-at-a-sink\"", "a const no sink names"),
    ] {
        not_names_value(&r, frag, &format!("{what} was read as a flow site"));
    }
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 107 (b): the PARAMETER of an expected-owner primitive (`owner: Option<u32>`) is the
/// site, per call, whatever is handed to it: a field, `None`, a local bound to `Some(..)`; and a
/// struct-literal field NAMED `owner` is a site. Round 11 replaced `lx.exec_owner` by `None` at
/// four consumers with the whole suite green because only `Some(<x>.owner)` was a form.
#[test]
fn an_owner_argument_is_a_site_wherever_it_comes_from() {
    let r = tree("flow-owner");
    add_code(&r, SCANNED, FLOW_PROBE);
    for (label, frag) in [
        ("flow_owner", "fw_cfg.fw_exec_owner"),
        ("flow_owner", "None"),
        ("flow_owner", "Some(7)"),
        ("flow_owner", "fw_o"),
        ("flow_owner_field", "fw_cfg.fw_route_owner"),
    ] {
        names_value(
            &r,
            label,
            frag,
            &format!("{frag} ({label}) was not a flow site"),
        );
    }
    let _ = std::fs::remove_dir_all(&r);
}

/// Amendment 107 (c): every non-test `Command::new` of the scope is in the gate's EXPLICIT table
/// of exec wrappers and builders, checked in both directions, so a NEW fn that builds a Command
/// from its parameters cannot become a blind wrapper: it fails the gate until it is listed.
#[test]
fn a_function_that_builds_a_command_is_listed_as_a_wrapper_or_a_builder() {
    let r = tree("flow-ctor");
    holds(&r, &[], "the unedited tree: every constructor is listed");
    add_code(
        &r,
        SCANNED,
        "pub fn fc_spawn(fc_a: &str) -> std::process::Command {\n    std::process::Command::new(fc_a)\n}\n",
    );
    refuses(
        &r,
        &[],
        "fn fc_spawn builds a Command and is in neither EXEC_WRAPPERS nor EXEC_CONSTRUCTORS",
        "a new fn that builds a Command from its parameters was not reported",
    );
    let r2 = tree("flow-ctor-gone");
    edit(
        &r2,
        GATE,
        "(\"crates/axon-fabric/src/git_data.rs\", \"refuse_config\"): ",
        "(\"crates/axon-fabric/src/git_data.rs\", \"no_such_fn\"): ",
    );
    refuses(
        &r2,
        &[],
        "EXEC_CONSTRUCTORS lists fn no_such_fn, which builds no Command",
        "a table entry for a fn that builds no Command was accepted",
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&r2);
}

/// Amendment 103: the gate names, in its last lines, what it STILL cannot see, and
/// the verdict spec says the same. A claim that lists only what the gate sees reads as
/// "every guard".
#[test]
fn the_gate_prints_what_it_still_cannot_see() {
    let r = tree("still-blind");
    let t = text(&gate(&r, &[]));
    let last: Vec<&str> = t.lines().rev().take(10).collect();
    assert!(
        last.iter().any(|l| l.starts_with("STILL BLIND:"))
            && last
                .iter()
                .any(|l| l.contains("COMPUTATION") && l.contains("VALUE FLOWS NOT FOLLOWED"))
            && last
                .iter()
                .any(|l| l.contains("EXEC_WRAPPERS") && l.contains("EXEC_CONSTRUCTORS"))
            && t.lines().any(|l| l.starts_with("VALUE FLOWS NOT FOLLOWED: ")),
        "ATTACK: the gate's last lines do not list what it cannot see (or the count of the flows it did not follow): {t}"
    );
    let _ = std::fs::remove_dir_all(&r);
}

// ── C9 round 12, eqgate8 (amendment 110): SIGNING INPUTS and DEFAULTS are sites ──────────────────

/// A fn whose message goes to a MAC and whose domain goes to a verifier: the literal context, the const
/// domain and the const's definition are sites; a const nothing signs and a computed buffer are not.
const SIGN_PROBE: &str = "pub fn sg_sign(sg_k: &[u8], sg_d: &[u8]) -> [u8; 32] {\n    let mut sg_msg = b\"sg-context/1\\0\".to_vec();\n    sg_msg.extend_from_slice(sg_d);\n    let _ = verify_document(&sg_sig, SG_DOMAIN, &sg_who, &sg_doc, sg_key);\n    hmac_sha256(sg_k, &sg_msg)\n}\n\nconst SG_DOMAIN: &str = \"sg.domain/1\";\nconst SG_UNUSED: &str = \"sg.never-signed/1\";\n";

#[test]
fn an_input_to_a_signing_or_mac_primitive_is_a_site_of_its_own() {
    let r = tree("sign-sites");
    add_code(&r, SCANNED, SIGN_PROBE);
    for (label, frag) in [
        ("flow_sign", "b\"sg-context/1\\0\".to_vec()"),
        ("flow_sign", "SG_DOMAIN"),
        ("flow_const", "\"sg.domain/1\""),
    ] {
        names_value(
            &r,
            label,
            frag,
            &format!("{frag} handed to a signing primitive was not a site"),
        );
    }
    not_names_value(
        &r,
        "\"sg.never-signed/1\"",
        "a const nothing signs was read as a site",
    );
    not_names_value(
        &r,
        "sg_d",
        "a computed buffer was read as a signing literal",
    );
    // the gate PRINTS them: a reader sees which constants are signing inputs, not only a count
    let t = text(&gate(&r, &[]));
    assert!(
        t.lines().any(|l| l.starts_with(
            "VALUE SITES (amendment 110 INPUTS TO A SIGNING / VERIFICATION / MAC PRIMITIVE)"
        )) && t.lines().any(
            |l| l.starts_with("SIGNING INPUT crates/axon-loop/src/tasks.rs:")
                && l.contains("fn sg_sign")
        ),
        "ATTACK: the gate does not list the signing inputs it found: {t}"
    );
    let _ = std::fs::remove_dir_all(&r);
}

/// A default read as a value in a protected crate: the variant, the literal, the bool, `unwrap_or_default()`,
/// `Default::default()` and `.or(Some(..))` are sites; a default computed at the site and a `#[cfg(test)]`
/// item are not.
const DEFAULT_PROBE: &str = "pub fn df_run(df_mode: Option<Mode>, df_n: Option<u64>, df_c: Option<i64>, df_y: Option<u64>, df_w: &str, df_z: Option<usize>) {\n    let _a = df_mode.unwrap_or(Mode::DfDev);\n    let _b = df_w.parse::<u8>().ok().unwrap_or_default();\n    let _c = df_n.unwrap_or(7701);\n    let _d = df_c.map_or(true, |c| c > 3);\n    let _e = df_y.or(Some(7702));\n    let _f: DfCfg = Default::default();\n    let _g = df_z.unwrap_or(df_w.len());\n}\n\n#[cfg(test)]\nfn df_test_only(df_t: Option<u64>) {\n    let _ = df_t.unwrap_or(7799);\n}\n";

#[test]
fn a_default_read_as_a_value_in_a_protected_crate_is_a_site() {
    let r = tree("default-sites");
    add_code(&r, SCANNED, DEFAULT_PROBE);
    for frag in [
        "Mode::DfDev",
        ".unwrap_or_default()",
        "7701",
        "true",
        "7702",
        "Default::default()",
    ] {
        names_value(
            &r,
            "val_default",
            frag,
            &format!("the default {frag} was not a site"),
        );
    }
    not_names_value(
        &r,
        "df_w.len()",
        "a default computed at the site was read as a literal",
    );
    not_names_value(
        &r,
        "7799",
        "a default inside #[cfg(test)] was read as a site",
    );
    let t = text(&gate(&r, &[]));
    assert!(
        t.lines().any(|l| l
            .starts_with("VALUE SITES (amendment 110 DEFAULTS read as a value, protected crates)")),
        "ATTACK: the gate does not count the defaults it found: {t}"
    );
    // outside the protected crates the form is not applied (axon-os is scanned for refusals, not defaults)
    let r2 = tree("default-sites-out");
    add_code(&r2, "crates/axon-os/src/approval.rs", DEFAULT_PROBE);
    not_names_value(
        &r2,
        "Mode::DfDev",
        "a default outside the protected crates was read as a site",
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&r2);
}

/// The count of arguments the gate could not follow is of DISTINCT arguments: round 12 found 46 where the
/// true number was 23, because two passes over the same files incremented one counter.
#[test]
fn a_computed_sink_argument_is_counted_once() {
    let count = |t: &str| -> usize {
        t.lines()
            .find_map(|l| l.strip_prefix("VALUE FLOWS NOT FOLLOWED: "))
            .and_then(|l| l.split_whitespace().next())
            .and_then(|n| n.parse().ok())
            .expect("the count line")
    };
    let rb = tree("computed-base");
    let base = count(&text(&gate(&rb, &[])));
    let _ = std::fs::remove_dir_all(&rb);
    let r = tree("computed-one");
    add_code(
        &r,
        SCANNED,
        "pub fn cc_run(cc_k: &[u8], cc_d: &[u8]) -> [u8; 32] {\n    hmac_sha256(cc_k, cc_d)\n}\n",
    );
    let one = count(&text(&gate(&r, &[])));
    assert_eq!(
        one,
        base + 1,
        "ATTACK: one computed signing argument moved the count from {base} to {one}"
    );
    let _ = std::fs::remove_dir_all(&r);
}
