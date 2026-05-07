# SQL / PL/pgSQL

ESAC's SQL extractor is **not** tree-sitter — it dispatches via `grammed` to
PostgreSQL's own `libpg_query` parser. The native extension target is to
preserve those semantics. Source of truth: `docs/EXTRACTION_TARGETS.md`
§ SQL / PL/pgSQL.

There is **no `tree-sitter-sql`** crate that captures PL/pgSQL semantics
faithfully. Use `libpg_query` (Cargo crate `pg_query` or equivalent FFI
binding) to parse. This is the one extractor where the language section
breaks from the tree-sitter shape — keep `parse` and `extract` in the same
`mod.rs`, but `parse` returns a `pg_query` parse tree, not a tree-sitter
`Tree`.

## Module shape

A `.sql` file is the module. Strip `.sql`, split path on `/`, append as
typed `dir`/`module` segments under the anchor. No in-source package. Schema names
(`public.foo`, `esac.symbol`) are part of ref targets, not the module
moniker.

## Definitions

Required:

- module (the file)
- `CREATE FUNCTION` / `CREATE OR REPLACE FUNCTION` → `function` def with
  arity (mandatory — PL/pgSQL supports overloads). Schema goes in the URI
  shape: `.../module:foo#schema:public#function:bar(int,text)` for
  `public.bar(int, text)`.
- `CREATE PROCEDURE` → `function` def (use the same kind label; procedures
  and functions are siblings in PG)
- trigger function definitions → `function` (the trigger itself is a
  binding, not a def — extract from `CREATE TRIGGER` only as a ref to the
  function)
- `CREATE TABLE` → `class` def. Columns may be emitted as `field` defs
  later; first parity does not require it unless ESAC currently emits them.
- `CREATE VIEW` / `CREATE MATERIALIZED VIEW` → `interface` def
- overload-disambiguated functions: arity (and ideally argument types in a
  later signature field) is mandatory in the moniker

## References

- top-level function calls with `schema.function(...)` → `calls` with
  resolved moniker such as
  `.../module:foo#schema:public#function:bar(int,text)`.
- unqualified function calls at top level → `calls` with name-only target
  (resolution happens in SQL via search_path semantics on the consumer side)
- function calls inside PL/pgSQL bodies → walk the function body's parsed
  AST and emit `calls` for each invocation. `libpg_query` exposes the body
  expression tree; do not parse the body string yourself.
- pgTAP / test SQL into `esac.*` and other schema functions → same as
  above; the test files are normal modules, the calls are normal `calls`
  refs
- table/view references (`SELECT … FROM schema.table`,
  `INSERT INTO schema.table`) → `uses_type` against the
  table/view moniker. Only emit if ESAC currently does — else skip.

## Resolution metadata

- schema and function name split: encoded as typed moniker segments
  (`#schema:public#function:bar(int,text)`), not as separate fields
- argument count: in the moniker via `MonikerBuilder::method`. Argument
  types as text are nice-to-have for full overload disambiguation; emit
  them on `RefRecord` when the field exists, else leave a TODO.
- robust error handling: `libpg_query` returns errors per top-level
  statement. **Never** abort the whole module on a single broken
  statement: emit the module def, skip the broken statement, continue.
  This is the only extractor where partial source is the norm — pgTAP
  fixtures intentionally include malformed statements.

## Non-targets

- dynamic SQL inside `EXECUTE format(...)` — the inner string is opaque;
  do not try to parse it
- full SQL semantic analysis across `search_path` and temporary schema
  state — that lives in the consumer
- DDL effects (table altering, column adding) — extract the statement as
  a def or skip it; do not model temporal mutation

## Why this extractor is shaped differently

PL/pgSQL has two layers: the SQL surface (parsable by `libpg_query` as PG
sees it) and the procedural body (parsed by PG's PL/pgSQL parser into a
separate AST inside `CREATE FUNCTION` bodies). `libpg_query` exposes both.
A tree-sitter grammar would have to re-implement both layers — and would
diverge from PG's own parser. Using `libpg_query` keeps the extractor
honest with the database it serves.

## Determinism and partial parses

`libpg_query` parse output is deterministic for the same input. The risk
is **iteration order over PL/pgSQL body statement lists** — these come
back as ordered vectors, so iterate them in given order, never via
HashMap. Same applies to argument list iteration in calls.

When a top-level statement fails to parse, record a graph diagnostic under
the module root with the byte range of the failing statement and the
parser's error message in metadata. Do not invent a `parse_error` ref kind
unless the shared ref vocabulary is explicitly extended. ESAC's `gaps`
operation should surface these diagnostics; without an explicit diagnostic
record, parse failures become invisible.
