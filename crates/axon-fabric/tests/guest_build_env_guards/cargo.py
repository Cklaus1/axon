# Run after shape.py: one controlled cargo invocation against a stand-in toolchain whose
# `cargo config get` answers from a marker file, so the effective-configuration checks
# (before the step, and during it) can be driven without a real toolchain.
CC_LD = g.host_tools(["cc", "ld"])
STUB_CARGO = """#!/bin/sh
case "$*" in
  *"--format json"*) if [ -e mark ]; then echo '{"build":{"rustc-wrapper":"/tmp/w"}}'; else echo '{"target":{"wasm32-wasip1":{"rustflags":["-Zx"]}}}'; fi ;;
  *"--show-origin"*) ;;
  *) %s ;;
esac
exit 0
"""
STUB_RUSTC = """#!/bin/sh
echo "host: x86_64-unknown-linux-gnu"
"""
def step_rec(tag, build_branch="true", marked=False):
    b = S + "/cs-" + tag
    mkdir(b)
    for d in ("src", "cargo-home", "target"):
        mkdir(b + "/" + d)
    r = good_at(b)
    cargo, rustc = r["toolchain"]["cargo"], r["toolchain"]["rustc"]
    put(cargo, STUB_CARGO % build_branch, 0o755)
    put(rustc, STUB_RUSTC, 0o755)
    r["toolchain"]["cargo_sha256"] = g.sha256(cargo)
    r["toolchain"]["rustc_sha256"] = g.sha256(rustc)
    r["toolchain"]["host_tools"] = {n: {"sha256": t["sha256"]} for n, t in CC_LD.items()}
    r["measured"] = g.measure(r)
    r["effective_config"] = {"origins": [], "foreign": []}
    if marked:
        put(b + "/src/mark", "")
    p = b + "/rec.json"
    put(p, json.dumps(r))
    return p, r
name0, args0, rf0 = g.PROTECTED_BUILDS[0]
if set(CC_LD) == {"cc", "ld"}:
    p, r = step_rec("ok")
    chk("shape_problems a stand-in toolchain record is a controlled build's", g.shape_problems(r), "")
    chk("cargo_step control runs the one controlled invocation and returns cargo's exit code", run(g.cargo_step, p, args0, rf0), 0)
    chk("cargo_step control records the build with begin's configuration before and after",
        [b["name"] for b in json.load(open(p))["builds"]], [name0])
    p, r = step_rec("args")
    chk("cargo_step other arguments than the table's", run(g.cargo_step, p, args0 + ["--features", "x"], rf0), "is not a controlled guest build invocation")
    chk("cargo_step other RUSTFLAGS than the table's", run(g.cargo_step, p, args0, "-C linker=/tmp/x"), "is not a controlled guest build invocation")
    p, r = step_rec("meas")
    r["measured"]["bin"] = "0" * 64
    put(p, json.dumps(r))
    chk("cargo_step tools that changed since begin", run(g.cargo_step, p, args0, rf0), "the tools the build stands on changed since begin")
    p, r = step_rec("before", marked=True)
    chk("cargo_step a configuration that changed before the step", run(g.cargo_step, p, args0, rf0), "effective configuration changed since begin, before")
    p, r = step_rec("during", build_branch=': > mark')
    chk("cargo_step a configuration that changed during the step", run(g.cargo_step, p, args0, rf0), "effective configuration changed DURING")
chk("host_build an output directory that exists", run(g.host_build, S), "exists: the host build writes a new directory")
