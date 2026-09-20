//! Every `AXON_*` environment variable the shipped code reads, with what it does.
//!
//! WHY THIS EXISTS.
//!
//! The meanings lived only in `CLAUDE.md`'s table, which documents 21 of 47 and
//! says so. That is the right call for a curated card — but it left no tool able
//! to enumerate them, so `axon reference` could describe verbs, builtins and
//! diagnostic codes exhaustively and say nothing about the knobs that change how
//! a run BEHAVES. A var that silently does nothing (or silently does something)
//! is the worst case: `AXON_ALLOWED_EFFECTS` and `AXON_BUDGET_TOKENS` were both
//! documented as enforced while being read by nothing at all, and only a
//! behavioural diff found it.
//!
//! Scope is the SHIPPED source (`crates/*/src`), not test harnesses. A variable
//! only a test reads is not part of the language, and listing it would imply a
//! supported knob.

/// `(name, description)` for every `AXON_*` var read by shipped code.
///
/// Gated in both directions by the tests below: a var read without a row here
/// fails, and a row here for a var nothing reads fails too.
pub const ALL_ENV_VARS: &[(&str, &str)] = &[
    // ── Determinism & execution ──────────────────────────────────────────
    ("AXON_SEED", "seed the RNG (u64) so `random_*` runs reproduce"),
    ("AXON_MAX_DEPTH", "recursion-depth ceiling (default 6000, clamped to 1,000,000); the interpreter thread stack scales with it"),
    ("AXON_CLOCK", "deterministic virtual clock `<start_ms>[:<tick_ms>]`; `sleep_ms` advances it without really sleeping"),
    ("AXON_PATH", "colon-separated module search path for `mod`/`use` imports"),
    ("AXON_STRICT", "promote advisory hazard diagnostics to errors (today E0302, an unused Result); `axon deploy` sets it itself"),
    // ── Record / replay ──────────────────────────────────────────────────
    ("AXON_RECORD", "path to write a host journal: every call through the AxonHost seam, performed for real and appended with its outcome. As sensitive as the run it records"),
    ("AXON_REPLAY", "serve a run from a host journal instead of the world; nothing is performed, and any miss is a divergence (exit 11). Mutually exclusive with AXON_RECORD"),
    ("AXON_AI_REPLAY", "path to an LLM-call replay cache; memoizes `ai_complete` by (prompt, model) so an AI run reproduces with no live call"),
    // ── AI routing & policy ──────────────────────────────────────────────
    ("AXON_AI_MOCK", "use deterministic stub AI responses instead of live calls (the real per-token cost is still metered)"),
    ("AXON_CORTEX_GENERATOR_TIMEOUT_MS", "how long `cortex repair --generator cmd:PATH` waits for the operator's program to answer (default 120s). Exists so the deadline can be TESTED: a suite cannot wait two minutes, and an untested deadline is the kind of check that turns out never to fire. A malformed or zero value is ignored in favour of the default rather than disabling every generator"),
    ("AXON_AI_PROVIDER", "live-AI codec: `anthropic` or `openai`"),
    ("AXON_AI_BASE_URL", "gateway URL for live AI calls"),
    ("AXON_AI_API_KEY", "API key for live AI calls"),
    ("AXON_AI_MODEL_CHEAP", "override the model `@[ai(tier: cheap)]` resolves to"),
    ("AXON_AI_MODEL_BALANCED", "override the model `@[ai(tier: balanced)]` resolves to"),
    ("AXON_AI_MODEL_STRONG", "override the model `@[ai(tier: strong)]` resolves to"),
    ("AXON_BUDGET_TOKENS", "run-wide AI token cap; the call that would exceed it halts with E1303 (exit 5) BEFORE dispatch. A malformed value fails closed to 0"),
    ("AXON_DOTENV", "path to a `.env` file to load for AI configuration"),
    ("AXON_DOTENV_WALK", "walk parent directories looking for a `.env` file"),
    ("AXON_INTENT_GEN", "let `axon intent compile` fill TODO stubs via a live model (needs `--features asi-runtime`)"),
    // ── Capabilities, principals, audit ──────────────────────────────────
    ("AXON_ALLOWED_EFFECTS", "ambient effect ceiling for the whole run; a true ceiling that an inner sandbox may narrow but never widen. EMPTY means deny every effect and is not the same as unset. Interpreter-only"),
    ("AXON_PRINCIPAL", "the principal a run executes as — audit ATTRIBUTION only; it grants and withholds nothing"),
    ("AXON_GUEST_ALLOW_NO_POLICY", "axon-guest-init: start the guest even though no MMDS capability policy could be loaded (no effect ceiling, no token cap, no seccomp). Development only — without it an unreadable policy REFUSES to start the guest, because an absent policy is not a permissive one"),
    ("AXON_REQUIRE_CERTS", "fail closed on the R23 solver-free kernel-mint certificate check instead of the default silent pass"),
    ("AXON_ATTEST_KEY", "operator-provisioned attestation key (hex, >=16 bytes). When set, axon-vm signs AND verifies the attestation report under it, so a report signed by anyone else fails. Unset falls back to an ephemeral per-process key, where signer and verifier are the same process — real integrity over the measurement, but attesting nothing to a third party"),
    ("AXON_AUDIT_LEDGER", "path to the R28 capability audit ledger"),
    ("AXON_AUDIT_DETERMINISTIC", "use a counter instead of a clock for ledger timestamps, so audit output is reproducible in tests"),
    ("AXON_KILL_FILE", "kill file the axon-os supervisor polls; its EXISTENCE trips nothing — the job stops only once its CONTENT reads `{\"latch\":\"tripped\"}`"),
    // ── Goal search ──────────────────────────────────────────────────────
    ("AXON_GOAL_CONTINUE", "resume a `goal` search from the best prior input in the provenance log (set automatically by `axon goal --iterate`)"),
    // ── Session (R44) ────────────────────────────────────────────────────
    ("AXON_DUMP_BINDINGS", "write a cell's final bindings back as Axon source literals — the session substrate, also usable by an external driver"),
    ("AXON_DUMP_SHAPES", "write a `name: shape` inventory of every binding, including ones that could not be persisted"),
    // ── SMT proving ──────────────────────────────────────────────────────
    ("AXON_PROOF_TIMEOUT_MS", "per-obligation SMT solver timeout; 0 selects the Z3-free runtime-check fallback"),
    ("AXON_PROOF_DEPTH", "SMT unrolling/search depth bound"),
    // ── Substrates: VM, OS, wasm, native ─────────────────────────────────
    ("AXON_HOST_SOCKET", "guest-kernel hypercall bridge socket; takes priority over AXON_VM_VSOCK_PORT for `host_await`"),
    ("AXON_VM_VSOCK_PORT", "vsock port for `host_await` inside an axon-vm microVM (set by the launcher)"),
    ("AXON_VM_ALLOWED_EFFECTS", "effect ceiling delivered to a guest via the VM's MMDS policy"),
    ("AXON_VM_TIMEOUT_SECS", "wall-clock bound on a microVM run"),
    ("AXON_VM_SOCKET_TIMEOUT_SECS", "bound on waiting for the VM's control socket"),
    ("AXON_VM_DEBUG", "verbose microVM launch diagnostics"),
    ("AXON_VM_QUIET", "suppress microVM launch chatter"),
    ("AXON_CONFIG_DIR", "override the config directory the VM reads its Principal registry from"),
    ("AXON_CI_NO_KVM", "force mock mode where /dev/kvm is unavailable (CI)"),
    ("AXON_OS_TIMEOUT_MS", "axon-os bound on a supervised job (`run_bounded`)"),
    ("AXON_WASM_RT", "path to the wasm runtime used to execute a built wasm artifact"),
    ("AXON_ANDROID_API", "Android API level for the NDK cross-link"),
    ("AXON_NATIVE_TRACE", "trace native-FFI module calls (gfx mock)"),
    ("AXON_BIN", "path to the `axon` binary an out-of-process synthesizer should invoke"),
    ("AXON_INTENT_TIMEOUT_MS", "bound on the `axon intent` subprocess synthesizer"),
    // ── Test fixtures that shipped code reads ────────────────────────────
    ("AXON_TEST_DOTENV_VAR", "(test fixture) name of a variable the .env loader test expects to find"),
    ("AXON_TEST_DOTENV_NEW", "(test fixture) asserts the .env loader does not clobber an already-set variable"),
    ("AXON_HOST_TEST_VAR", "(test fixture) exercises the host seam's env_var path"),
    // ── Read through the HOST SEAM, not std::env ─────────────────────────
    // These two had no row for as long as they have existed, because the
    // scan above matched only literal `env::var(` forms. R24 TEE.
    ("AXON_TEE_ENCLAVE", "R24 TEE: set to 1 by the gramine-direct manifest to signal the workload is executing inside an enclave; this is what makes `tee_in_enclave()` return true. Read through the host seam, so it is recorded and replayed"),
    ("AXON_TEE_MEASUREMENT", "R24 TEE: the simulated enclave launch measurement returned by `tee_attest_measurement()` when set, a stub otherwise. A genuine hardware-rooted quote comes only from confidential hardware. Read through the host seam"),
];

#[cfg(test)]
mod tests {
    use super::ALL_ENV_VARS;
    use std::collections::HashSet;
    use std::path::Path;

    /// Read every `.rs` under `crates/*/src`, skipping this registry itself.
    fn shipped_source() -> String {
        let crates = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
        let mut out = String::new();
        fn walk(dir: &Path, out: &mut String) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && p.file_name().is_some_and(|f| f != "env_registry.rs")
                {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        out.push_str(&t);
                    }
                }
            }
        }
        let Ok(rd) = std::fs::read_dir(crates) else {
            return out;
        };
        for e in rd.flatten() {
            let src = e.path().join("src");
            if src.is_dir() {
                walk(&src, &mut out);
            }
        }
        out
    }

    /// Every `AXON_*` literal handed to `env::var`/`env::var_os` in shipped code.
    fn vars_read(src: &str) -> HashSet<String> {
        let mut found = HashSet::new();
        // `.env_var("` is the HOST SEAM, and leaving it out was a hole in the
        // guard rather than a gap in the registry. `tee_in_enclave` and
        // `tee_attest_measurement` read `AXON_TEE_ENCLAVE` and
        // `AXON_TEE_MEASUREMENT` through `with_host(|h| h.env_var(..))`, so
        // neither matched the scan, neither had a registry row, and neither
        // appeared in `AXON_REFERENCE.md` — while this test reported full
        // coverage in both directions. An attestation-category signal that
        // decides whether `tee_in_enclave()` returns true was invisible to the
        // tool whose whole job is enumerating controls.
        //
        // Reading via the seam is the BETTER choice, not a mistake: those
        // reads are recorded and replayed, unlike every `std::env::var` read.
        // The scan had simply never been taught the shape. Measured when
        // added: exactly three new names, no false positives from prose.
        for pat in ["env::var(\"", "env::var_os(\"", ".env_var(\""] {
            let mut rest = src;
            while let Some(i) = rest.find(pat) {
                rest = &rest[i + pat.len()..];
                if let Some(end) = rest.find('"') {
                    let name = &rest[..end];
                    if name.starts_with("AXON_") {
                        found.insert(name.to_string());
                    }
                }
            }
        }
        // Shape 2: a const bound to the literal, then read through its name.
        // `replay.rs` does this for AXON_RECORD/AXON_REPLAY, and a rule that
        // flagged those correct sites would be worse than the loose one.
        for line in src.lines() {
            if line.contains(": &str = \"AXON_") || line.contains(": &'static str = \"AXON_") {
                if let Some(i) = line.find("\"AXON_") {
                    let r = &line[i + 1..];
                    if let Some(end) = r.find('"') {
                        found.insert(r[..end].to_string());
                    }
                }
            }
        }
        found
    }

    #[test]
    fn every_env_var_read_by_shipped_code_is_in_the_registry() {
        let src = shipped_source();
        // Guard the guard: an empty scan would satisfy "nothing missing".
        assert!(
            src.len() > 500_000,
            "source scan produced only {} bytes — the guard would pass vacuously",
            src.len()
        );
        let read = vars_read(&src);
        assert!(read.len() > 30, "expected many vars, found {}", read.len());

        let registered: HashSet<&str> = ALL_ENV_VARS.iter().map(|(n, _)| *n).collect();
        let mut missing: Vec<&String> = read
            .iter()
            .filter(|v| !registered.contains(v.as_str()))
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "these AXON_* vars are read but have no registry row: {missing:?}\n\
             Add `(\"NAME\", \"what it does\"),` to ALL_ENV_VARS."
        );
    }

    #[test]
    fn the_registry_lists_no_variable_nothing_reads() {
        // The reverse direction. A documented var that no code reads is the
        // defect that shipped twice already: AXON_ALLOWED_EFFECTS and
        // AXON_BUDGET_TOKENS were both described as enforced while being read
        // by nothing, and only a behavioural diff caught it.
        let src = shipped_source();
        assert!(src.len() > 500_000, "source scan failed — guard is vacuous");
        let mut phantom: Vec<&str> = ALL_ENV_VARS
            .iter()
            .map(|(n, _)| *n)
            .filter(|n| !src.contains(*n))
            .collect();
        phantom.sort();
        assert!(
            phantom.is_empty(),
            "the registry documents vars no shipped code reads: {phantom:?}"
        );
    }

    #[test]
    fn the_registry_is_unique_and_described() {
        let mut seen = HashSet::new();
        for (name, desc) in ALL_ENV_VARS {
            assert!(seen.insert(*name), "duplicate registry row: {name}");
            assert!(name.starts_with("AXON_"), "not an Axon var: {name}");
            assert!(
                desc.len() > 15,
                "{name}'s description is too thin to be useful: {desc:?}"
            );
        }
    }
}
