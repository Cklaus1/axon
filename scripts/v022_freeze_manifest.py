#!/usr/bin/env python3
"""Bind a PSV candidate's evidence into one freeze manifest (operator
2026-09-28), so no classification can change after the certifying review and
still claim the same evidence. Emits axon-v022-psv-freeze/1.

Binds: Axon SHA, MiCode SHA, guest image digest; mutation-registry digest,
active-mutant count, retired-equivalent count; equivalence-record digest;
paired-disable evidence digest; PSV spec / protocol / gap-map / negative-matrix
hashes. Reads only; writes the manifest to the path given (or stdout)."""
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


def git(args, cwd):
    return subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True).stdout.strip()


def main():
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

    img = json.load(open(os.path.join(ROOT, "profiles/linux-microvm/manifest.json")))
    manifest = {
        "schema": "axon-v022-psv-freeze/1",
        "axon_sha": git(["rev-parse", "HEAD"], ROOT),
        "micode_sha": git(["rev-parse", "HEAD"], micode) if os.path.isdir(micode) else None,
        "guest_image": {"profile_manifest_sha256": sha_file("profiles/linux-microvm/manifest.json"),
                        "artifacts": {k: v["sha256"] for k, v in img["artifacts"].items()},
                        "source_revision": img["source"]["axon_git_rev_at_build"]},
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
