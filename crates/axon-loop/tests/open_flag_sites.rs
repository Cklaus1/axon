//! C9 round 4c, EQGATE (amendment 81; M1917): a store write is a CREATE.
//!
//! `Store::write_atomic` writes `.<name>.tmp.<pid>.<seq>` with
//! `create_new(true)` (O_CREAT|O_EXCL), then renames it into place. The
//! temporary's name is predictable (the pid and a process-wide counter), so
//! a file planted at it (here a hard link to a file outside the write) must be
//! REFUSED, never written through: the refusal is the kernel's, so it builds
//! no `Err` the refusal-coverage gate could see, and the suite stayed green
//! with the flag removed.

use axon_loop::store::Store;

#[test]
fn a_store_write_never_writes_through_a_file_planted_at_its_temporary_name() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::open_dir(t.path().join("store")).unwrap();
    let dir = store.root().join("d");
    store.ensure_dir(&dir).unwrap();
    // CONTROL (the first write of the process, so the counter is where the
    // plants below start): a write with nothing planted succeeds.
    store
        .write_atomic(&dir.join("control"), b"ok\n")
        .expect("control: an unplanted write succeeds");
    let victim = t.path().join("victim");
    std::fs::write(&victim, "precious\n").unwrap();
    // Plant a hard link at every temporary name the next writes could use.
    let pid = std::process::id();
    for seq in 0..64u32 {
        let p = dir.join(format!(".target.tmp.{pid}.{seq}"));
        std::fs::hard_link(&victim, &p).unwrap();
    }
    let got = store.write_atomic(&dir.join("target"), b"attacker-chosen bytes\n");
    assert!(
        got.is_err() && std::fs::read_to_string(&victim).unwrap() == "precious\n",
        "ATTACK: a store write went through a file planted at its temporary name: {got:?}, \
         victim now {:?}",
        std::fs::read_to_string(&victim)
    );
}

/// C9 round 7, EQGATE3 (amendment 91): `Store::guard` is `pub`, and refuses a
/// path that climbs out of the store (`..`) or is not below it. That refusal was
/// exempted as UNREACHABLE (every internal path is built from validated
/// segments); a caller of the `pub` guard reaches it.
#[test]
fn the_store_guard_refuses_a_path_that_leaves_the_store() {
    let t = tempfile::tempdir().unwrap();
    let store = Store::open_dir(t.path().join("store")).unwrap();
    store
        .guard(&store.root().join("a").join("b"))
        .expect("control: a path below the root is fine");
    let climbing = store.root().join("a").join("..").join("..").join("outside");
    let got = store.guard(&climbing);
    assert!(
        got.as_ref()
            .is_err_and(|e| e.to_string().contains("non-normal store path")),
        "ATTACK: the store guard accepted a path that climbs out of the store: {got:?}"
    );
    let got = store.guard(&t.path().join("elsewhere"));
    assert!(
        got.as_ref()
            .is_err_and(|e| e.to_string().contains("outside the store")),
        "ATTACK: the store guard accepted a path outside the store: {got:?}"
    );
}
