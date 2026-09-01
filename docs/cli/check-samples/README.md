# Moved: executable samples

The executable catalog samples now live in [`samples/catalog/`](../../../samples/catalog/)
as scenario documents — rules, a demo file layout, and CI-verified expected
violations in one Markdown file (format:
[`docs/check-scenarios.md`](../../check-scenarios.md)).

- Browse them: [`samples/README.md`](../../../samples/README.md)
- Replay one: `code-moniker check . --scenario samples/catalog/<name>.cm.md`

`code-moniker rules learn` prints a progressive Markdown summary backed by the
focused material in [`samples/learn/`](../../../samples/learn/) and the
catalog. `rules learn <topic>` opens one level and prints the canonical focused
document; `rules learn --format json` returns the complete inventory.
