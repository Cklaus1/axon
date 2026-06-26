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

    // ── S2: the grant algebra (allows / intersect / subset) ─────────────────

    /// Admission predicate (R21 §3.2): does this grant permit every effect axis
    /// present in `needed`? (`declared ⊆ grant.effect_set()`.)
    pub fn allows(&self, needed: &EffectSet) -> bool {
        needed.subset_of(&self.effect_set())
    }

    /// `self ⊆ other` on every axis — caps (each present axis ⊆), budget (each
    /// resource ≤), and label (≤). The attenuation predicate at the supervisor
    /// boundary: a delegated grant must be a subset of the supervisor's own.
    pub fn is_subset_of(&self, other: &Grant) -> bool {
        prefixes_within(&self.fs_read, &other.fs_read)
            && prefixes_within(&self.fs_write, &other.fs_write)
            && hosts_within(&self.net, &other.net)
            && (self.effect_set().exec <= other.effect_set().exec)
            && self.max_label <= other.max_label
            && self.budget.calls <= other.budget.calls
            && self.budget.tokens <= other.budget.tokens
            && self.budget.cost_micro <= other.budget.cost_micro
    }

    /// `self ∩ other` (R21 §4.1): the grant that permits only what BOTH permit.
    /// The result is provably ⊆ both — you cannot delegate authority you lack.
    /// Per axis: keep, for each overlapping prefix pair, the NARROWER prefix
    /// (sound: it is ⊆ both); exec = none unless both Any; label/budget = min.
    pub fn intersect(&self, other: &Grant) -> Grant {
        Grant {
            fs_read: intersect_prefixes(&self.fs_read, &other.fs_read),
            fs_write: intersect_prefixes(&self.fs_write, &other.fs_write),
            net: intersect_hosts(&self.net, &other.net),
            exec: if matches!(self.exec, ExecPolicy::Any) && matches!(other.exec, ExecPolicy::Any) {
                ExecPolicy::Any
            } else {
                ExecPolicy::None
            },
            max_label: self.max_label.min(other.max_label),
            budget: self.budget.min(&other.budget),
        }
    }
}

impl Budget {
    /// Per-axis minimum — the conjunctive (tightest-binds) budget.
    pub fn min(&self, o: &Budget) -> Budget {
        Budget {
            calls: self.calls.min(o.calls),
            tokens: self.tokens.min(o.tokens),
            cost_micro: self.cost_micro.min(o.cost_micro),
        }
    }
}

impl EffectSet {
    /// Every axis present in `self` is present in `other` (`self ⊆ other`).
    pub fn subset_of(&self, other: &EffectSet) -> bool {
        (!self.fs_read || other.fs_read)
            && (!self.fs_write || other.fs_write)
            && (!self.net || other.net)
            && (!self.exec || other.exec)
    }
}

/// `prefix` is an ancestor-or-equal of `path` (path lies under prefix).
fn is_ancestor(prefix: &str, path: &str) -> bool {
    path == prefix || path.starts_with(prefix)
}

/// A host is allowed by `list` if some entry equals it or globs it (`*.x`).
fn host_allows(list: &[String], host: &str) -> bool {
    list.iter().any(|h| host_matches(h, host))
}
fn host_matches(pat: &str, host: &str) -> bool {
    match pat.strip_prefix('*') {
        Some(suffix) => host.ends_with(suffix), // "*.x.com" matches "a.x.com"
        None => pat == host,
    }
}

/// Every prefix in `a` lies within some prefix in `b` (a's allowed region ⊆ b's).
fn prefixes_within(a: &[String], b: &[String]) -> bool {
    a.iter().all(|pa| b.iter().any(|pb| is_ancestor(pb, pa)))
}
fn hosts_within(a: &[String], b: &[String]) -> bool {
    a.iter()
        .all(|ha| host_allows(b, ha) || b.iter().any(|hb| hb == ha))
}

fn push_unique(out: &mut Vec<String>, s: String) {
    if !out.contains(&s) {
        out.push(s);
    }
}

/// The narrower of every overlapping prefix pair (⊆ both). Sound + dedup'd.
fn intersect_prefixes(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for pa in a {
        for pb in b {
            if is_ancestor(pa, pb) {
                push_unique(&mut out, pb.clone()); // pb lies under pa ⇒ pb narrower
            } else if is_ancestor(pb, pa) {
                push_unique(&mut out, pa.clone());
            } // disjoint ⇒ grant nothing (sound)
        }
    }
    out
}
fn intersect_hosts(a: &[String], b: &[String]) -> Vec<String> {
    // A host pattern survives iff it is permitted by the other side.
    let mut out: Vec<String> = Vec::new();
    for ha in a {
        if host_allows(b, ha) || b.contains(ha) {
            push_unique(&mut out, ha.clone());
        }
    }
    for hb in b {
        if host_allows(a, hb) || a.contains(hb) {
            push_unique(&mut out, hb.clone());
        }
    }
    out
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

    fn g(
        fs_read: &[&str],
        net: &[&str],
        exec: ExecPolicy,
        label: Label,
        budget: (i64, i64, i64),
    ) -> Grant {
        Grant {
            fs_read: fs_read.iter().map(|s| s.to_string()).collect(),
            fs_write: vec![],
            net: net.iter().map(|s| s.to_string()).collect(),
            exec,
            max_label: label,
            budget: Budget {
                calls: budget.0,
                tokens: budget.1,
                cost_micro: budget.2,
            },
        }
    }

    #[test]
    fn intersect_clamps_to_the_smaller_on_every_axis() {
        let s = g(
            &["./"],
            &["*.x.com"],
            ExecPolicy::Any,
            Label::Secret,
            (100, 100, 100),
        );
        let j = g(
            &["./data/"],
            &["a.x.com"],
            ExecPolicy::None,
            Label::Internal,
            (50, 200, 10),
        );
        let i = j.intersect(&s);
        assert_eq!(i.fs_read, vec!["./data/".to_string()]); // narrower prefix kept
        assert_eq!(i.net, vec!["a.x.com".to_string()]); // matched by *.x.com
        assert_eq!(i.exec, ExecPolicy::None); // none unless BOTH any
        assert_eq!(i.max_label, Label::Internal); // min label
        assert_eq!(i.budget.calls, 50); // per-axis min
        assert_eq!(i.budget.tokens, 100);
        assert_eq!(i.budget.cost_micro, 10);
        assert!(i.is_subset_of(&s) && i.is_subset_of(&j));
    }

    #[test]
    fn mint_cannot_exceed_supervisor_grant() {
        // Supervisor holds neither net nor exec; a job asking for both must, after
        // intersection, hold neither — authority cannot be manufactured (R20 at the
        // supervisor boundary).
        let supervisor = g(
            &["./data/"],
            &[],
            ExecPolicy::None,
            Label::Internal,
            (10, 10, 10),
        );
        let job = g(
            &["./data/", "./secret/"],
            &["evil.com"],
            ExecPolicy::Any,
            Label::Secret,
            (999, 999, 999),
        );
        let eff = job.intersect(&supervisor);
        assert!(!eff.effect_set().net, "net cannot be manufactured");
        assert!(!eff.effect_set().exec, "exec cannot be manufactured");
        assert_eq!(eff.max_label, Label::Internal, "label clamped down");
        assert!(eff.budget.calls <= 10, "budget clamped to supervisor");
        // ./secret/ is not under any supervisor prefix ⇒ dropped.
        assert_eq!(eff.fs_read, vec!["./data/".to_string()]);
        assert!(
            eff.is_subset_of(&supervisor),
            "effective grant ⊆ supervisor"
        );
    }

    #[test]
    fn allows_is_the_admission_predicate() {
        let grant = g(
            &["./data/"],
            &[],
            ExecPolicy::None,
            Label::Public,
            (1, 1, 1),
        );
        let needs_read = EffectSet {
            fs_read: true,
            ..Default::default()
        };
        let needs_net = EffectSet {
            net: true,
            ..Default::default()
        };
        assert!(grant.allows(&needs_read));
        assert!(!grant.allows(&needs_net));
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
