# Vendored PL/pgSQL parser

C sources from PostgreSQL's `src/pl/plpgsql/src/`, lightly adapted so
the `lang/sql` extractor can parse PL/pgSQL function bodies without
running inside a PostgreSQL backend.

Upstream: <https://github.com/postgres/postgres/tree/master/src/pl/plpgsql/src>

Licensed under the PostgreSQL License — see [`../../LICENSE-POSTGRESQL`](../../LICENSE-POSTGRESQL).

The single non-upstream file is `cmk_plpgsql_driver.c`, which provides
the entry point our `build.rs` compiles against. It is licensed under
the same MIT OR Apache-2.0 terms as the rest of the crate.
