# SQL / PL/pgSQL

The SQL/PL-pgSQL extractor does not use tree-sitter. It calls
PostgreSQL's own parser directly through `pgrx::pg_sys::pg_parse_query`
(declared by pgrx-pg-sys, resolved at runtime against the running
backend's libpostgres). No external Cargo dependency, no libpg_query
bundle. Source of truth: `docs/EXTRACTION_TARGETS.md` § SQL / PL/pgSQL.

`parse` and `extract` live in the same `mod.rs`, but `parse` returns
a `Vec<*mut pg_sys::RawStmt>` produced by the backend parser. PL/pgSQL
function bodies are walked through `plpgsql_compile_inline`
(dynamically resolved via `load_external_function`), then each
embedded SQL fragment is re-parsed with `raw_parser(query, parseMode)`
to expose the FuncCall tree.

## Module shape

A `.sql` file is the module. Strip `.sql`, split path on `/`, append as
typed `dir`/`module` segments under the anchor. No in-source package. Schema names
(`public.foo`, `esac.symbol`) are part of ref targets, not the module
moniker.

## Definitions

Required:

- module (the file)
- `CREATE FUNCTION` / `CREATE OR REPLACE FUNCTION` → `function` def with
  the **full parameter type signature** in the moniker segment
  (`function:bar(int4,text)`). Arity alone is not enough — PG's
  same-name same-arity overloads (`min(int)` vs `min(text)`) collide
  on arity-only monikers. Schema lives inside the moniker as a
  `/schema:NAME/` segment when the def is qualified in source:
  `.../module:foo/schema:public/function:bar(int4,text)` for
  `CREATE FUNCTION public.bar(a int, b text)`.
- `CREATE PROCEDURE` → `function` def (use the same kind label; procedures
  and functions are siblings in PG)
- trigger function definitions → `function` (the trigger itself is a
  binding, not a def — extract from `CREATE TRIGGER` only as a ref to the
  function)
- `CREATE TABLE` → `class` def. Columns may be emitted as `field` defs
  later; first parity does not require it unless ESAC currently emits them.
- `CREATE VIEW` / `CREATE MATERIALIZED VIEW` → `interface` def
- overload-disambiguated by full parameter types in the moniker
  segment (`function:bar(int4,text)`); arity alone collides for the
  common PG case `min(int)` / `min(text)` and is not acceptable

## References

- top-level function calls (`SELECT schema.function(...)`,
  `INSERT INTO ... SELECT fn(...)`, etc.) → `calls` ref. Raw_parser
  does not analyse argument types at a call site, so the target
  moniker carries **arity-only** (`function:bar(2)`) with explicit
  `confidence: unresolved`. Consumers project on name+arity to match
  defs whose moniker carries the full type signature. Emitted via
  `pg_sys::raw_expression_tree_walker_impl` over each top-level
  `RawStmt`; ViewStmt requires walking its `query` field directly
  because the tree walker rejects DDL statement shapes.
- function calls inside PL/pgSQL bodies → walk the procedural AST
  returned by `plpgsql_compile_inline`, then for every embedded
  `PLpgSQL_expr.query` re-parse with `raw_parser(query, parseMode)`
  and reuse the same call walker. Same arity-only / unresolved
  contract as top-level calls. Linux-only by current platform
  constraint: `plpgsql_compile_inline` is hidden as a local symbol on
  macOS plpgsql.dylib so the walker no-ops on macOS dev.
- pgTAP / test SQL into `esac.*` and other schema functions → same as
  above; the test files are normal modules, the calls are normal `calls`
  refs
- table/view references (`SELECT … FROM schema.table`,
  `INSERT INTO schema.table`) → `uses_type` against the
  table/view moniker. Only emit if ESAC currently does — else skip.

## Resolution metadata

- schema and function name split: encoded as typed moniker segments
  (`/schema:public/function:bar(int4,text)`), not as separate fields
- argument types: full list inside the moniker segment via
  `extend_callable_typed`, mirrored on `DefRecord.signature` for
  consumer projection. Types are normalised: `pg_catalog.<type>`
  prefix stripped (so `int4` rather than `pg_catalog.int4`), array
  suffix preserved (`name[]`). No display-name normalization
  (`int4` is what we emit; consumer maps to `integer` if it wants).
- robust error handling: `pg_parse_query` ereports on any malformed
  top-level statement and aborts the whole call. The walker wraps
  the call in `PgTryBuilder::catch_others` and falls back to a
  module-only graph in that case. Per-statement isolation
  (dollar-quote-aware splitting + per-fragment parse) is a follow-up
  — pgTAP fixtures with intentionally-broken statements currently
  produce only the module def.

## Non-targets

- dynamic SQL inside `EXECUTE format(...)` — the inner string is opaque;
  do not try to parse it
- full SQL semantic analysis across `search_path` and temporary schema
  state — that lives in the consumer
- DDL effects (table altering, column adding) — extract the statement as
  a def or skip it; do not model temporal mutation

## Why this extractor is shaped differently

PL/pgSQL has two layers: the SQL surface (DDL + statement-level
expressions, handled by `pg_parse_query` / `raw_parser`) and the
procedural body (BEGIN/END/IF/LOOP/PERFORM, parsed by plpgsql.so's
own parser via `plpgsql_compile_inline`). The native extension is
itself a PG extension: it dogfoods the running backend's parser
rather than carrying a separate copy. A tree-sitter grammar would
have to re-implement both layers and diverge from PG's own
semantics; calling `pg_sys::pg_parse_query` keeps the extractor
honest with the database it serves and adds zero crate dependencies.

## Determinism and partial parses

`pg_parse_query` and `plpgsql_compile_inline` are deterministic for
the same input + catalog state. `pg_parse_query` is purely
syntactic (no catalog reads). `plpgsql_compile_inline` does perform
catalog lookups (type resolution, RECORD/%ROWTYPE), so phase 2
violates the strict "no table reads" contract — the pragmatic
compromise is "deterministic given the catalog state at extraction
time", validated end-to-end via pgTAP. Iteration order over
PL/pgSQL body statement lists is preserved (PG returns ordered
`List` vectors); never re-iterate via HashMap.

When a top-level statement fails to parse, the whole module
extraction currently falls back to module-only (the call to
`pg_parse_query` ereports and is caught by `PgTryBuilder`). A
follow-up will split on `;` with dollar-quote awareness so a single
broken statement stops shadowing the rest. ESAC's `gaps` operation
will eventually surface these as graph diagnostics once the shared
metadata channel exists; for now parse failures are silent.
