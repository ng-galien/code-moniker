//! Per-language extractors. Pure Rust, no pgrx dependency.
//!
//! Each language has a submodule under `lang/`. The extractor for a
//! language consumes `(uri, source, anchor moniker, presets)` and
//! produces a `code_graph` value (Phase 3 onwards). The current modules
//! ship the parser shim — the moniker canonicalisation and ref walk
//! are added incrementally, test-first.
//!
//! For the MVP only TypeScript is wired in. Java, Python, and others
//! follow once the TS path is fully validated.

pub mod ts;
