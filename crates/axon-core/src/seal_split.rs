//! The split of one half of a `--seal` check (C9 round 15, amendment 121).
//!
//! `axon test --seal` checks the merged program for the OPERATOR's diagnostics and the sealed
//! items alone for the CANDIDATE's (`run_check_pipeline_located` in `main.rs`). Which half a
//! diagnostic belongs to is read off the file its span names. A merged diagnostic that names
//! no line cannot be told from a sealed item's by the file (the entry file is the label every
//! spanless diagnostic gets), so it is handed to the BACKSTOP list with the sealed-side ones:
//! shown only when the sealed-only half found no error (so it still refuses), never beside that
//! half's own text. The only spanless diagnostics that are the operator's for certain are the
//! module-load ones (the first `n_load`), which are about the entry file's own `use` lines.

use crate::PipelineDiagnostic;

/// `(kept, handed back)`. `merged_half` is true for the run over the merged program;
/// `masked(file, line)` says the diagnostic belongs to the OTHER half by its file.
pub fn split_for_view(
    diags: Vec<PipelineDiagnostic>,
    n_load: usize,
    merged_half: bool,
    masked: &dyn Fn(&str, u32) -> bool,
) -> (Vec<PipelineDiagnostic>, Vec<PipelineDiagnostic>) {
    let mut kept = Vec::new();
    let mut rest = Vec::new();
    for (i, d) in diags.into_iter().enumerate() {
        let unattributable = merged_half && d.line == 0 && i >= n_load;
        if masked(&d.file, d.line) || unattributable {
            rest.push(d);
        } else {
            kept.push(d);
        }
    }
    (kept, rest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(file: &str, line: u32, msg: &str) -> PipelineDiagnostic {
        PipelineDiagnostic {
            code: "E0001".into(),
            message: msg.into(),
            file: file.into(),
            line,
            col: 0,
            severity: "error".into(),
            caret: String::new(),
            expected: None,
            found: None,
            help: None,
        }
    }

    #[test]
    fn a_spanless_merged_diagnostic_is_not_shown_beside_the_sealed_ones() {
        // The sealed side is the files named `sealed/..`.
        let masked = |f: &str, _l: u32| f.starts_with("sealed/");
        let diags = vec![
            d("suite/main.ax", 0, "load"),
            d("suite/main.ax", 0, "spanless, could be a sealed item's"),
            d("suite/main.ax", 4, "operator's, located"),
            d("sealed/sol.ax", 2, "sealed, located"),
        ];
        let (kept, rest) = split_for_view(diags, 1, true, &masked);
        let kept: Vec<_> = kept.iter().map(|x| x.message.as_str()).collect();
        let rest: Vec<_> = rest.iter().map(|x| x.message.as_str()).collect();
        assert!(
            !kept.contains(&"spanless, could be a sealed item's"),
            "ATTACK: a spanless merged diagnostic was shown as the operator's: {kept:?}"
        );
        assert!(
            kept.contains(&"load"),
            "ATTACK: a module-load diagnostic of the entry file was hidden: {kept:?}"
        );
        assert!(
            kept.contains(&"operator's, located"),
            "an operator-file diagnostic with a line was not kept: {kept:?}"
        );
        assert_eq!(
            rest,
            ["spanless, could be a sealed item's", "sealed, located"],
            "a spanless merged diagnostic was not handed to the backstop"
        );
    }
}
