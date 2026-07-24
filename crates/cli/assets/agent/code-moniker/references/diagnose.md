# Diagnose — architecture and change risk through MCP

Use counted Code Moniker facts, not manual file sampling. Keep
`budget:"small"`, compact rendering and a narrow scope until evidence requires
expansion.

## Coupling map

`identity.graph` projects one symbolic level as nodes, weighted relation edges
and boundary ports. Reach it through the advanced MCP entry:

```text
code_moniker_query query:'identity.graph prefix:"lang:ts/dir:apps" limit:20'
```

Start near the requested area rather than at the workspace root. Heavy edges,
bidirectional pairs and hub nodes are factual refactor signals; `external`,
`manifest_blocked` and genuinely `unresolved` references remain separate.

For one unit, prefer `code_moniker_graph`. Filter by `direction`, `relation`
and `min_count` so the response carries only relevant boundary crossings.

## Smells and architecture rules

Use `code_moniker_rules action:"list" profile:"smells" limit:20` to inspect
the compiled contract and rationale. Use `action:"run"`, the project profile,
a touched `file` scope and a bounded `limit` to evaluate it. Start new rules at
warning severity, review their signal on the project corpus, then promote them
only after cleanup.

`rules.applicable` is available through `code_moniker_query` and is already
included in `code_moniker_context`. It distinguishes applicable, ignored and
potential rules with a reason instead of dumping the whole ruleset.

## Resolution quality

Use `code_moniker_query query:'resolution.audit prefix:"<narrow prefix>" limit:20'`
for clustered causes and zones. Treat external dependencies as expected,
manifest-blocked references as policy facts, and only unresolved causes as
index coverage gaps.

## Refactor targets

Combine evidence rather than ranking by one metric:

- high weighted coupling from `identity.graph` or `code_moniker_graph`;
- consumers spread across several prefixes from `code_moniker_usages`;
- multiple independent rules failing in the same files;
- unresolved references concentrated in one scope;
- existing notes or worktree changes from `code_moniker_context`.

The MCP provides facts and coverage. Risk, priority and the refactor decision
remain the agent's interpretation.

## Review changes

Use `code_moniker_diff` for HEAD-to-worktree symbol facts: moves, renames, body
or signature changes, retargeted references and residual hunks. Then run the
canonical scoped checks returned by `code_moniker_context`. Non-analyzable
files are explicit coverage limits, not silent omissions.

## Stop conditions

Stop when the current facts answer the question and their coverage is adequate.
Do not request another page, broader prefix, source code, `compact:false` or a
larger budget merely because it exists. Never replay the same diagnosis by
direct daemon query or shell command.
