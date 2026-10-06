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
