//! `libaxon_rt_ai.a` — the native runtime for Axon programs that call an AI
//! builtin (`ai_complete`, `ai_extract_*`).
//!
//! It contains no code of its own. A staticlib bundles every crate it links, so
//! naming both runtimes here yields one archive holding the `__axon_*` symbols
//! of `axon-rt` and the `__axon_ai_*` symbols of `axon-ai` over a SINGLE copy
//! of `std`. `extern crate` (rather than a `use`) is what forces each crate into
//! the link: nothing here references their items, and an unreferenced
//! dependency would otherwise be dropped.

extern crate axon_ai;
extern crate axon_rt;
