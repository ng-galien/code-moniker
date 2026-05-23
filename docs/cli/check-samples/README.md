# Check DSL Samples

Copy one of these files into `.code-moniker.toml`, then trim it to the
rules you actually want to enforce.

Each sample is intentionally commented:

- `default_rules = false` means the file is a complete rule pack.
- `[aliases]` gives names to recurring moniker predicates.
- `[[<lang>.<kind>.where]]` rules run on defs.
- `[[refs.where]]` and `[[<lang>.refs.where]]` rules run on refs.

Available samples:

| Language | Sample |
| -------- | ------ |
| Architecture patterns | [architecture.toml](architecture.toml) |
| Test guardrails | [test-guardrails.toml](test-guardrails.toml) |
| TypeScript / JavaScript | [typescript.toml](typescript.toml) |
| Rust | [rust.toml](rust.toml) |
| Java | [java.toml](java.toml) |
| Python | [python.toml](python.toml) |
| Go | [go.toml](go.toml) |
| C# | [csharp.toml](csharp.toml) |
| SQL / PL/pgSQL | [sql.toml](sql.toml) |
| Local code smell warnings | [code-smells-local.toml](code-smells-local.toml) |

Literature-inspired samples — community-authored encodings of structural
rules from canonical software engineering literature. Attribution sits at
the top of each file; the authors did not endorse these encodings.

| Source | Sample |
| ------ | ------ |
| Robert C. Martin, *Clean Architecture* (2017) | [clean-architecture.toml](clean-architecture.toml) |
| Martin Fowler, *Patterns of Enterprise Application Architecture* (2002) | [fowler-eaa.toml](fowler-eaa.toml) |
| Martin Fowler, *Refactoring* (1999/2018) | [fowler-refactoring.toml](fowler-refactoring.toml) |

Use `code-moniker langs <tag>` to inspect the exact kind and visibility
vocabulary for a language. The Java sample includes Spring AOP proxy
self-invocation checks; rationale and source links live in
[check.md](../check.md#spring-proxy-self-invocation).
