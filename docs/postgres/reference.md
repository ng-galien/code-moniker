# PostgreSQL extension — reference

Install, types, operators, accessors, constructors, extractors, indexes. For the schema + populate + query walkthrough, see the [usage guide](usage.md).

Everything below lives in the `code_moniker` schema. Pin `search_path = code_moniker, public` or qualify every call.

## Install

### Docker

```sh
docker build -t code-moniker:dev .
docker run --rm -e POSTGRES_PASSWORD=pgcm -p 5432:5432 \
    --name pgcm code-moniker:dev
docker exec -it pgcm psql -U postgres -c "CREATE EXTENSION code_moniker;"
```

Image lands on top of `postgres:17`. Override defaults with `--build-arg`: `PG_MAJOR=17`, `PGRX_VERSION=0.18.0`.

### From source (pgrx)

```sh
cargo install --locked cargo-pgrx
cargo pgrx init --pg17 download
cargo pgrx install --manifest-path crates/pg/Cargo.toml \
                   --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
```

Then `CREATE EXTENSION code_moniker;`. `cargo pgrx run pg17` drops into `psql` with the extension preloaded.

Compatibility: PG 14–17 (one major active at build time via `pg14`…`pg17` features), Rust 1.85+, pgrx 0.18+.

## Types

| Type              | Purpose                                                   |
| ----------------- | --------------------------------------------------------- |
| `moniker`         | content-addressed identity of a def or ref target         |
| `moniker_pattern` | glob over moniker path segments (for `~`)                 |
| `code_graph`      | per-module bundle of defs + refs                          |

## Operators

| Operator                          | Returns | Semantics                                  |
| --------------------------------- | ------- | ------------------------------------------ |
| `moniker = moniker`               | `bool`  | byte equality                              |
| `moniker < moniker` (and `<=`, `>`, `>=`) | `bool` | byte-lex order (btree-backed)      |
| `moniker <@ moniker`              | `bool`  | left is descendant of right (GiST)         |
| `moniker @> moniker`              | `bool`  | left is ancestor of right (GiST)           |
| `moniker ~ moniker_pattern`       | `bool`  | path-segment glob match (GiST)             |
| `bind_match(moniker, moniker)`    | `bool`  | asymmetric cross-file resolution (`?=`)    |
| `moniker \|\| (segment, kind)`    | `moniker` | append a child segment                   |
| `code_graph @> moniker`           | `bool`  | graph contains def for moniker (GIN)       |

## Accessors

`moniker`: `kind_of`, `project_of`, `lang_of`, `path_of`, `parent_of`, `depth`, `bare_callable_name`.

`code_graph`: `graph_root`, `graph_defs`, `graph_refs`, `graph_locate`, `graph_def_monikers`, `graph_ref_targets`, `graph_export_monikers`, `graph_import_targets`.

## Constructors

| Function                                          | Purpose                                |
| ------------------------------------------------- | -------------------------------------- |
| `graph_create(moniker, kind) → code_graph`        | empty graph rooted at moniker          |
| `graph_add_def(...)`, `graph_add_ref(...)`        | immutable append, returns a new graph  |
| `code_graph_declare(jsonb) → code_graph`          | build from a declarative JSON spec     |
| `code_graph_to_spec(code_graph) → jsonb`          | inverse of `declare` (lossy on non-canonical ref kinds) |

JSON Schema for `code_graph_declare`: [declare-schema](declare-schema.json).

## Extractors

`extract_typescript`, `extract_rust`, `extract_java`, `extract_python`, `extract_go`, `extract_csharp`, `extract_plpgsql`. Manifest parsers: `extract_cargo`, `extract_package_json`, `extract_pom_xml`, `extract_pyproject`, `extract_go_mod`, `extract_csproj`.

Signature:

```
extract_<lang>(uri text, source text, anchor moniker,
               deep boolean DEFAULT false)
  → code_graph
```

`deep => true` also emits `param:` and `local:` segments. `extract_typescript` takes one extra named argument `di_register_callees text[] DEFAULT ARRAY[]::text[]` listing factory-style callees that should emit `di_register` refs (NestJS providers, custom DI registries, …).

The bytes are identical to what the CLI's `--cache` writes (same `core::code_graph::encoding` module).

## Binary I/O

Both native types expose explicit casts to/from `bytea` and CBOR. The bytes are byte-identical across CLI cache, `bytea` casts, and the column Datum.

| Function | Purpose |
| --- | --- |
| `moniker_to_bytea(moniker) → bytea` / `moniker_from_bytea(bytea) → moniker` | raw varlena round-trip |
| `code_graph_to_bytea(code_graph) → bytea` / `code_graph_from_bytea(bytea) → code_graph` | versioned encoding (custom format) |
| `code_graph_to_cbor(code_graph) → bytea` / `code_graph_from_cbor(bytea) → code_graph` | RFC 8949 CBOR for polyglot ingestion |
| `moniker_to_cbor(moniker) → bytea` / `moniker_from_cbor(bytea) → moniker` | same, for monikers |

Use bytea for Rust↔Rust pipelines (CLI cache → PG import); CBOR for cross-language ingestion. See the [COPY BINARY walkthrough](usage.md#binary-io--bytea-round-trip--copy-binary) for a worked example.

## Configuration

| GUC | Default | Effect |
| --- | --- | --- |
| `code_moniker.scheme` | `code+moniker://` | URI scheme used by `moniker_in` / `moniker_out`. Stored moniker bytes are scheme-independent. |

```sql
ALTER DATABASE myapp SET code_moniker.scheme = 'myapp+moniker://';
```

## Indexes

- Custom GiST opclass for `moniker` (covers `=`, `bind_match`, `<@`, `@>`, `~`).
- Btree + hash opclasses for ordering and hash joins.
- GIN over `moniker[]` for `code_graph @> moniker`.

All functions are `IMMUTABLE STRICT PARALLEL SAFE` unless noted.
