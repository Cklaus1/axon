#!/usr/bin/env python3
"""Bind a PSV candidate's evidence into one freeze manifest (operator
2026-09-28), so no classification can change after the certifying review and
still claim the same evidence. Emits axon-v022-psv-freeze/1.

Binds: Axon SHA, MiCode SHA, guest image digest; mutation-registry digest,
active-mutant count, retired-equivalent count; equivalence-record digest;
paired-disable evidence digest; PSV spec / protocol / gap-map / negative-matrix
hashes. Reads only; writes the manifest to the path given (or stdout).

REQUIREMENT (protocol amendment 44, operator decision E): a freeze runs from a
STANDALONE CLONE, never a linked worktree, and binds a guest image whose
manifest says axon_tree_dirty_at_build: false under the amendment-44 rule. Both
are enforced here: a root whose .git is not a real directory (a gitfile, i.e. a
linked worktree, or a symlink) is refused; so is a root whose repository is not
its own .git (a linked worktree's admin dir copied in as .git, whose commondir
names another repository: C9 round 3, A79), asked of the operator's git the
way axon-fabric's git_data::discover asks it; and so is a guest manifest that
is not clean with no reasons. The clean flag and its reasons are bound.

C9 round 4 (amendment 56): the guest manifest must also carry the record of the
controlled build environment its binaries were built in
(scripts/guest_build_env.py): exactly the constructed environment, fresh
CARGO_HOME and target dir, no effective cargo setting the build would use, and
artifact digests equal to the manifest's. The record's digest and `rustc -vV`
are bound.

Git is /usr/bin/git with the caller's environment dropped (GIT_DIR and the like
cannot steer which repository answers) and replace objects off."""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def sha_file(rel):
    p = os.path.join(ROOT, rel)
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def sha_str(s):
    return hashlib.sha256(s.encode()).hexdigest()


GIT = "/usr/bin/git"
# The caller's environment is dropped: GIT_DIR, GIT_COMMON_DIR, GIT_CONFIG_*
# and a git on PATH would otherwise choose what answers.
GIT_ENV = {"PATH": "/usr/bin:/bin", "LC_ALL": "C", "GIT_CONFIG_NOSYSTEM": "1",
           "GIT_CONFIG_GLOBAL": "/dev/null", "GIT_NO_REPLACE_OBJECTS": "1",
           "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1"}


def git(args, cwd):
    return subprocess.run([GIT, "--no-replace-objects", "-c", "safe.directory=*",
                           "-c", "core.fsmonitor=", "-C", cwd, *args],
                          env=GIT_ENV, capture_output=True, text=True).stdout.strip()


def not_standalone(root):
    """Why `root` is not a standalone clone (decision E), or None. The rule
    axon-fabric's git_data::discover applies: .git is a real directory, and the
    repository git acts on (its common dir) is that directory."""
    dotgit = os.path.join(root, ".git")
    if os.path.islink(dotgit) or not os.path.isdir(dotgit):
        return ".git is not a real directory"
    common = git(["rev-parse", "--path-format=absolute", "--git-common-dir"], root)
    if not common or os.path.realpath(common) != os.path.realpath(dotgit):
        return f"its repository is {common or 'unknown'} (a linked worktree's git dir)"
    return None


def main():
    # The freeze process itself compiles nothing; this refuses only a freeze
    # run from a shell that names a rustc wrapper (the development sccache),
    # as defence in depth. Whether the BYTES the freeze binds were built
    # through a wrapper is judged below, from the guest manifest's recorded
    # controlled build environment -- not from this process's variables.
    wrappers = [v for v in ("RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_RUSTC_WRAPPER",
                            "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER") if os.environ.get(v)]
    if wrappers:
        sys.exit(f"refused: {wrappers} set; a freeze is never made through a compiler wrapper")
    out = sys.argv[1] if len(sys.argv) > 1 else None
    micode = sys.argv[2] if len(sys.argv) > 2 else os.path.join(os.path.dirname(ROOT), "micode-v022-wt")
    spec = importlib.util.spec_from_file_location("mut", os.path.join(ROOT, "scripts/v022_g01_mutations.py"))
    mut = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mut)

    # A stable digest over the mutation registry's decision content: every row's
    # (id, file, old, new, test) plus the retirement sets. Any change to what a
    # mutation targets or how a row is classified changes this digest.
    reg = [[r[0], r[2], r[3], r[4], r[7]] for r in mut.MUTATIONS]
    registry_digest = sha_str(json.dumps(reg, sort_keys=True))
    equiv = {"EQUIV_RECORD": mut.EQUIV_RECORD, "STALE_REFACTORED": mut.STALE_REFACTORED,
             "LEGACY_EQUIV": sorted(mut.LEGACY_EQUIV)}
    equivalence_digest = sha_str(json.dumps(equiv, sort_keys=True))
    active = [r[0] for r in mut.MUTATIONS if r[0] not in mut.RETIRED]

    why = not_standalone(ROOT)
    if why:
        sys.exit(f"refused: {ROOT} is not a standalone clone ({why}): "
                 "a freeze runs from a standalone clone (amendment 44, decision E)")
    img = json.load(open(os.path.join(ROOT, "profiles/linux-microvm/manifest.json")))
    src = img.get("source", {})
    if src.get("axon_tree_dirty_at_build") is not False or src.get("axon_tree_dirty_reasons") != []:
        sys.exit("refused: the guest manifest is not clean with no reasons "
                 f"(axon_tree_dirty_at_build={src.get('axon_tree_dirty_at_build')!r}, "
                 f"reasons={src.get('axon_tree_dirty_reasons')!r}): rebuild the guest image in a "
                 "standalone clone with the operator allowlist installed (amendment 44)")
    # The guest's bytes were built in the controlled environment (C9 round 4):
    # scripts/guest_build_env.py dropped the caller's environment, used the
    # pinned toolchain, a fresh CARGO_HOME and target dir, and refused any
    # effective cargo config but the tree's own. A list of wrapper variables
    # checked here could not see an ancestor config, RUSTC, RUSTFLAGS, a linker
    # or a reused target dir; the record of the environment that built the
    # bytes can.
    sys.dont_write_bytecode = True
    gspec = importlib.util.spec_from_file_location("guest_build_env",
                                                   os.path.join(ROOT, "scripts/guest_build_env.py"))
    gbe = importlib.util.module_from_spec(gspec)
    gspec.loader.exec_module(gbe)
    benv = src.get("build_environment")
    why = gbe.shape_problems(benv) if benv is not None else "the manifest records no build environment"
    if why:
        sys.exit("refused: the guest image was not built in the controlled build environment "
                 f"(scripts/guest_build_env.py): {why}")
    built = (benv or {}).get("artifacts") or {}
    unbound = [n for n in ("axon", "axon-guest-init", "axon-psv-runner")
               if not built.get(n) or built.get(n) != (img.get("artifacts", {}).get(n) or {}).get("sha256")]
    if unbound:
        sys.exit(f"refused: the guest manifest's {unbound} are not the bytes its controlled build "
                 "produced (scripts/guest_build_env.py records each artifact's digest)")
    manifest = {
        "schema": "axon-v022-psv-freeze/1",
        "axon_sha": git(["rev-parse", "HEAD"], ROOT),
        "micode_sha": git(["rev-parse", "HEAD"], micode) if os.path.isdir(micode) else None,
        "guest_image": {"profile_manifest_sha256": sha_file("profiles/linux-microvm/manifest.json"),
                        "artifacts": {k: v["sha256"] for k, v in img["artifacts"].items()},
                        "source_revision": src["axon_git_rev_at_build"],
                        "axon_tree_dirty_at_build": src["axon_tree_dirty_at_build"],
                        "axon_tree_dirty_reasons": src["axon_tree_dirty_reasons"],
                        "build_environment_sha256": sha_str(json.dumps(benv, sort_keys=True)),
                        "rustc": benv["toolchain"]["rustc_vV"]},
        "mutation_registry_total": len(mut.MUTATIONS),
        "active_mutants": len(active),
        "retired_equivalent": len(mut.EQUIVALENT_DID),
        "retired_stale_refactored": len(mut.STALE_REFACTORED),
        "retired_legacy": len(mut.LEGACY_EQUIV),
        "mutation_registry_digest": registry_digest,
        "equivalence_record_digest": equivalence_digest,
        "paired_disable_digest": sha_file("governance/status/v022-psv-paired-disable.json"),
        "spec_hashes": {
            "protocol": sha_file("governance/specs/v022-psv-protocol.md"),
            "gap_map": sha_file("governance/specs/v022-psv-gap-map.md"),
            "negative_matrix": sha_file("governance/specs/v022-psv-negative-matrix.md"),
            "protected_suite_verdict": sha_file("governance/specs/v022-protected-suite-verdict.md"),
        },
    }
    text = json.dumps(manifest, indent=2) + "\n"
    if out:
        open(os.path.join(ROOT, out), "w").write(text)
        print(f"freeze manifest -> {out}  (axon {manifest['axon_sha'][:8]}, "
              f"active {manifest['active_mutants']}, equiv {manifest['retired_equivalent']})")
    else:
        print(text)


if __name__ == "__main__":
    main()
