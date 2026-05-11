# Use `code-moniker` as a PostgreSQL extension

The extension provides native types (`moniker`, `code_graph`),
an indexed algebra (`=`, `?=`, `<@`, `@>`, `||`, plus btree-order
`<`/`<=`/`>`/`>=`), and per-language extractors. It owns no
tables, no triggers, no I/O against external state. Persistence
is the caller's responsibility; the shape below is the
recommended one.

References: [`design/spec.md`](design/spec.md) (conceptual model + SQL
surface), [`design/moniker-uri.md`](design/moniker-uri.md) (URI grammar).

## Install

### Docker

```sh
docker build -t code-moniker:dev .
docker run --rm -e POSTGRES_PASSWORD=pgcm -p 5432:5432 \
    --name pgcm code-moniker:dev
docker exec -it pgcm psql -U postgres -c "CREATE EXTENSION code_moniker;"
```

The image lands the extension on top of `postgres:17`. Version pins:
`PG_MAJOR=17`, `PGRX_VERSION=0.18.0`; override either with `--build-arg`.

### From source

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
```

`cargo pgrx run pg17` drops into an interactive `psql` with the
extension loaded. See [`CONTRIBUTING.md`](../CONTRIBUTING.md) for the
test loop.

## Schema

```sql
CREATE EXTENSION code_moniker;

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

Spec schema: [`declare_schema.json`](declare_schema.json). Reverse
projection: `code_graph_to_spec(graph) → jsonb`.

## Extension vs caller

| Extension                | Caller                                |
|--------------------------|---------------------------------------|
| `moniker`, `code_graph`  | tables, indexes, triggers, RLS        |
| `extract_<lang>(...)`    | srcset / project anchor strategy      |
| operators + GiST opclass | linkage projection, materialised views |
| `code_graph_declare`     | spec sourcing, validation pipeline    |
| `code_graph_to_spec`     | UI, history, coverage tables          |

## See also

- [`design/spec.md`](design/spec.md) — conceptual model, full SQL surface.
- [`design/moniker-uri.md`](design/moniker-uri.md) — URI grammar, escaping, `bind_match` semantics.
- [`cli-extract.md`](cli-extract.md) — standalone CLI, no PG required.
- [`use-as-agent-harness.md`](use-as-agent-harness.md) — same extractors plus a rule engine.
