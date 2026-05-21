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
| TypeScript / JavaScript | [typescript.toml](typescript.toml) |
| Rust | [rust.toml](rust.toml) |
| Java | [java.toml](java.toml) |
| Python | [python.toml](python.toml) |
| Go | [go.toml](go.toml) |
| C# | [csharp.toml](csharp.toml) |
| SQL / PL/pgSQL | [sql.toml](sql.toml) |

Use `code-moniker langs <tag>` to inspect the exact kind and visibility
vocabulary for a language.
