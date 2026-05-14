# Dogfood corpus

Representative open-source projects ingested through the extension's
`extract_<lang>()` SQL functions. Used to validate extractor coverage at
real-world scale and to spot regressions that fixture-sized tests miss.

## Run

```sh
scripts/dogfood/run.sh ingest                    # full panel
scripts/dogfood/run.sh ingest --lang ts          # only TypeScript entries
scripts/dogfood/run.sh ingest --only zod         # one project
scripts/dogfood/run.sh ingest --reset            # discard caches and re-clone
```

Cloned repositories land under `<repo_root>/dogfood/<lang>/<project>/`
and are gitignored. Re-running without `--reset` reuses existing clones.

## Regression floors

After ingest, `scripts/dogfood/run.sh check` asserts that every
`(project, kind)` count is at least the floor recorded in
`scripts/dogfood/baselines.tsv`. The floors are 5% below the snapshot
in the file — small fluctuations from tree-sitter grammar updates or
panel pin refreshes pass; a real regression where the extractor stops
emitting a class of defs/refs fails loudly.

Workflow when extractor behavior changes legitimately (a fix that
emits more defs, a new kind, etc.):

```sh
scripts/dogfood/run.sh ingest             # re-ingest
scripts/dogfood/run.sh check              # may fail on the new code path
scripts/dogfood/run.sh baseline           # regenerate the floors
git diff scripts/dogfood/baselines.tsv    # audit the change
git add scripts/dogfood/baselines.tsv && git commit
```

Tolerance is configurable: `FLOOR_RATIO=0.90 scripts/dogfood/run.sh baseline`
loosens to ±10%, `FLOOR_RATIO=1.0` matches exact counts.

## Schema

Single DB `pcm_dogfood` with two tables, both project-keyed so a query
can isolate one project (`WHERE project = 'zod'`) or compare across
them.

```sql
module(project text, lang text, source_uri text, graph code_graph)
package(project text, name text, version text, dep_kind text, import_root text)
```

GIN indexes on `graph_def_monikers(graph)` and `graph_ref_targets(graph)`
support the same membership queries used in pgTAP.

## Panel

See `scripts/dogfood/panel.sh` for the canonical list. Selection bias:

- mid-size enough to exercise real call/heritage/import graphs (>50
  source files, real-world conventions);
- small enough to ingest in under a minute on a developer machine;
- pinned to a tag/commit for reproducibility;
- diverse codebase styles per language so a regression on one project
  doesn't pass unnoticed everywhere else.

Pinned today:

| lang | project          | size hint            |
| ---- | ---------------- | -------------------- |
| rs   | code-moniker     | self (~50 files)     |
| rs   | clap (builder)   | popular CLI lib      |
| rs   | tokio bytes      | small but type-rich  |
| ts   | zod              | declarative schemas  |
| ts   | date-fns         | many small modules   |

To extend the panel: add a row to `panel.sh`. Ingest is per-language so
adding a new entry won't disturb the others.

## Spot-check queries

After ingest, useful one-liners (from `psql -d pcm_dogfood`):

```sql
-- defs/refs density per project
SELECT project, count(*) AS files,
       sum(array_length(graph_def_monikers(graph), 1)) AS defs,
       sum(array_length(graph_ref_targets(graph), 1)) AS refs
FROM module GROUP BY project ORDER BY refs DESC;

-- ref-kind histogram per project (uses graph_refs accessor)
SELECT project, kind, count(*)
FROM module m, graph_refs(m.graph)
GROUP BY project, kind ORDER BY project, count(*) DESC;

-- which packages are imported but missing from the manifest?
SELECT DISTINCT m.project, external_pkg_root(t) AS pkg
FROM module m, unnest(graph_ref_targets(m.graph)) t
WHERE external_pkg_root(t) IS NOT NULL
  AND NOT EXISTS (SELECT 1 FROM package p
                  WHERE p.project = m.project
                    AND p.import_root = external_pkg_root(t));
```
