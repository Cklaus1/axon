//! The Cortex skill set is DATA, so it is validated like data.
//!
//! A skill that cites evidence which no longer exists is worse than no skill:
//! it reads as grounded while pointing at nothing, which is the same
//! absent-vs-passed collapse the skills themselves are about. So the one thing
//! this test insists on beyond shape is that every cited file EXISTS.
//!
//! What this test does NOT claim: that anything consumes the skills. Nothing
//! does yet. That is stated in the README rather than implied by a passing
//! test.

use serde_json::Value;

fn skills_dir() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("skills")
}

#[test]
fn skills_are_well_formed_and_cite_real_evidence() {
    let dir = skills_dir();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("skills dir unreadable at {}: {e}", dir.display()))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
        .collect();
    files.sort();

    // Non-vacuity floor. Without it a renamed directory or extension makes this
    // test pass having validated nothing — the exact failure the skills warn
    // about, in the test that validates the skills.
    assert!(
        files.len() >= 3,
        "expected the Cortex skill set, found {} json files in {}",
        files.len(),
        dir.display()
    );

    // The repo root, for resolving the file paths skills cite.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repo root");

    let mut ids: Vec<String> = Vec::new();
    let mut evidence_count = 0usize;

    for f in &files {
        let raw = std::fs::read_to_string(f).expect("read skill");
        let v: Value = serde_json::from_str(&raw)
            .unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", f.display()));

        let name = f.file_stem().unwrap().to_str().unwrap();
        let get = |k: &str| -> &Value {
            v.get(k)
                .unwrap_or_else(|| panic!("{} is missing required field `{k}`", f.display()))
        };

        let id = get("id").as_str().expect("id is a string").to_string();
        assert_eq!(id, name, "{}: `id` must match the filename", f.display());
        ids.push(id);

        for k in ["trigger", "priority", "transfers", "limits"] {
            let s = get(k)
                .as_str()
                .unwrap_or_else(|| panic!("{name}: `{k}` must be a string"));
            // Empty is not a value. A skill with an empty `limits` is a skill
            // that claims no limits, which is never true.
            assert!(!s.trim().is_empty(), "{name}: `{k}` is empty");
        }

        let probe = get("probe").as_array().expect("probe is an array");
        assert!(
            probe.len() >= 2,
            "{name}: a probe of fewer than 2 steps is an assertion, not a procedure"
        );

        let evidence = get("evidence").as_array().expect("evidence is an array");
        assert!(
            !evidence.is_empty(),
            "{name}: a skill with no evidence is a guess"
        );
        for e in evidence {
            let file = e
                .get("file")
                .and_then(|x| x.as_str())
                .unwrap_or_else(|| panic!("{name}: an evidence entry has no `file`"));
            let finding = e
                .get("finding")
                .and_then(|x| x.as_str())
                .unwrap_or_else(|| panic!("{name}: evidence for {file} has no `finding`"));
            assert!(
                !finding.trim().is_empty(),
                "{name}: empty finding for {file}"
            );
            let p = root.join(file);
            assert!(
                p.exists(),
                "{name}: evidence cites {file}, which does not exist. A skill \
                 pointing at a file that is gone reads as grounded while \
                 pointing at nothing — delete the entry or repoint it."
            );
            evidence_count += 1;
        }
    }

    ids.sort();
    let before = ids.len();
    ids.dedup();
    assert_eq!(before, ids.len(), "duplicate skill ids: {ids:?}");

    // Second non-vacuity floor, on the thing actually checked: file existence.
    // Files could be well-formed while citing nothing.
    assert!(
        evidence_count >= 6,
        "only {evidence_count} evidence entries across {} skills — the \
         file-existence check is the substance of this test",
        files.len()
    );
}
