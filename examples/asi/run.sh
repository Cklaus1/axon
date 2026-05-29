#!/usr/bin/env bash
# Axon ASI demo — Phase-10 CLI surface, simulated as a bash wrapper.
#
# This is the engineering-v1 CLI shape from ROADMAP §2.5.  Each command
# below is what the eventual `axon goal …` / `axon ast …` / `axon trace …`
# subcommands will look like once Phase 10 ships.  The wrapper exists so
# the demo end-to-end loop is exercisable today on shipped primitives.
#
# Subcommands intentionally produce stable JSON-friendly output where
# possible so a future thin-wrapper UI can consume them directly.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
DEMO="$HERE/${DEMO:-optimize.ax}"

# Provenance log path — see crates/axon-rt/src/provenance.rs
PROV="${XDG_CACHE_HOME:-$HOME/.cache}/axon/provenance.jsonl"

axon_bin() {
    if [[ -x "$REPO/target/release/axon" ]]; then
        echo "$REPO/target/release/axon"
    elif [[ -x "$REPO/target/debug/axon" ]]; then
        echo "$REPO/target/debug/axon"
    elif command -v axon >/dev/null 2>&1; then
        command -v axon
    else
        echo "ERROR: axon binary not found. Build it: cargo build -p axon-core --no-default-features --bin axon" >&2
        exit 1
    fi
}

cmd="${1:-help}"
shift || true

case "$cmd" in
    demos)
        # Run the key-free deterministic decision-pattern demos end to end — the
        # public-face tour. (The LLM demos #1–3 need AXON_AI_MOCK=1 or a key.)
        bin="$(axon_bin)"
        for d in supervised_agent deliberative_agent planner pareto allocate rank; do
            echo "── $d ──"
            "$bin" run "$HERE/$d.ax" || echo "  (demo $d exited $?)"
            echo ""
        done
        ;;
    # ── Phase-10 CLI shape ──────────────────────────────────────────────────
    compile|check)
        # Future: axon ast review optimize.ax
        # Today: parse + type-check only
        "$(axon_bin)" check "$DEMO"
        ;;
    run)
        # Future: axon goal run optimize.ax
        # Today: full compile + execute
        if [[ -z "${ANTHROPIC_API_KEY:-}" && -z "${AXON_AI_MOCK:-}" ]]; then
            echo "NOTE: neither ANTHROPIC_API_KEY nor AXON_AI_MOCK is set." >&2
            echo "      Key-free deterministic run:  AXON_AI_MOCK=1 ./run.sh run" >&2
            echo "      (ai_complete returns stubs; works on the codegen-free interpreter)." >&2
            echo "      Live inference: build with --features asi-runtime and set ANTHROPIC_API_KEY." >&2
            echo "" >&2
        fi
        "$(axon_bin)" run "$DEMO"
        ;;
    trace)
        # Now first-class: `axon trace` summarizes the per-fn score trajectory.
        # Extra args pass through (e.g. --fn try_variant).
        "$(axon_bin)" trace "$@"
        ;;
    improve)
        # Continue the search across runs: AXON_GOAL_CONTINUE makes goal_run
        # resume its hill-climb from the best input recorded in the persisted
        # provenance log, so repeated `improve` runs converge.
        echo "# Continuing search from existing provenance log (AXON_GOAL_CONTINUE=1)"
        AXON_GOAL_CONTINUE=1 "$(axon_bin)" run "$DEMO"
        ;;
    redteam)
        # Future: axon redteam <plan-id>
        # Today: rerun — the demo's main runs a redteam pass at the end
        echo "# Redteam pass is included in the main run"
        "$(axon_bin)" run "$DEMO"
        ;;
    replay)
        # Future: axon trace replay <run-id> — deterministic re-execution
        # GAP: not implemented.  Phase 9 deliverable.  Today's runs are
        # non-deterministic because ai_complete responses vary.
        echo "GAP: replay requires deterministic ai_complete (Phase 9)." >&2
        echo "     Today's traces are observational, not replayable." >&2
        exit 64
        ;;
    log)
        # Future: axon log --principal <id>
        # Today: dump the entire provenance log as NDJSON
        if [[ -f "$PROV" ]]; then
            cat "$PROV"
        else
            echo "No provenance log yet" >&2
            exit 1
        fi
        ;;
    analyze)
        # Score-trajectory analysis of the provenance log: per-fn score stats,
        # plateau detection, and tuning recommendations. Extra args pass through
        # (e.g. --fn try_variant, --json).
        if [[ ! -f "$PROV" ]]; then
            echo "No provenance log at $PROV (run the demo first)" >&2
            exit 1
        fi
        python3 "$HERE/analyze.py" --path "$PROV" "$@"
        ;;
    clear)
        # Wipe the provenance log so a fresh run starts from zero
        rm -f "$PROV"
        echo "Cleared $PROV"
        ;;
    help|*)
        cat <<EOF
Axon ASI demo CLI (Phase-10 surface, simulated)

  ./run.sh demos       # run the 6 key-free decision-pattern demos end to end (the tour)
  ./run.sh compile     # parse + type-check the .ax (future: axon ast review)
  ./run.sh run         # compile + execute (AXON_AI_MOCK=1 for key-free, or set ANTHROPIC_API_KEY)
  ./run.sh trace       # axon trace — per-fn score trajectory (add --fn NAME or --json)
  ./run.sh analyze     # score-trajectory analysis (plateau, recommendations)
  ./run.sh improve     # continue search — adds more evals to the same log
  ./run.sh redteam     # run the adversarial pass (currently part of main)
  ./run.sh replay      # GAP: deterministic replay (Phase 9)
  ./run.sh log         # dump full provenance log as NDJSON
  ./run.sh clear       # wipe provenance log

Provenance log: $PROV
Demo source:    $DEMO

Build the interpreter CLI first (fast, no LLVM):
  cargo build -p axon-core --no-default-features --bin axon
For live LLM calls instead of AXON_AI_MOCK, add: --features asi-runtime
EOF
        ;;
esac
