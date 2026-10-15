# Flight plan: gaps (G1-G5)

Mission: close the five tracked defects so an approval binds every file that runs, tokens/records cannot be forged without a key, a kill stops the whole job, LSP locations name the right file, and parity harnesses cannot pass on a build failure.

Decisions (engineering, canon):
- Closure digest source → loader-recorded modules, not the separate resolver → cannot disagree with what ran.
- Digest content → name + axh1 hash + sealed bit, sorted by name; no paths → machine-independent (r22 byte-identity).
- `/1` `.approved` records → entry hash still binds (exit 8 on mismatch); match = approved:false, non-blocking → no regression, no downgrade.
- Run-time binding → `AXON_REQUIRE_IMPORTS_DIGEST` checked in the loading process, removed from env after → no check/use gap, no nested-run refusal.
- Native → refused at build time and by an axon-rt REFUSED row → house rule for interp-only controls.
- Approver env for intent→os → child's env shape via one axon-os helper → hermetic jobs with imports keep running.
- ⚠ close call: Fabric route → migrate tokens to /3; pass the digest to its executor or refuse import-bearing programs, whichever needs no new protocol.
- ast review origin → module name (not path), schema stays /2 → additive field, no absolute paths to web clients.
- Key → dedicated `AXON_APPROVAL_KEY`, derived subkeys, no ephemeral fallback → reusing axon-vm's key would silently flip behaviour.
- Token → `axon-approval/3`; keyed `axmac1`, unkeyed `axtok3`; verifier's key decides → no downgrade.
- ⚠ close call: `/1` tokens → refused under a key; keyless accepted only for import-free programs → nothing working today breaks; unbound imports never pass.
- Records → `approval` sealed; `axrec3` unkeyed / `axrmac1` keyed; `axrec2` verifies keyless only → keyed replay of old runs refuses (documented opt-in).
- Signals → forward to registered job groups, write killed record, exit 128+sig; keep ignored signals.
- LSP → foreign-file diagnostics on the `use` line with relatedInformation → no stale foreign-URI diagnostics.
- Harness skip → only proven-missing prerequisites; parity_all strict inline at gate.sh:996; floor computed.
- Lanes run `gate.sh --fast` + crate tests; full gate at integrator after each wave → avoid 5 concurrent full gates.

needs-human: NH1 entry-file-directory module search (1 item, no subtree).
[REVISED] markers: 19 review findings folded (4 blockers: /1 records, unkeyed /3 prefix, Fabric route, record prefixes).
Critical path: G1a; gate = examples cross-check (resolver set = loader set, digest stable across processes/cwds) + empty-closure known value.
Shape: 21 tasks, 2 waves, longest chain T0→G1a→G1d→F1→review (5), 5 parallel tracks in wave 1; TEST_CMD gate.sh (+strict parity_all); budget unbounded; baseline: see tasks/gaps-baseline.md.
First tasks: G1a (lane core), G2d (lane cancel), G3a (lane lsp), G4c (lane parity), G5 key module (lane record).
