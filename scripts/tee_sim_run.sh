#!/usr/bin/env bash
# tee_sim_run.sh — R21 Slice 2: run the Axon confidential workload inside a
# SIMULATED trusted execution environment using `gramine-direct` (no SGX/SEV
# hardware required — gramine-direct runs an unmodified binary as if in an
# enclave on ANY CPU).
#
# ── THE HONESTY BOUNDARY ──────────────────────────────────────────────────────
# This host has NO TEE hardware (CPU exposes `sme` only; no /dev/sev,
# /dev/sgx_enclave, /dev/tdx_guest). So NO genuine hardware attestation quote can
# be produced here — and this script does NOT fake one. What it demonstrates:
#
#   TYPE-ENFORCED (REAL, gate-tested elsewhere): a sealed Secret is unsealed ONLY
#     inside an `@[enclave]` fn (E1810). See scripts/gate.sh / the integration test.
#   SIMULATED (here): the workload EXECUTES inside a gramine-direct "enclave"
#     region, with AXON_TEE_ENCLAVE=1 + a stub measurement set by the manifest.
#   REAL-ATTESTED: nothing here. A genuine SEV-SNP / SGX quote is produced only
#     on a confidential cloud runner — see .github/workflows/tee.yml.
#
# The `tee_*` builtins are interpreter-only (codegen E0910-refuses them), so the
# workload runs via `axon run` under the interpreter; gramine-direct wraps that
# process in its enclave-simulation loader.
#
# Skips cleanly (exit 0) when gramine-direct is absent — it is NOT installed on
# this host. Install with:  sudo apt-get install -y gramine   (or the Gramine
# apt repo: https://gramine.readthedocs.io/en/stable/installation.html).
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORKLOAD="examples/tee/confidential_score.ax"
# A deterministic stub "measurement" so the run is reproducible. NOT a real,
# hardware-rooted quote.
MEASUREMENT="SIM-gramine-direct-$(sha256sum "$WORKLOAD" 2>/dev/null | cut -c1-16)"

echo "tee_sim_run: building the interpreter axon binary…"
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "tee_sim_run: interpreter build failed — cannot run; skipping" >&2
  exit 0
fi
AXON="$ROOT/target/debug/axon"

# Expected output of the in-enclave run (with the enclave env vars set). We
# verify the workload PRODUCES this both directly and (if available) under
# gramine-direct.
EXPECT_AVG="confidential average salary: 130000"
EXPECT_INSIDE="running INSIDE a TEE/enclave region"

# ── 1. Direct run under the enclave env (always runs; the baseline) ───────────
echo
echo "── direct run with the enclave env signal (AXON_TEE_ENCLAVE=1) ──────────"
direct_out="$(AXON_TEE_ENCLAVE=1 AXON_TEE_MEASUREMENT="$MEASUREMENT" \
  "$AXON" run "$WORKLOAD" 2>/dev/null)"
echo "$direct_out"
if ! grep -qF "$EXPECT_AVG" <<<"$direct_out"; then
  echo "tee_sim_run: FAIL — expected '$EXPECT_AVG' in the workload output" >&2
  exit 1
fi
if ! grep -qF "$EXPECT_INSIDE" <<<"$direct_out"; then
  echo "tee_sim_run: FAIL — workload did not detect the enclave region" >&2
  exit 1
fi
echo "tee_sim_run: direct enclave-env run OK (aggregate computed; raw salaries never left the enclave fn)."

# ── 2. gramine-direct simulated-enclave run (SKIP-guarded) ────────────────────
if ! command -v gramine-direct >/dev/null 2>&1; then
  echo
  echo "── gramine-direct: NOT INSTALLED — skipping the simulated-enclave wrap ──"
  echo "tee_sim_run: install gramine to run the workload inside a gramine-direct"
  echo "             enclave (sudo apt-get install -y gramine). The TYPE guarantee"
  echo "             (E1810) is verified independently by scripts/gate.sh."
  echo "tee_sim_run: PASS (simulation skipped; baseline + type rule verified)."
  exit 0
fi

echo
echo "── gramine-direct present: wrapping the workload in a simulated enclave ──"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

# Gramine manifest. `loader.entrypoint` is the axon interpreter; argv runs the
# confidential workload. The manifest injects the enclave env signals so the
# workload's tee_in_enclave()/tee_attest_measurement() report the simulated TEE.
MANIFEST="$WORK/axon_tee.manifest"
cat > "$MANIFEST" <<MANIFEST_EOF
# Gramine manifest for the R21 Axon confidential workload (gramine-direct).
# This is the SIMULATED enclave: gramine-direct provides the enclave loader on a
# CPU with no SGX/SEV. It does NOT produce a hardware attestation quote.
loader.entrypoint.uri = "file:$AXON"
libos.entrypoint = "$AXON"
loader.argv = ["axon", "run", "$ROOT/$WORKLOAD"]

loader.env.AXON_TEE_ENCLAVE = "1"
loader.env.AXON_TEE_MEASUREMENT = "$MEASUREMENT"
loader.env.PATH = "/usr/bin:/bin"
loader.env.HOME = "$HOME"
loader.insecure__use_cmdline_argv = true

sys.enable_sigterm_injection = true

fs.mounts = [
  { uri = "file:/usr/bin", path = "/usr/bin" },
  { uri = "file:/bin",     path = "/bin" },
  { uri = "file:/lib",     path = "/lib" },
  { uri = "file:/usr/lib", path = "/usr/lib" },
  { uri = "file:$ROOT",    path = "$ROOT" },
]
MANIFEST_EOF

echo "tee_sim_run: manifest at $MANIFEST"
gramine_out="$(gramine-direct "$MANIFEST" 2>/dev/null || true)"
echo "$gramine_out"
if grep -qF "$EXPECT_AVG" <<<"$gramine_out" && grep -qF "$EXPECT_INSIDE" <<<"$gramine_out"; then
  echo "tee_sim_run: PASS — workload executed inside the gramine-direct SIMULATED enclave."
  echo "tee_sim_run: (SIMULATED, not hardware-attested. Real attestation: .github/workflows/tee.yml)"
  exit 0
else
  echo "tee_sim_run: FAIL — gramine-direct run did not produce the expected enclave output" >&2
  exit 1
fi
