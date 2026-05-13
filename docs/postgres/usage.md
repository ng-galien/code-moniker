# PostgreSQL — usage

Reference (install, types, operators, accessors): see the [SQL reference](reference.md).

## Schema

The extension installs its types, operators, and functions in the
`code_moniker` schema. Either qualify every call (`code_moniker.moniker`,
`code_moniker.extract_typescript(...)`) or pin the search path once:

```sql
SET search_path = code_moniker, public;
-- or, per-session-persistent:
ALTER DATABASE mydb SET search_path = code_moniker, public;
```

The examples below assume the search path is set.

```sql
CREATE EXTENSION code_moniker;
SET search_path = code_moniker, public;

CREATE TABLE module (
    id          uuid PRIMARY KEY,
    graph       code_graph NOT NULL,
    source_text text,
    source_uri  text,
    origin      text NOT NULL  -- 'extracted' | 'declared' | 'external'
);

CREATE UNIQUE INDEX module_root_uniq ON module ((graph_root(graph)));
CREATE INDEX module_root_gist        ON module USING gist ((graph_root(graph)));

CREATE INDEX module_def_monikers_gin ON module USING gin (graph_def_monikers(graph));
CREATE INDEX module_ref_targets_gin  ON module USING gin (graph_ref_targets(graph));
```

A module's identity is `graph_root(graph)`. A row update is a value
replacement: the new `code_graph` overwrites the old, atomically.

## Populate from source

```sql
INSERT INTO module (id, graph, source_text, source_uri, origin) VALUES
    (gen_random_uuid(),
     extract_typescript(
         'src/util.ts',
         'export class Util { run() { return 1; } }',
         'code+moniker://app'::moniker
     ),
     'export class Util { run() { return 1; } }',
     'src/util.ts',
     'extracted');
```

| Function              | Grammar                                  | Manifest parser            |
|-----------------------|------------------------------------------|----------------------------|
| `extract_typescript`  | tree-sitter (TS/TSX/JS/JSX)              | `extract_package_json`     |
| `extract_rust`        | tree-sitter                              | `extract_cargo`            |
| `extract_java`        | tree-sitter                              | `extract_pom_xml`          |
| `extract_python`      | tree-sitter                              | `extract_pyproject`        |
| `extract_go`          | tree-sitter                              | `extract_go_mod`           |
| `extract_csharp`      | tree-sitter                              | `extract_csproj`           |
| `extract_plpgsql`     | tree-sitter                              | —                          |

Each takes `deep boolean DEFAULT false`; pass `deep => true` to
also emit parameters and local variables. `extract_typescript`
takes one extra named argument `di_register_callees text[] DEFAULT
ARRAY[]::text[]` to declare which factory-style calls emit
`di_register` refs.

## Query

### Find the module that defines a moniker

```sql
SELECT id FROM module
 WHERE graph_def_monikers(graph)
       @> ARRAY['code+moniker://app/lang:ts/dir:src/module:util/class:Util'::moniker];
```

### Iterate every def of a module

```sql
SELECT * FROM module m, graph_defs(m.graph) WHERE m.id = $1;
```

`graph_defs` returns rows of `(moniker, kind, visibility, signature,
binding, origin, start_byte int, end_byte int)`. `graph_refs`
returns `(source, target, kind, receiver_hint, alias, confidence,
binding, start_byte, end_byte)`.

### Subtree containment

```sql
SELECT id FROM module
 WHERE graph_root(graph) <@ 'code+moniker://app/srcset:main'::moniker;

SELECT id FROM module
 WHERE graph_root(graph) <@ 'code+moniker://app/srcset:main/lang:java'::moniker;
```

### Cross-file linkage with `?=` (`bind_match`)

The extractor knows an import's path but not the kind of the
imported symbol, so byte-strict `=` cannot match an `imports_symbol`
ref against the exporting `class:` / `function:` def. The `?=`
operator (`bind_match` function) compares everything except the
final segment's kind.

```sql
CREATE INDEX module_export_gin ON module USING gin (graph_export_monikers(graph));
CREATE INDEX module_import_gin ON module USING gin (graph_import_targets(graph));

SELECT m_imp.id AS importer, m_def.id AS exporter
FROM module m_imp, LATERAL graph_refs(m_imp.graph) r,
     module m_def, LATERAL graph_defs(m_def.graph) d
WHERE r.binding IN ('import', 'inject')
  AND d.binding IN ('export', 'inject')
  AND r.target ?= d.moniker;
```

`?=` is registered in the moniker GiST opclass; lookups are
O(log n) on the corpus.

## Flat linkage cache

A projection table is reconstructible from `module` rows at any time;
it is a cache, not the truth.

```sql
CREATE TABLE linkage (
    source_id      uuid       NOT NULL REFERENCES module(id) ON DELETE CASCADE,
    source_moniker moniker    NOT NULL,
    target_moniker moniker    NOT NULL,
    kind           text       NOT NULL,
    binding        text       NOT NULL,
    confidence     text       NOT NULL,
    start_byte     integer,
    end_byte       integer
);

CREATE INDEX linkage_target_gist ON linkage USING gist (target_moniker);
CREATE INDEX linkage_source     ON linkage (source_id);

INSERT INTO linkage (source_id, source_moniker, target_moniker, kind, binding, confidence, start_byte, end_byte)
SELECT m.id, r.source, r.target, r.kind, r.binding, r.confidence, r.start_byte, r.end_byte
FROM module m, LATERAL graph_refs(m.graph) AS r;
```

## Declarative graphs

`code_graph_declare(jsonb) → code_graph` builds a graph from a spec
instead of source. The defs it produces carry `origin = 'declared'`;
the moniker is identity, so `bind_match` resolves a declared symbol
against any later-extracted one with the same identity.

```sql
SELECT code_graph_declare($$ {
  "root": "code+moniker://app/srcset:main/lang:ts/module:billing",
  "lang": "ts",
  "symbols": [
    {"moniker": "code+moniker://app/srcset:main/lang:ts/module:billing/class:Charge",
     "kind": "class",
     "parent": "code+moniker://app/srcset:main/lang:ts/module:billing",
     "visibility": "public"}
  ],
  "edges": [
    {"from": "code+moniker://app/srcset:main/lang:ts/module:billing/class:Charge",
     "kind": "depends_on",
     "to":   "code+moniker://app/external_pkg:stripe/path:Charge"}
  ]
} $$::jsonb);
```

Spec schema: [declare-schema](declare-schema.json). Reverse
projection: `code_graph_to_spec(graph) → jsonb`.

## Binary I/O — `bytea` round-trip + COPY BINARY

Both native types expose explicit casts to/from `bytea`, backed by
the same encoding the CLI uses for its on-disk cache (see
`docs/cli/extract.md` `--cache`). The bytes are byte-identical
across the two surfaces.

```sql
-- export a graph as raw bytes
SELECT code_graph_to_bytea(graph) FROM modules WHERE id = 1;

-- ingest from raw bytes (validated; bad bytes throw)
INSERT INTO modules(uri, graph)
  VALUES ('src/foo.ts', code_graph_from_bytea(decode('...', 'hex')));

-- and for moniker too
SELECT moniker_from_bytea(moniker_to_bytea('code+moniker://app/lang:ts'::moniker));
```

Batch export / import via COPY BINARY using a `bytea` staging
column — same workflow as `pg_dump --format=binary`:

```sql
CREATE TEMP TABLE staging (id int, graph_bytes bytea);

-- export
INSERT INTO staging
  SELECT id, code_graph_to_bytea(graph) FROM modules;
COPY staging TO '/tmp/graphs.bin' WITH (FORMAT BINARY);

-- import (on another DB)
COPY staging FROM '/tmp/graphs.bin' WITH (FORMAT BINARY);
INSERT INTO modules(id, graph)
  SELECT id, code_graph_from_bytea(graph_bytes) FROM staging;
```

The detour through a `bytea` column exists because pgrx 0.18 does
not currently expose a hook to override its CBOR-based `typsend`/
`typreceive` generator. When that gap closes upstream, `COPY
BINARY` will work directly on the `code_graph` / `moniker` columns
without the staging step.

## Polyglot import via CBOR

`code_graph_to_cbor` / `code_graph_from_cbor` (and the matching
`moniker_*_cbor` pair) speak CBOR (RFC 8949). They are intended for
**polyglot interop** — a framework / library in any language
(Rust, Python, Node, …) can produce graphs via `serde_cbor` (or
any CBOR library) and import them through `COPY BINARY` of a
bytea staging column.

```sql
CREATE TEMP TABLE graph_import (id text, payload bytea);

-- ingest a PGCOPY-binary dump where `payload` is CBOR-encoded
COPY graph_import FROM '/tmp/graphs.cbor.copy' WITH (FORMAT BINARY);

INSERT INTO modules(id, graph)
SELECT id, code_graph_from_cbor(payload) FROM graph_import;
```

Trade-offs vs `bytea` (custom encoding):

| | `*_to_bytea` (custom) | `*_to_cbor` |
|---|---|---|
| Size | ~30 % smaller | RFC-8949 framing overhead |
| Speed | direct byte ops | serde traversal |
| Producer language | Rust crate required | any CBOR lib |
| Spec stability | versioned (`LAYOUT_VERSION`) | RFC 8949 (stable) |
| Debug tooling | none (opaque) | `cbor-diag`, etc. |

Rule of thumb: bytea for Rust→Rust pipelines (CLI cache → PG
import); CBOR for cross-language ingestion. The Datum stored in
the column is the SAME in both cases — these are import / export
encodings, not storage formats.

## Extension vs caller

| Extension                | Caller                                |
|--------------------------|---------------------------------------|
| `moniker`, `code_graph`  | tables, indexes, triggers, RLS        |
| `extract_<lang>(...)`    | srcset / project anchor strategy      |
| operators + GiST opclass | linkage projection, materialised views |
| `code_graph_declare`     | spec sourcing, validation pipeline    |
| `code_graph_to_spec`     | UI, history, coverage tables          |

## See also

- [SQL reference](reference.md) — install, types, operators, accessors, indexes.
- [Spec](../design/spec.md) — conceptual model.
- [Moniker URI](../design/moniker-uri.md) — URI grammar, `bind_match` semantics.
- [Extract](../cli/extract.md) — standalone CLI.
- [Agent harness](../cli/agent-harness.md) — same extractors with the rule engine.
