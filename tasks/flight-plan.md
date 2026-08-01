# Flight plan

**Spec:** `governance/reviews/2026-07-31-deep-review.md` (185 code findings)
**Branch:** `governance-audit-2026-07-18` · **baseline:** `20eb218`

## What I'm building

Not the 185 findings. Triage of the 20 CRITICALs confirmed **3**, so the severity
column cannot be trusted to plan from. This run implements the verified cluster —
**capability-sandbox escape** — plus three mechanically-certain fixes, then stops.

The cluster matters because it is the product: README, flagship demo and the
CVE-Bench claim all rest on `@[contained]` binding. Today it does not.

- **T1** `sandbox_run` intersects instead of replacing the ceiling → fixes 2 of 3 CRITICALs
- **T2** capability walker follows string-named dispatch → fixes the 3rd
- **T3** `fs:`/`net:` allowlists actually constrain paths and hosts (largest)
- **T4** axon-os `scan_effects` parses instead of substring-matching
- **T5** add the MIT `LICENSE` (manifest claims it, file absent)
- **T6** fix README test counts (claims 246; actual 987+)
- **T7** wire CI to the real gates — it currently never builds codegen

## Order

T1 first (restores the runtime floor). T2 and T4 then run in parallel — disjoint
files. T3 last and alone; it is the widest change. T5/T6/T7 are independent.

## Gate

Green = **no failing test other than `wasm_interp_matches_native_on_pure_compute`**,
the single failure at clean baseline. Every security task needs a test that
**fails before the fix** — the review's central finding is gates that assert
nothing, and I will not add another.

Smoke: `./flagship --ci` → exit 0, 4 sections.

## What I will not do

Implement untriaged findings. The 165 remaining are logged, not queued.

## Needs you (excluded, not forgotten)

1. **What gates `principal_root`?** It is ungated — anyone mints root, so
   attenuation is bypassable without any forgery. Found while *refuting* a
   finding. The fix is a product decision, not a bug fix.
2. **The "~28 of 40 CVE-Bench" README claim.** T3 shows the allowlists do not
   bind today, so the claim is unverified as written.
3. **O001 error precedence** — which error a user sees first on an unconfigured
   host. A UX contract, and the one test the gate is pinned to.

## Risk

T3 touches the effect representation and could ripple into codegen parity. If it
destabilises, T1/T2/T4 still stand alone and deliver the escape fixes — I'll land
those and report T3 as incomplete rather than force it.
