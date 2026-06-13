# Moved: executable samples

The executable catalog samples now live in [`samples/catalog/`](../../../samples/catalog/)
as scenario documents — rules, a demo file layout, and CI-verified expected
violations in one Markdown file (format:
[`docs/check-scenarios.md`](../../check-scenarios.md)).

- Browse them: [`samples/README.md`](../../../samples/README.md)
- Replay one: `code-moniker check . --scenario samples/catalog/<name>.cm.md`

`code-moniker rules learn <topic>` now prints focused DSL learning material
from [`samples/learn/`](../../../samples/learn/), not the full catalog scenarios.
