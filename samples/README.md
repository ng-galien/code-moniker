# Samples

This directory has two distinct sample corpora:

- `catalog/`: executable check scenarios used by the VSCode extension catalog
  and by integration tests. Each document contains rules, a file layout, and
  the violations that layout must produce.
- `learn/`: focused DSL learning documents used by `code-moniker rules learn`.
  These are documentation-oriented snippets, not full scenario fixtures.

- Format contract: [`docs/check-scenarios.md`](../docs/check-scenarios.md)
- Run one catalog sample: `code-moniker check . --scenario samples/catalog/<name>.cm.md`
- Validate all (CI gate): `cargo test -p code-moniker --test samples_contract`
- Regenerate expectations: `CM_SCENARIO_BLESS=1 cargo test -p code-moniker --test samples_contract`
