# CX-17 v0.23 Forge integration amendment — local corpus and bootstrap packs

External repository/library knowledge used to teach an unfamiliar language/toolchain is revision-bound. Forge bootstrap packs are curated projections over repository source, installed libraries, conformance tests, canonical examples and versioned specs; they do not become new global memory authority.

## v0.23 acceptance gates

**G17-r23-bootstrap-source:** A bootstrap pack records compiler/language revision and authoritative local source roots; stale or mismatched packs are not silently reused as current language truth.

