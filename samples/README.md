# Executable samples

Each `.md` document in this directory is an executable check scenario: rules,
a file layout, and the violations that layout must produce. They render as
regular Markdown and replay against the real scan pipeline.

- Format contract: [`docs/check-scenarios.md`](../docs/check-scenarios.md)
- Run one: `code-moniker check . --scenario samples/<name>.md`
- Validate all (CI gate): `cargo test -p code-moniker --test samples_contract`
- Regenerate expectations: `CM_SCENARIO_BLESS=1 cargo test -p code-moniker --test samples_contract`
