# CX-03 v0.23 Forge integration amendment — meta/build capability requests

Forge meta-programs, compiler plugins, migrations and BuildPrograms are untrusted requesters. They may declare capability requirements but cannot mint grants. Current principal/scope/budget/revocation state is checked by the existing CX-03 authority before every protected dispatch.

## v0.23 acceptance gates

**G03-r23-meta-no-mint:** A Forge meta/build/transform request cannot create or widen a capability grant; unknown, stale, revoked or over-budget authority refuses before side effects.

**G03-r23-forge-recheck:** Cached SIR, compiler results or resource projections do not bypass current capability recheck at dispatch or commit.

