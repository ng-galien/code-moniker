---
name: pgrx-reinstall
description: Build, install the code_moniker extension into the pgrx-managed PG17 instance, and run the full pgTAP suite. Use after any change to src/pg/ or src/lang/ to validate that the SQL surface still behaves correctly. Reports only failures; quiet on success.
---

# pgrx-reinstall

Run these two commands in sequence from the project root:

```bash
cargo pgrx install --pg-config $HOME/.pgrx/17.9/pgrx-install/bin/pg_config
./test/run.sh
```

## Output rules

- **On install failure** : surface only the `error[...]` / `error:` lines and the linker output if any. Drop the `Building / Compiling / Finished` chatter.
- **On test failure** : surface the failing assertion lines (those starting with `not ok` or `# Failed`) plus the file/test that owns them. Drop the long `ok N - ...` stream.
- **On success** : one line summary — `<N> SQL entities installed, <M> pgTAP tests passed`. Don't repeat the per-test `ok` output.

## Common failure modes

- **Linker error on macOS** : check `.cargo/config.toml` exists with the `dynamic_lookup` rustflags.
- **`requires` cycle in `extension_sql!`** : pgrx complains that a `requires` target isn't found — re-check the Rust function name spellings and confirm those functions are `#[pg_extern]` / `#[pg_operator]`.
- **`type "moniker" does not exist` in pgTAP output** : known `ANALYZE` quirk on tables with `moniker` columns; the test should `SET LOCAL enable_seqscan = off` instead of `ANALYZE`. Tracked in TODO.md as part of the custom Datum chantier.
- **`operator does not exist: moniker = moniker[]`** : forgot `array_ops` works on the moniker btree opclass, but PG hasn't seen `=` registered for the type — check that `index.rs`'s `requires = [...]` lists `moniker_eq` first.
