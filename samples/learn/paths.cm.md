---
name: paths
title: Moniker path patterns and aliases
summary: Match architectural locations with moniker globs and reusable aliases.
---

# Moniker Path Patterns And Aliases

Path expressions match moniker segments, not raw filesystem strings. Use
`**` for descendants and segment regexes such as `dir:/^(app|domain)$/`.

```toml
[aliases]
domain = "source ~ '**/dir:domain/**'"
infra  = "target ~ '**/dir:infrastructure/**'"

[[refs.where]]
id      = "domain-no-infra"
rationale = "A path alias can express an architectural boundary in plain project language: domain code should not call infrastructure directly."
expr    = "$domain => NOT $infra"
message = "Domain code must not depend on infrastructure."
```

Common fields include `uri`, `moniker`, `source`, `target`, `source.parent`,
`target.parent`, `name`, `kind`, `shape`, `visibility`, and `lines`.
