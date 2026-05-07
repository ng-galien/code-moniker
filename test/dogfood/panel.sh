#!/usr/bin/env bash
# Source-able panel of representative open-source projects, one per
# extractor language. Each entry pins a tag/commit so dogfooding stays
# reproducible across machines.
#
# Format (single string, fields separated by `|`):
#   lang | project_id | git_url | ref | src_subdir | manifest_path
#
# - lang          One of: rs, ts, java, sql. Drives the extract_<lang> dispatch.
# - project_id    Becomes the moniker project segment AND the keyed
#                 `project` column in the dogfood schema. Stable across
#                 runs; do not change without resetting the DB.
# - git_url       HTTPS clone URL, or the literal `self` to ingest the
#                 local pg_code_moniker repo (no clone needed).
# - ref           Tag, branch, or commit. Pinned. `HEAD` is reserved
#                 for `self` only.
# - src_subdir    Sub-path under the clone root that holds source files
#                 (avoid sweeping vendor/test fixture trees by default).
# - manifest_path Path to the build manifest from the clone root, or `-`
#                 if the project has none / it should be skipped.
#
# Bias: pick projects that are big enough to exercise the extractor on
# real symbol density (>~50 source files, real call/heritage/import
# graph), small enough to ingest in <1 minute. Avoid mega-monorepos
# until the extractor is benchmarked.

PCM_DOGFOOD_PANEL=(
	# Rust ----------------------------------------------------------------
	"rs|pg_code_moniker|self|HEAD|src|Cargo.toml"
	"rs|clap|https://github.com/clap-rs/clap.git|v4.5.20|clap_builder/src|clap_builder/Cargo.toml"
	"rs|bytes|https://github.com/tokio-rs/bytes.git|v1.7.2|src|Cargo.toml"

	# TypeScript ----------------------------------------------------------
	"ts|zod|https://github.com/colinhacks/zod.git|v3.23.8|src|package.json"
	"ts|date-fns|https://github.com/date-fns/date-fns.git|v3.6.0|src|package.json"

	# Java ----------------------------------------------------------------
	"java|gson|https://github.com/google/gson.git|gson-parent-2.11.0|gson/src/main/java|-"

	# SQL / PL-pgSQL ------------------------------------------------------
	# pgTAP is the canonical pure-pgSQL codebase: deep symbol density,
	# many overloads (`is(int,int,...)`, `is(text,text,...)`), real
	# call graph between assertion helpers.
	"sql|pgtap|https://github.com/theory/pgtap.git|v1.3.3|sql|-"
)
