# C1 — the package's "existing Axon" claims, verified by execution

`EXISTING_AXON_MAP.md` states its own basis is "documented, not rerun" and that
repository paths "must be confirmed before editing". Its basis is
`axon-docs.pdf` dated 2026-09-17, which predates ~115 commits on this branch.
This file is the rerun. Every row below was executed against the live binary on
2026-09-19, not read from the attachment.

| Package claim | Verdict | Evidence (executed) |
|---|---|---|
| `AxonHost` journal is a reusable observation seam | **CONFIRMED** | `AXON_RECORD=… axon run` exit 0, journal written; `AXON_REPLAY=…` exit 0 reproduces |
| Journals hold secrets; isolate before training export | **CONFIRMED** | `axon replay` prints "values redacted (sizes + digests shown); pass `--show-values`" — redacted BY DEFAULT |
| Replay divergence is detectable | **CONFIRMED** | replaying a different program against the journal exits **11** |
| `axon check` works as an independent out-of-band verifier | **CONFIRMED** | broken program exit 2, clean program exit 0 |
| `axon test` runs `@[test]` fns as completion tests | **CONFIRMED** | mixed suite reports "1 passed, 1 failed" |
| Empty sandbox scope is "unscoped, not deny-all" (p.48) | **WAS TRUE, NOW FIXED** | was the live behaviour; changed 2026-09-19 — `""` denies, `"*"` is unrestricted. The package's warning is what prompted it. |
| Approval should be bound to a content digest | **ALREADY DONE** | `axsha256:` digest binding in `main.rs`; tamper after approval is refused |
| Ambient effect ceiling is interpreter-only | **CONFIRMED** | `AXON_ALLOWED_EFFECTS=Pure` → interp exit 8, native binary exit 0 |
| Closures/channels do not persist across R44 cells | **CONFIRMED** | session reports `not_persisted` and the next cell raises E0001 |
| Audit identity ≠ authorization | **CONFIRMED** | `AXON_PRINCIPAL` documented and implemented as attribution only |

## Consequences for the build

1. The observer (C4) reuses the host journal rather than inventing a second
   observation path: it already redacts, already replays, and already detects
   divergence.
2. Independent verification (C9) shells out to `axon check` + `axon test`
   rather than re-implementing a checker inside Cortex — genuinely out-of-band
   from the actor, which is the property CX-01 asks for.
3. The grant catalog (C5) builds on `axon_os::grant::Grant` and
   `axon_os::profile::Profile`, not a third authority vocabulary.
4. Two of the package's open items are already closed. They are recorded here
   so no task "fixes" them again.
