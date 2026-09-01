# Samples

This directory has two distinct sample corpora:

- `catalog/`: executable check scenarios used by `code-moniker rules learn`,
  the VSCode extension catalog, and integration tests. Each document contains
  searchable Learn metadata, rules, a file layout, and the violations that
  layout must produce. `workspace-symbol.cm.md`,
  `workspace-group.cm.md`, and `workspace-path.cm.md` cover the full-index
  rule roots, including bounded transitive architecture checks.
- `learn/`: focused DSL learning documents used by the default
  `code-moniker rules learn` output.
  They are also executable scenario fixtures, but their purpose is to teach one
  concept at a time. `taxonomy.cm.md` covers project vocabulary and diagnostic
  interpretation. `fragments.cm.md` covers view URIs and namespaced rule ids
  (the in-memory runner mounts the effective merged rule, not a live fragment
  file).

- Format contract: [`docs/check-scenarios.md`](../docs/check-scenarios.md)
- Run one sample: `code-moniker check . --scenario samples/catalog/<name>.cm.md`
- Run one learn topic: `code-moniker check . --scenario samples/learn/<name>.cm.md`
- Start with the progressive CLI summary: `code-moniker rules learn`
- Discover every topic as structured data: `code-moniker rules learn --format json`
- Read one catalog recipe from the CLI: `code-moniker rules learn <name>`
- Follow framework and syntax children explicitly, for example `java` →
  `spring`, `typescript` → `tsx` → `react`, and `sql` → `plpgsql`.
- Validate all (CI gate): `cargo test -p code-moniker --test samples_contract`
- Regenerate expectations: `CM_SCENARIO_BLESS=1 cargo test -p code-moniker --test samples_contract`
