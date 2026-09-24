//! D-C3: the ONE `acf1:` canonicaliser the cortex → fabric seam uses, pinned
//! to bytes produced by the reference rule:
//!
//! ```text
//! python3 -c 'import json,hashlib; b=json.dumps(V,sort_keys=True,
//!   separators=(",",":"),ensure_ascii=False).encode(); print(b, hashlib.sha256(b).hexdigest())'
//! ```

use axon_cortex::runner::{
    acf1_canonical_bytes, acf1_digest, fabric_executable_digest, fabric_workspace_digest,
};

#[test]
fn executable_identity_matches_the_reference_bytes() {
    let sha = "ab".repeat(32);
    assert_eq!(
        String::from_utf8(acf1_canonical_bytes(&[
            ("sha256", &sha),
            ("registered_executable_ref", "axon-test-local"),
        ]))
        .unwrap(),
        format!(r#"{{"registered_executable_ref":"axon-test-local","sha256":"{sha}"}}"#),
        "keys sorted here, whatever order the caller passes"
    );
    assert_eq!(
        fabric_executable_digest("axon-test-local", &sha),
        "acf1:7d5a4353bc70157fe3785d96b33b0ececaec3a03a170b68c26ecfd8387246ff0"
    );
}

#[test]
fn workspace_identity_escapes_by_the_cl22_rule() {
    // A path with a quote, a backslash, a C0 control, DEL and non-ASCII.
    let path = "dir/é\"q\\\u{1}\u{7f}😀.ax";
    let bytes = acf1_canonical_bytes(&[
        ("path", path),
        (
            "sha256",
            "277089d91c0bdf4f2e6862ba7e4a07605119431f5d13f726dd352b06f1b206a9",
        ),
    ]);
    assert_eq!(
        bytes,
        "{\"path\":\"dir/é\\\"q\\\\\\u0001\u{7f}😀.ax\",\"sha256\":\
         \"277089d91c0bdf4f2e6862ba7e4a07605119431f5d13f726dd352b06f1b206a9\"}"
            .as_bytes()
    );
    assert_eq!(
        fabric_workspace_digest(path, b"bytes"),
        "acf1:ecf6b874cfc9eaeaeeb66b8ab3ca4e71997dd3b5521ab963b635f7fe8f5765da"
    );
    assert_eq!(
        acf1_digest(&[("sha256", "x"), ("path", "y")]),
        acf1_digest(&[("path", "y"), ("sha256", "x")])
    );
}

#[test]
#[should_panic(expected = "duplicate key")]
fn a_duplicate_key_is_refused() {
    acf1_canonical_bytes(&[("path", "a"), ("path", "b")]);
}
