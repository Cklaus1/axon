//! Tier → model routing (R3 §4.2/§4.4).
//!
//! A program never names a concrete model — it names a **tier** (`cheap` |
//! `balanced` | `strong`), and a *host-side* table maps that tier to a concrete
//! `(model, version)`. This is what makes a program reproducible across model
//! generations: the tier is stable source; the host pins the exact version into
//! the AiCall provenance record (R3 §4.3). Tiers are the only routing surface
//! the language commits to — `cheap < balanced < strong` by intended capability.
//!
//! Resolution order (R3 §4.2): per-call `tier:` arg (deferred — needs named-arg
//! grammar) → the enclosing `@[ai(policy(tier: …))]` → the default tier.
//! This module owns the table + the parse; the interpreter applies it and
//! stamps the resolved `(tier, model, version)` into provenance.

/// The closed tier enum. `cheap`/`balanced`/`strong` are the only valid tiers;
/// an unknown name is **E1302**.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Cheap,
    Balanced,
    Strong,
}

/// The default tier when neither a per-call `tier:` nor a policy tier is given
/// (R3 §4.2 step 3).
pub const DEFAULT_TIER: Tier = Tier::Balanced;

impl Tier {
    /// The stable tier name as written in source / recorded in provenance.
    pub fn as_str(self) -> &'static str {
        match self {
            Tier::Cheap => "cheap",
            Tier::Balanced => "balanced",
            Tier::Strong => "strong",
        }
    }

    /// Parse a tier name. `None` for an unknown name (the caller raises E1302).
    pub fn parse(s: &str) -> Option<Tier> {
        match s.trim() {
            "cheap" => Some(Tier::Cheap),
            "balanced" => Some(Tier::Balanced),
            "strong" => Some(Tier::Strong),
            _ => None,
        }
    }

    /// The concrete `(model, version)` this tier routes to in the default host
    /// table. The version string is what gets pinned into provenance so a replay
    /// (R9) knows the exact model generation. A real deployment overrides this
    /// table via config; the language only guarantees the *tier* is stable.
    pub fn model(self) -> (&'static str, &'static str) {
        match self {
            // (model id, version) — illustrative defaults; a host config maps
            // tiers to whatever concrete models it has provisioned.
            Tier::Cheap => ("anthropic:claude-haiku", "4.5"),
            Tier::Balanced => ("anthropic:claude-sonnet", "4.6"),
            Tier::Strong => ("anthropic:claude-opus", "4.8"),
        }
    }

    /// The comma-separated list of valid tier names, for the E1302 message.
    pub fn configured() -> &'static str {
        "cheap, balanced, strong"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tier_roundtrips_through_name() {
        for t in [Tier::Cheap, Tier::Balanced, Tier::Strong] {
            assert_eq!(Tier::parse(t.as_str()), Some(t));
        }
    }

    #[test]
    fn unknown_tier_is_none() {
        assert_eq!(Tier::parse("turbo"), None);
        assert_eq!(Tier::parse(""), None);
    }

    #[test]
    fn each_tier_maps_to_a_distinct_model() {
        let (cm, _) = Tier::Cheap.model();
        let (bm, _) = Tier::Balanced.model();
        let (sm, _) = Tier::Strong.model();
        assert_ne!(cm, bm);
        assert_ne!(bm, sm);
        assert_ne!(cm, sm);
    }

    #[test]
    fn default_tier_is_balanced() {
        assert_eq!(DEFAULT_TIER, Tier::Balanced);
        assert_eq!(DEFAULT_TIER.as_str(), "balanced");
    }
}
