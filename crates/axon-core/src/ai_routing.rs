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

    /// The concrete Anthropic Messages-API model string for this tier — the
    /// `"model"` field a live request actually sends. The `model()` pair above
    /// is the provenance form (`anthropic:claude-opus` + `4.8`); this is the
    /// wire form (`claude-opus-4-8`). Overridable per-tier by env
    /// (`AXON_AI_MODEL_CHEAP` / `_BALANCED` / `_STRONG`) so a deployment — e.g.
    /// a TrainLoop/proxy gateway — can pin whatever concrete model it has
    /// provisioned without a recompile; the env value wins when set+non-empty.
    pub fn api_model(self) -> String {
        let (env_key, default) = match self {
            Tier::Cheap => ("AXON_AI_MODEL_CHEAP", "claude-haiku-4-5"),
            Tier::Balanced => ("AXON_AI_MODEL_BALANCED", "claude-sonnet-4-6"),
            Tier::Strong => ("AXON_AI_MODEL_STRONG", "claude-opus-4-8"),
        };
        std::env::var(env_key)
            .ok()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| default.to_string())
    }

    /// The comma-separated list of valid tier names, for the E1302 message.
    pub fn configured() -> &'static str {
        "cheap, balanced, strong"
    }

    /// Phase-7 `cost_meter` / F4: the per-token cost RATE for this tier, in
    /// integer micro-dollars (µ$) per 1000 tokens. Integer µ$ keeps the cost
    /// arithmetic exact and deterministic (no f64 in the meter). Like
    /// [`Self::model`], these are illustrative host defaults a real deployment
    /// overrides via config; the language guarantees only that the *tier* is
    /// stable and that `cheap < balanced < strong` in cost (stronger models cost
    /// more), so a cost budget behaves monotonically across tiers.
    pub fn rate_micro(self) -> i64 {
        match self {
            Tier::Cheap => 250,      // µ$ / 1k tokens  (e.g. haiku-class)
            Tier::Balanced => 3000,  // µ$ / 1k tokens  (e.g. sonnet-class)
            Tier::Strong => 15000,   // µ$ / 1k tokens  (e.g. opus-class)
        }
    }

    /// µ$ cost of a call estimated at `tokens` tokens at this tier's rate.
    /// `rate_micro` is per 1000 tokens; negative token counts clamp to 0.
    pub fn cost_micro(self, tokens: i64) -> i64 {
        let t = if tokens < 0 { 0 } else { tokens };
        self.rate_micro() * t / 1000
    }
}

/// Resolve the AI tier a fn's `ai_complete` calls run at, from its attributes:
/// the enclosing `@[ai(policy(tier: X))]`, else [`DEFAULT_TIER`]. `Err(raw)`
/// carries the unknown tier name (the caller raises E1302). This is the SINGLE
/// source of the policy→tier rule, shared by the interpreter's `current_ai_tier`
/// and the native codegen's tier-routing refusal (so both agree on which tier a
/// call uses — interp routes to it, native refuses cheap/strong it can't honor).
/// Per-call `tier:` args (R3b) are resolved separately by the caller; this is the
/// fn-level policy step only.
pub fn tier_from_attrs(attrs: &[crate::ast::Attr]) -> Result<Tier, String> {
    let Some(ai) = attrs.iter().find(|a| a.name == "ai") else {
        return Ok(DEFAULT_TIER);
    };
    for arg in &ai.args {
        if let Some(rest) = arg.strip_prefix("tier:") {
            let raw = rest.trim();
            return Tier::parse(raw).ok_or_else(|| raw.to_string());
        }
    }
    Ok(DEFAULT_TIER)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // The `AXON_AI_MODEL_*` env vars are process-global, but cargo runs tests in
    // parallel threads of ONE process. The default-model test (asserts the
    // built-ins when the vars are unset) and the override test (sets+removes a
    // var) therefore race: the override's set_var can leak into the default
    // test's read window. Serialize the two env-touching tests on this mutex so
    // neither observes the other's transient state.
    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    fn tier_from_attrs_resolves_policy_or_default() {
        use crate::ast::Attr;
        let attr = |name: &str, args: &[&str]| Attr {
            name: name.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
        };
        // No attrs / no @[ai] → DEFAULT_TIER (balanced — the one native honors).
        assert_eq!(tier_from_attrs(&[]), Ok(DEFAULT_TIER));
        assert_eq!(tier_from_attrs(&[attr("pure", &[])]), Ok(Tier::Balanced));
        // @[ai(policy(tier: X))] flattens to an `ai` attr carrying a `tier:` arg.
        assert_eq!(tier_from_attrs(&[attr("ai", &["policy", "tier: strong"])]), Ok(Tier::Strong));
        assert_eq!(tier_from_attrs(&[attr("ai", &["tier:cheap"])]), Ok(Tier::Cheap));
        assert_eq!(tier_from_attrs(&[attr("ai", &["tier: balanced"])]), Ok(Tier::Balanced));
        // An @[ai] with no tier: arg falls back to the default.
        assert_eq!(tier_from_attrs(&[attr("ai", &["budget: 3"])]), Ok(DEFAULT_TIER));
        // Unknown tier name surfaces the raw name (caller raises E1302).
        assert_eq!(tier_from_attrs(&[attr("ai", &["tier: turbo"])]), Err("turbo".to_string()));
    }

    #[test]
    fn api_model_is_tier_specific_and_distinct() {
        // R3 routing bug fix: the live request's `model` follows the tier, so
        // `strong` (t3) actually reaches the strong model — not a hardcoded one.
        // (Env vars are process-global; only assert the defaults when unset.)
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        if std::env::var_os("AXON_AI_MODEL_CHEAP").is_none()
            && std::env::var_os("AXON_AI_MODEL_BALANCED").is_none()
            && std::env::var_os("AXON_AI_MODEL_STRONG").is_none()
        {
            assert_eq!(Tier::Cheap.api_model(), "claude-haiku-4-5");
            assert_eq!(Tier::Balanced.api_model(), "claude-sonnet-4-6");
            assert_eq!(Tier::Strong.api_model(), "claude-opus-4-8");
            // All three distinct — selecting a tier changes the model.
            assert_ne!(Tier::Cheap.api_model(), Tier::Strong.api_model());
            assert_ne!(Tier::Balanced.api_model(), Tier::Strong.api_model());
        }
    }

    #[test]
    fn api_model_honors_env_override() {
        // A deployment (e.g. a TrainLoop/proxy gateway) pins a tier's concrete
        // model via env without a recompile.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var("AXON_AI_MODEL_STRONG", "my-gateway-opus");
        assert_eq!(Tier::Strong.api_model(), "my-gateway-opus");
        std::env::remove_var("AXON_AI_MODEL_STRONG");
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

    #[test]
    fn cost_rates_are_monotonic_by_tier() {
        // cheap < balanced < strong — a cost budget behaves monotonically.
        assert!(Tier::Cheap.rate_micro() < Tier::Balanced.rate_micro());
        assert!(Tier::Balanced.rate_micro() < Tier::Strong.rate_micro());
    }

    #[test]
    fn cost_micro_scales_with_tokens() {
        // 2000 tokens costs 2× 1000 tokens at the same tier.
        assert_eq!(Tier::Cheap.cost_micro(2000), Tier::Cheap.cost_micro(1000) * 2);
        // Balanced rate 3000 µ$/1k → 1000 tokens = 3000 µ$.
        assert_eq!(Tier::Balanced.cost_micro(1000), 3000);
        // Negative clamps to 0.
        assert_eq!(Tier::Strong.cost_micro(-5), 0);
    }
}
