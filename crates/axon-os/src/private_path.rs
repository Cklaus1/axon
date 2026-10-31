//! Owner-private paths: a store or key file whose owner and mode are part of
//! its meaning.
//!
//! A record store (the custodian's nonce store, the repair-envelope spent
//! store, the run ledger) or a key file is evidence only while nobody but its
//! owner can write it: another owner, or a group the untrusted side is in,
//! could plant records that were never issued, erase spent ones (a spent nonce
//! spends again) or swap the key. Moved here from `axon-fabric::custodian`
//! (NH-R1 §4.2, D13) so axon-os's repair envelope checks its paths by the same
//! rule; axon-fabric re-exports [`check_store`].

use std::path::Path;

/// What a private path is, for its refusals.
pub struct Private {
    /// The noun the refusal names (`nonce store`).
    pub what: &'static str,
    /// Whose it must be, as the refusal words it (`the custodian's`). Text
    /// only: the uid compared is the caller's `euid`.
    pub whose: &'static str,
    /// `true`: a real directory; `false`: a regular file.
    pub dir: bool,
    /// What a foreign holder could do (`write nonce records`).
    pub takeover: &'static str,
    /// What group/other access lets another uid do.
    pub exposure: &'static str,
}

/// The custodian's nonce store (axon-fabric).
pub const NONCE_STORE: Private = Private {
    what: "nonce store",
    whose: "the custodian's",
    dir: true,
    takeover: "write nonce records",
    exposure: "plant or erase nonce records",
};

/// The path must be the caller's OWN: not a symlink, of the expected kind,
/// owned by `euid`, with no group or other access.
pub fn check_private(path: &Path, euid: u32, p: &Private) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let what = p.what;
    let m = std::fs::symlink_metadata(path)
        .map_err(|e| format!("{what} {}: {e} (it must exist)", path.display()))?;
    let kind_ok = if p.dir { m.is_dir() } else { m.is_file() };
    if m.file_type().is_symlink() || !kind_ok {
        return Err(format!(
            "{what} {} is not a {} (a symlink is never followed)",
            path.display(),
            if p.dir { "directory" } else { "regular file" }
        ));
    }
    if m.uid() != euid {
        return Err(format!(
            "{what} {} is owned by uid {}, not {} ({euid}): its owner could {}",
            path.display(),
            m.uid(),
            p.whose,
            p.takeover
        ));
    }
    if m.mode() & 0o077 != 0 {
        return Err(format!(
            "{what} {} is accessible to group or other (mode {:o}; must be {}): another uid \
             could {}",
            path.display(),
            m.mode() & 0o7777,
            if p.dir { "0700" } else { "0600" },
            p.exposure
        ));
    }
    Ok(())
}

/// The custodian's store must be ITS OWN: a real directory owned by the uid
/// the custodian runs as, with no group or other access. Another owner, or a
/// group Fabric is in, could plant `<nonce>.issued` records (a nonce the
/// custodian never issued) or erase `.used` ones (a spent nonce spends again).
pub fn check_store(dir: &Path, euid: u32) -> Result<(), String> {
    check_private(dir, euid, &NONCE_STORE)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn me() -> u32 {
        // SAFETY: geteuid cannot fail.
        unsafe { libc::geteuid() }
    }

    /// The moved refusals keep the custodian's exact words (its operators
    /// and logs read them).
    #[test]
    fn the_nonce_store_refusals_keep_their_text() {
        let d = std::env::temp_dir().join(format!("axon-os-pp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir(&d).unwrap();
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o770)).unwrap();
        let e = check_store(&d, me()).unwrap_err();
        assert_eq!(
            e,
            format!(
                "nonce store {} is accessible to group or other (mode 770; must be 0700): \
                 another uid could plant or erase nonce records",
                d.display()
            )
        );
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
        let e = check_store(&d, me().wrapping_add(1)).unwrap_err();
        assert_eq!(
            e,
            format!(
                "nonce store {} is owned by uid {}, not the custodian's ({}): its owner could \
                 write nonce records",
                d.display(),
                me(),
                me().wrapping_add(1)
            )
        );
        let f = d.join("f");
        std::fs::write(&f, b"x").unwrap();
        let e = check_store(&f, me()).unwrap_err();
        assert_eq!(
            e,
            format!(
                "nonce store {} is not a directory (a symlink is never followed)",
                f.display()
            )
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}
