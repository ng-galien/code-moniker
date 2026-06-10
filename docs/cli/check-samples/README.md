# Moved: executable samples

The check rule samples now live in [`samples/`](../../../samples/) as
executable scenario documents — rules, a demo file layout, and CI-verified
expected violations in one Markdown file (format:
[`docs/check-scenarios.md`](../../check-scenarios.md)).

- Browse them: [`samples/README.md`](../../../samples/README.md)
- Print one as TOML: `code-moniker rules learn <name>`
- Replay one: `code-moniker check . --scenario samples/<name>.md`
