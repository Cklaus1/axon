//! FU-2 (O-S1-14): a builtin or native call written DIRECTLY in a predicate
//! the interpreter runs implicitly — an `@[verify]` predicate, a struct's
//! `where`, a named refinement, an inline parameter refinement — must be
//! seen by every consumer of the program-wide capability surface, and they
//! must agree with the `axon caps` reach walker (S1) on what it is.
//!
//! Before the fix the program-wide walkers (`program_capabilities`,
//! `program_builtin_calls`) rooted only fn and impl-method bodies and module
//! `let` values, so such a predicate was invisible to G2 (a pass could add
//! it), to the import edge (E1203: a contained importer took it in) and to
//! the manifest's `effect_union` (`program_effect_rows`), while
//! `reachable_capabilities` already reported it.
//!
//! Each fixture is an UN-type-checked parse (G2 and E1203 see such ASTs, and
//! E1209 would otherwise reject an impure refinement before any of this
//! runs) and USES its predicate: a parameter of the refined type, a struct
//! literal, or a fn carrying the `@[verify]`. `PRED` in a template is the
//! predicate; the G2 corpus member is the same program with `true` there, so
//! the only thing the "pass" adds is the predicate's call.

use axon_core::ast::Program;
use axon_core::capabilities::{
    check_import_capabilities, program_effect_rows, reachable_capabilities,
};
use axon_core::error::{E1203, E1402};
use axon_core::improve::verify_pass;

struct Case {
    name: &'static str,
    /// The program; `PRED` is replaced by the predicate (or by `true`).
    template: &'static str,
    pred: &'static str,
    /// The capability label the predicate exercises.
    label: &'static str,
    /// The effect row it implies (`None`: a native call, whose module effects
    /// `program_effect_rows` does not record anywhere — O-S1-20).
    row: Option<&'static str>,
}

const MAIN: &str = "\nfn main() -> i64 {\n    0\n}\n";

const CASES: &[Case] = &[
    Case {
        name: "verify_read",
        template: "@[verify(PRED)]\nfn f(n: i64) -> i64 {\n    n\n}\n",
        pred: "is_ok(read_file(\"/nonexistent/in\"))",
        label: "fs:read",
        row: Some("IO"),
    },
    Case {
        name: "verify_write",
        template: "@[verify(PRED)]\nfn f(n: i64) -> i64 {\n    n\n}\n",
        pred: "is_ok(write_file(\"/nonexistent/out\", \"x\"))",
        label: "fs:write",
        row: Some("IO"),
    },
    Case {
        name: "struct_where",
        template: "type Pair = { lo: i64, hi: i64 } where PRED\n\
                   fn f(n: i64) -> i64 {\n    let p = Pair { lo: n, hi: n }\n    p.lo\n}\n",
        pred: "is_ok(write_file(\"/nonexistent/out\", \"x\"))",
        label: "fs:write",
        row: Some("IO"),
    },
    Case {
        name: "named_refine",
        template: "type Lk = i64 where PRED\nfn f(y: Lk) -> i64 {\n    y\n}\n",
        pred: "is_ok(read_file(\"/nonexistent/in\"))",
        label: "fs:read",
        row: Some("IO"),
    },
    Case {
        name: "inline_param",
        template: "fn f(n: i64 where PRED) -> i64 {\n    n\n}\n",
        pred: "is_ok(write_file(\"/nonexistent/out\", \"x\"))",
        label: "fs:write",
        row: Some("IO"),
    },
    Case {
        name: "named_refine_exec",
        template: "type Ex = i64 where PRED\nfn f(y: Ex) -> i64 {\n    y\n}\n",
        pred: "is_ok(exec(\"true\"))",
        label: "exec",
        row: Some("Exec"),
    },
    Case {
        name: "verify_native",
        template: "use native::gfx\n@[verify(PRED)]\nfn f(n: i64) -> i64 {\n    n\n}\n",
        pred: "gfx::window_open(1, 1, \"x\") > 0",
        label: "native:gfx",
        row: None,
    },
];

fn program(case: &Case, pred: &str) -> Program {
    let src = format!("{}{MAIN}", case.template.replace("PRED", pred));
    axon_core::parse_source(&src).unwrap_or_else(|e| panic!("{}: parse: {e:?}\n{src}", case.name))
}

/// Run `check` on every fixture it applies to; fail once, listing every case
/// it refused, so a red run shows the whole table.
fn each(applies: fn(&Case) -> bool, check: fn(&Case) -> Result<(), String>) {
    let failures: Vec<String> = CASES
        .iter()
        .filter(|c| applies(c))
        .filter_map(|c| check(c).err())
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn every(_: &Case) -> bool {
    true
}

/// (4) S1's reach walker, from the fn that uses the predicate. Passes before
/// and after FU-2: it is the reference the other three must agree with.
fn reach(c: &Case) -> Result<(), String> {
    let p = program(c, c.pred);
    let reach = reachable_capabilities(&p, Some("f")).expect("f exists");
    let got: Vec<&str> = reach.capabilities.iter().map(String::as_str).collect();
    if got != [c.label] {
        return Err(format!(
            "ATTACK: {}: the reach walker reports {got:?} for a `{}` predicate",
            c.name, c.label
        ));
    }
    Ok(())
}

/// (1) G2: a pass that adds the predicate is rejected (E1402), naming
/// exactly the label the reach walker reports.
fn g2(c: &Case) -> Result<(), String> {
    let pure = program(c, "true");
    let added = program(c, c.pred);
    let rec = verify_pass(&move |_: &Program| added.clone(), &[pure]);
    match &rec.g2_safety {
        Err(e) if e.code == E1402 && e.message.contains(&format!("{{{}}}", c.label)) => Ok(()),
        Err(e) => Err(format!(
            "{}: G2 must name exactly `{}`: {} {}",
            c.name, c.label, e.code, e.message
        )),
        Ok(()) => Err(format!(
            "ATTACK: {}: a pass adding a `{}` predicate passed G2",
            c.name, c.label
        )),
    }
}

/// (2) The import edge: a contained importer granting nothing takes the
/// module in only with one E1203 naming the same label; the pure module
/// passes.
fn import_edge(c: &Case) -> Result<(), String> {
    let importer = axon_core::parse_source(
        "@[contained(fs: [], net: [], exec: none)]\nfn main() -> i64 {\n    0\n}\n",
    )
    .unwrap();
    let pure = check_import_capabilities(&importer, "m", &program(c, "true"));
    if !pure.is_empty() {
        return Err(format!("{}: the pure module widened: {pure:?}", c.name));
    }
    let errs = check_import_capabilities(&importer, "m", &program(c, c.pred));
    match errs.as_slice() {
        [] => Err(format!(
            "ATTACK: {}: a contained importer took in a `{}` predicate without E1203",
            c.name, c.label
        )),
        [e] if e.code == E1203 && e.message.contains(&format!("capability `{}`", c.label)) => {
            Ok(())
        }
        _ => Err(format!(
            "{}: want one E1203 naming `{}`: {errs:?}",
            c.name, c.label
        )),
    }
}

/// (3) The manifest's effect union (`build_axmeta_manifest` copies
/// `program_effect_rows` verbatim) carries the row the predicate implies.
fn effect_row(c: &Case) -> Result<(), String> {
    let row = c.row.expect("a builtin case");
    let pure = program_effect_rows(&program(c, "true"));
    if !pure.is_empty() {
        return Err(format!("{}: the pure program has rows {pure:?}", c.name));
    }
    let rows = program_effect_rows(&program(c, c.pred));
    if !rows.contains(row) {
        return Err(format!(
            "ATTACK: {}: a `{}` predicate is missing `{row}` from the effect union: {rows:?}",
            c.name, c.label
        ));
    }
    Ok(())
}

#[test]
fn the_reach_walker_reports_every_predicate_route() {
    each(every, reach);
}

#[test]
fn g2_rejects_a_pass_that_adds_a_predicate_route() {
    each(every, g2);
}

#[test]
fn the_import_edge_refuses_a_module_whose_predicate_widens_a_contained_importer() {
    each(every, import_edge);
}

/// Builtin cases only: the effect union records no native module effects
/// even from bodies (O-S1-20).
#[test]
fn the_effect_union_carries_the_row_a_predicate_route_implies() {
    each(|c| c.row.is_some(), effect_row);
}
