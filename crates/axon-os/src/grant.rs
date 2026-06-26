//! R21 §3.2 — the capability grant model (types). Pure.
//!
//! S1 defines the *types* (Grant, Budget, ExecPolicy, Label, EffectSet) used by
//! the manifest. The *algebra* (`allows`, `intersect`, subset) lands in S2.

/// An allowlisted filesystem path prefix or network host (a plain string;
/// validated for traversal at manifest parse).
pub type PathPrefix = String;
pub type Host = String;

/// Process-spawning policy for a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecPolicy {
    None,
    Any,
}

impl ExecPolicy {
    /// Parse the manifest token `"none"` | `"any"`.
    pub fn parse(s: &str) -> Option<ExecPolicy> {
        match s {
            "none" => Some(ExecPolicy::None),
            "any" => Some(ExecPolicy::Any),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            ExecPolicy::None => "none",
            ExecPolicy::Any => "any",
        }
    }
}

/// Confidentiality level. Ordered: Public < Internal < Secret. `Ord` is derived
/// from the discriminant order so `<`/`max` work directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Label {
    Public = 0,
    Internal = 1,
    Secret = 2,
}

impl Label {
    pub fn parse(s: &str) -> Option<Label> {
        match s {
            "public" => Some(Label::Public),
            "internal" => Some(Label::Internal),
            "secret" => Some(Label::Secret),
            _ => None,
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Label::Public => "public",
            Label::Internal => "internal",
            Label::Secret => "secret",
        }
    }
}

/// A resource budget over a set of axes (mirrors the userland ResBudget). Any
/// axis overrun exhausts the whole budget (conjunctive contract).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub calls: i64,
    pub tokens: i64,
    pub cost_micro: i64,
}

/// The induced effect-set view of a grant: which capability axes are *present*
/// (an axis is present iff its allowlist is non-empty / exec ≠ none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct EffectSet {
    pub fs_read: bool,
    pub fs_write: bool,
    pub net: bool,
    pub exec: bool,
}

/// A capability grant: what a program is permitted to touch, spend, and handle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grant {
    pub fs_read: Vec<PathPrefix>,
    pub fs_write: Vec<PathPrefix>,
    pub net: Vec<Host>,
    pub exec: ExecPolicy,
    pub max_label: Label,
    pub budget: Budget,
}

impl Grant {
    /// The induced effect set (R21 §3.2): an axis is present iff its allowlist
    /// is non-empty (exec iff policy is `Any`).
    pub fn effect_set(&self) -> EffectSet {
        EffectSet {
            fs_read: !self.fs_read.is_empty(),
            fs_write: !self.fs_write.is_empty(),
            net: !self.net.is_empty(),
            exec: matches!(self.exec, ExecPolicy::Any),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_ordering_is_public_lt_internal_lt_secret() {
        assert!(Label::Public < Label::Internal);
        assert!(Label::Internal < Label::Secret);
        assert_eq!(Label::Internal.max(Label::Public), Label::Internal);
    }

    #[test]
    fn effect_set_is_present_iff_allowlist_nonempty() {
        let g = Grant {
            fs_read: vec!["./data/".into()],
            fs_write: vec![],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 1,
                tokens: 1,
                cost_micro: 1,
            },
        };
        let e = g.effect_set();
        assert!(e.fs_read);
        assert!(!e.fs_write && !e.net && !e.exec);
    }

    #[test]
    fn exec_present_only_when_any() {
        let mut g = Grant {
            fs_read: vec![],
            fs_write: vec![],
            net: vec![],
            exec: ExecPolicy::Any,
            max_label: Label::Public,
            budget: Budget {
                calls: 0,
                tokens: 0,
                cost_micro: 0,
            },
        };
        assert!(g.effect_set().exec);
        g.exec = ExecPolicy::None;
        assert!(!g.effect_set().exec);
    }
}
