//! Capability PROFILES — the product-layer answer to "what may a job do by
//! default?".
//!
//! Axon's product policy is **easy by default, explicit lockdown when needed**:
//! a normal project works out of the box — network on, workspace readable and
//! writable, subprocesses allowed — so package managers, git and HTTP APIs need
//! no setup.
//!
//! The important part is WHERE that permissiveness lives. It is a named profile
//! here, not a meaning smuggled into a low-level primitive. Until now an empty
//! host list meant "unrestricted" inside `sandbox_create_scoped`, so the policy
//! was invisible: a grant that named no hosts read as deny-all to every human
//! and as allow-all to the runtime — and it was the inverse of the sibling
//! convention, where `AXON_ALLOWED_EFFECTS=` empty means deny every effect.
//!
//! So the primitives are now unambiguous — `""` denies, `"*"` is unrestricted —
//! and a profile states the broad grant EXPLICITLY. The developer default still
//! grants everything; it just says so, as `net = ["*"]` rather than `net = []`.
//! That is what makes the tighter profiles below introducible later without
//! changing what any existing policy primitive means.

use crate::grant::Budget;
use crate::grant::{ExecPolicy, Grant, Host, Label, PathPrefix};

/// A named capability posture. `Developer` is the default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Profile {
    /// Easy by default: everything a normal project needs, no setup.
    #[default]
    Developer,
    /// Workspace-scoped filesystem, network still open. Intended as the step
    /// most projects can take without noticing.
    Balanced,
    /// Explicit allowlists only — nothing broad is granted implicitly.
    Restricted,
    /// No ambient authority at all: no network, no filesystem, no subprocess.
    Hermetic,
}

impl Profile {
    pub fn name(self) -> &'static str {
        match self {
            Profile::Developer => "developer",
            Profile::Balanced => "balanced",
            Profile::Restricted => "restricted",
            Profile::Hermetic => "hermetic",
        }
    }

    /// Parse a profile name. Unknown names are an ERROR rather than a silent
    /// fallback to the permissive default: a typo in `profile = "restrcted"`
    /// must not quietly hand a job the developer grant.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "developer" | "dev" | "easy" => Ok(Profile::Developer),
            "balanced" => Ok(Profile::Balanced),
            "restricted" => Ok(Profile::Restricted),
            "hermetic" => Ok(Profile::Hermetic),
            other => Err(format!(
                "unknown profile `{other}` — expected one of: developer, \
                 balanced, restricted, hermetic"
            )),
        }
    }

    /// The default host allowlist for a dimension the job did not specify.
    ///
    /// Returned as a concrete list, never as "absent means everything". A
    /// caller that omits `net` under the developer profile gets `["*"]` written
    /// into its grant, so the manifest it round-trips to says what it may do.
    pub fn default_net(self) -> Vec<Host> {
        match self {
            Profile::Developer | Profile::Balanced => vec!["*".to_string()],
            Profile::Restricted | Profile::Hermetic => Vec::new(),
        }
    }

    pub fn default_fs_read(self) -> Vec<PathPrefix> {
        match self {
            Profile::Developer => vec!["*".to_string()],
            // Workspace-relative, which is what a build actually needs.
            Profile::Balanced => vec!["./".to_string()],
            Profile::Restricted | Profile::Hermetic => Vec::new(),
        }
    }

    pub fn default_fs_write(self) -> Vec<PathPrefix> {
        match self {
            Profile::Developer => vec!["*".to_string()],
            Profile::Balanced => vec!["./".to_string()],
            Profile::Restricted | Profile::Hermetic => Vec::new(),
        }
    }

    pub fn default_exec(self) -> ExecPolicy {
        match self {
            Profile::Developer | Profile::Balanced => ExecPolicy::Any,
            Profile::Restricted | Profile::Hermetic => ExecPolicy::None,
        }
    }

    /// The whole grant this profile hands a job that specifies nothing.
    pub fn default_grant(self, max_label: Label, budget: Budget) -> Grant {
        Grant {
            fs_read: self.default_fs_read(),
            fs_write: self.default_fs_write(),
            net: self.default_net(),
            exec: self.default_exec(),
            max_label,
            budget,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The product policy, asserted: a job that asks for nothing under the
    /// default profile can still do the things a normal project does.
    #[test]
    fn the_default_profile_is_easy() {
        let p = Profile::default();
        assert_eq!(p, Profile::Developer);
        assert_eq!(p.default_net(), vec!["*".to_string()]);
        assert_eq!(p.default_fs_read(), vec!["*".to_string()]);
        assert_eq!(p.default_fs_write(), vec!["*".to_string()]);
        assert_eq!(p.default_exec(), ExecPolicy::Any);
    }

    /// ...and it says so EXPLICITLY. The whole point of the change: broad
    /// authority is written as `*`, never as an empty list that the runtime
    /// secretly reads as unrestricted.
    #[test]
    fn broad_authority_is_spelled_out_not_left_empty() {
        for p in [Profile::Developer, Profile::Balanced] {
            assert!(
                !p.default_net().is_empty(),
                "{} must state its network grant",
                p.name()
            );
        }
    }

    /// The tighter profiles grant nothing implicitly — an empty list here means
    /// deny, which is now what the primitive means too.
    #[test]
    fn hermetic_grants_no_ambient_authority() {
        let h = Profile::Hermetic;
        assert!(h.default_net().is_empty());
        assert!(h.default_fs_read().is_empty());
        assert!(h.default_fs_write().is_empty());
        assert_eq!(h.default_exec(), ExecPolicy::None);
    }

    #[test]
    fn restricted_is_allowlist_only_but_still_a_real_profile() {
        let r = Profile::Restricted;
        assert!(r.default_net().is_empty());
        assert_eq!(r.default_exec(), ExecPolicy::None);
        assert_eq!(Profile::parse("restricted").unwrap(), r);
    }

    /// A misspelled profile must not fall back to the permissive default.
    #[test]
    fn an_unknown_profile_is_refused_not_defaulted() {
        let e = Profile::parse("restrcted").unwrap_err();
        assert!(e.contains("unknown profile"), "{e}");
        assert!(e.contains("hermetic"), "and lists the valid names: {e}");
    }

    #[test]
    fn profile_names_round_trip() {
        for p in [
            Profile::Developer,
            Profile::Balanced,
            Profile::Restricted,
            Profile::Hermetic,
        ] {
            assert_eq!(Profile::parse(p.name()).unwrap(), p);
        }
    }
}
