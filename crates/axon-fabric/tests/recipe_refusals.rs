//! The workspace recipe's own refusals (axon-workspace-recipe: the ONE walker
//! and path rule every tree digest is computed by, host and guest), each
//! driven through the production store (C9 round 4c, r4c-fixes part 2,
//! amendment 71): `WorkspaceStore::import_dir` (submit's candidate and check
//! imports) and `WorkspaceStore::materialize` (what a launch writes from a
//! stored version). Every attack asserts the store refused AND nothing was
//! published or written; the CONTROL is the same tree without the defect.
use axon_fabric::workspace::{Quota, StoreError, WorkspaceStore};
use axon_loop_contracts::{Acf1Ref, TenantId};
use std::path::{Path, PathBuf};

fn tenant() -> TenantId {
    TenantId::new("tenant-recipe").unwrap()
}

fn files_under(p: &Path) -> usize {
    let Ok(rd) = std::fs::read_dir(p) else {
        return 0;
    };
    rd.map(|e| {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            files_under(&e.path())
        } else {
            1
        }
    })
    .sum()
}

struct Case {
    _d: tempfile::TempDir,
    root: PathBuf,
    state: PathBuf,
}

fn case() -> Case {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("tree");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("ok.txt"), "fine\n").unwrap();
    let state = d.path().join("state");
    Case { root, state, _d: d }
}

/// Import `root` through the store: ATTACK (`what`) if it was admitted;
/// otherwise the refusal must be `class` and nothing may be published.
fn import_refused(c: &Case, root: &Path, class: &str, what: &str) {
    let store = WorkspaceStore::open(&c.state, &tenant()).unwrap();
    match store.import_dir(root, &Quota::default()) {
        Ok(r) => panic!("ATTACK: {what}: the tree was imported as {r}"),
        Err(StoreError::Refused(r)) => assert_eq!(r.class(), class, "setup: {r}"),
        Err(e) => panic!("setup: {what}: refused on something else: {e}"),
    }
    assert_eq!(
        files_under(&c.state.join("tenants")),
        0,
        "{what}: a refused import published something"
    );
}

/// CONTROL: the base tree imports.
fn control_imports(c: &Case) {
    WorkspaceStore::open(&c.state, &tenant())
        .unwrap()
        .import_dir(&c.root, &Quota::default())
        .unwrap_or_else(|e| panic!("control: the base tree imports: {e}"));
}

/// A name with a control character (here a newline) is refused: the manifest
/// is one `<mode> <size> <sha> <path>` LINE per entry, so a newline in a path
/// would let one tree's manifest spell another's.
#[test]
fn a_name_with_a_control_character_is_refused() {
    let c = case();
    std::fs::write(c.root.join("a\nb"), "x").unwrap();
    import_refused(
        &c,
        &c.root,
        "control_character",
        "a name holding a newline was imported into a line-based manifest",
    );
    let c = case();
    control_imports(&c);
}

/// A symlink whose target is absolute leaves the tree wherever the tree is
/// materialized.
#[test]
fn an_absolute_symlink_target_is_refused() {
    let c = case();
    std::os::unix::fs::symlink("/etc/passwd", c.root.join("abs")).unwrap();
    import_refused(
        &c,
        &c.root,
        "escaping_symlink",
        "a symlink to an absolute target was imported",
    );
    let c = case();
    std::os::unix::fs::symlink("ok.txt", c.root.join("inside")).unwrap();
    control_imports(&c);
}

/// A name that is not UTF-8 refuses the whole tree; it is never silently left
/// out of the version.
#[test]
fn a_non_utf8_name_is_refused_not_dropped() {
    use std::os::unix::ffi::OsStrExt;
    let c = case();
    std::fs::write(
        c.root.join(std::ffi::OsStr::from_bytes(b"bad\xffname")),
        "x",
    )
    .unwrap();
    import_refused(
        &c,
        &c.root,
        "non_utf8",
        "a file whose name is not UTF-8 was silently left out of the version",
    );
    let c = case();
    control_imports(&c);
}

/// A FIFO (a special file the recipe cannot represent) refuses the whole
/// tree; it is never silently left out of the version.
#[test]
fn a_special_file_is_refused_not_dropped() {
    let c = case();
    let p = std::ffi::CString::new(c.root.join("fifo").to_str().unwrap()).unwrap();
    // SAFETY: a path we own.
    assert_eq!(
        unsafe { libc::mkfifo(p.as_ptr(), 0o644) },
        0,
        "setup: mkfifo"
    );
    import_refused(
        &c,
        &c.root,
        "special_file",
        "a FIFO was silently left out of the version",
    );
    let c = case();
    control_imports(&c);
}

/// The import root itself is never followed through a symlink: a root that
/// is a link to another directory imports nothing of that directory.
#[test]
fn an_import_root_that_is_a_symlink_is_refused() {
    let c = case();
    let link = c.root.parent().unwrap().join("root-link");
    std::os::unix::fs::symlink(&c.root, &link).unwrap();
    let store = WorkspaceStore::open(&c.state, &tenant()).unwrap();
    match store.import_dir(&link, &Quota::default()) {
        Ok(r) => panic!(
            "ATTACK: an import root that is a symlink was followed: {} imported as {r}",
            link.display()
        ),
        Err(StoreError::Refused(r)) => assert_eq!(r.class(), "io", "setup: {r}"),
        Err(e) => panic!("setup: refused on something else: {e}"),
    }
    control_imports(&c);
}

/// A stored version whose manifest names a path that climbs out of the tree
/// (`../escape`) never materializes: nothing is written outside the
/// destination. The version is well-formed in every other way (its manifest
/// hashes to its reference, its blob holds its bytes, it has an omission
/// record), so the path rule is what refuses it.
#[test]
fn a_stored_version_that_climbs_out_never_materializes() {
    use axon_workspace_recipe::{sha256_hex, workspace_version_ref};
    let d = tempfile::tempdir().unwrap();
    let state = d.path().join("state");
    let store = WorkspaceStore::open(&state, &tenant()).unwrap();
    let ws = {
        use sha2::{Digest, Sha256};
        let key = format!("{:x}", Sha256::digest(tenant().as_str().as_bytes()));
        state.join("tenants").join(&key[..32]).join("workspaces")
    };
    let plant = |path: &str| -> Acf1Ref {
        let body = b"escaped\n";
        let sha = sha256_hex(body);
        std::fs::write(ws.join("blobs").join(&sha), body).unwrap();
        let manifest = format!("100644 {} {sha} {path}\n", body.len());
        let r = workspace_version_ref(manifest.as_bytes());
        let hex = r.strip_prefix("acf1:").unwrap();
        std::fs::write(
            ws.join("versions").join(format!("{hex}.manifest")),
            &manifest,
        )
        .unwrap();
        let om = ws.join("versions").join(format!("{hex}.omissions"));
        std::fs::create_dir_all(&om).unwrap();
        std::fs::write(om.join(format!("{}.json", sha256_hex(b"[]"))), "[]").unwrap();
        Acf1Ref::new(r).unwrap()
    };
    let r = plant("../escape");
    let dest = d.path().join("out").join("dest");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let res = store.materialize(&r, &dest, false);
    let escaped = d.path().join("out").join("escape");
    if res.is_ok() || escaped.exists() {
        panic!(
            "ATTACK: a stored version naming ../escape materialized (wrote {}: {}): {res:?}",
            escaped.display(),
            escaped.exists()
        );
    }
    match res {
        Err(StoreError::Refused(x)) => assert_eq!(x.class(), "traversal", "setup: {x}"),
        other => panic!("setup: refused on something else: {other:?}"),
    }
    // CONTROL: the same planting with an inside path materializes.
    let r = plant("inside.txt");
    let dest2 = d.path().join("out").join("dest2");
    store
        .materialize(&r, &dest2, false)
        .unwrap_or_else(|e| panic!("control: a planted inside version materializes: {e}"));
    assert!(dest2.join("inside.txt").is_file(), "control: the file");
}
