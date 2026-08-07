# Vendored Code Moniker PL/pgSQL grammar

This directory contains the PL/pgSQL half of
[`tree-sitter-postgres`](https://github.com/gmr/tree-sitter-postgres), derived
from version `1.2.4` at commit
`9b27ba5c8700f9bf808221a0f6d17fe6515da787`.

This source is compiled directly by `code-moniker-core`; it is not an
independently versioned or published package. Code Moniker carries this focused
grammar because PostgreSQL labels are identifiers and may therefore be
delimited with double quotes. The upstream grammar used only unquoted
identifiers for opening labels and did not recognize quoted identifiers in
label references.

Code Moniker intentionally keeps the stable `tree-sitter-postgres` 1.x grammar
baseline. Do not rebase this vendor directory onto the PostgreSQL 19 prerelease
grammar as part of maintaining this fix; evaluate that upgrade separately once
the upstream release is stable.

The generated parser is kept in source control. After changing `grammar.js`,
regenerate it from this directory with:

```bash
npx --yes tree-sitter-cli@0.25.10 generate
```
