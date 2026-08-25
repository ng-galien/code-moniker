# On-demand syntax tree

Code Moniker can return a bounded Tree-sitter syntax tree directly from source
text, or for one indexed source file or symbol. Parsing happens only for the
request; the source and tree are not stored in the workspace index.

Supported language tags are `rs`, `ts`, `java`, `python`, `go`, `c`, `cs`,
`sql`, and standalone `plpgsql`. The request delegates parsing to the same
language SDK contract as semantic extraction. A language produces one
`ParsedDocument`; both the graph extractor and the AST renderer consume it.

`ParsedDocument` can include embedded-language trees. PostgreSQL functions
declared with `LANGUAGE plpgsql` or `LANGUAGE sql` therefore expose their body
below the host `dollar_quoted_string` instead of leaving it as an opaque leaf.
The injected tree root carries a language marker, for example `[plpgsql]`, and
all byte ranges and positions remain relative to the original `.sql` source.

## MCP client

For stateless parsing, pass source text and its language:

```text
code_moniker_read
  language:"plpgsql"
  source:"DECLARE total numeric; BEGIN total := 1; RETURN total; END;"
```

This executes `syntax.parse` before workspace snapshot loading. It requires no
file, moniker, refresh, or prior indexing and does not persist the source. An
optional `uri` is only a parser filename hint; use `snippet.tsx` when TSX
grammar selection matters.

To parse current indexed source instead, pass its URI:

```text
code_moniker_read
  uri:"crates/core/src/lang/mod.rs"
  ast:true
  max_depth:6
  max_nodes:20
```

`uri` accepts:

- a workspace-relative source path;
- an absolute source path, useful when multiple workspace roots contain the
  same relative path;
- a compact or canonical moniker returned by `code_moniker_symbols`;
- a symbol id.

A file path selects the whole file. A symbol moniker or symbol id selects the
smallest matching declaration.

### MCP request fields

| Field | Type | Default | Accepted values |
| --- | --- | --- | --- |
| `uri` | string | optional for direct text; required for indexed reads | parser filename hint, file path, moniker, or symbol id |
| `source` | string | none | direct source text; requires `language` and implies `syntax.parse` |
| `language` | string | none | parser tag for `source`; requires `source` |
| `ast` | boolean | `false` | required only when `uri` selects indexed source |
| `max_depth` | integer | `6` | `>= 0`; client-selected traversal limit |
| `max_nodes` | integer | profile cap | `>= 1`; capped to `20` for `small`, `80` for `medium`, and `500` for `full` |
| `named_only` | boolean | `true` | `false` includes punctuation and anonymous grammar nodes |
| `include_text` | boolean | `false` | attaches normalized source text to leaf nodes |
| `max_text_chars` | integer | `80` | `0..=1000`, used only with `include_text:true` |

Direct source is limited to 1 MiB. The normal MCP output contract still
applies: `compact:true` and `budget:"small"` are the defaults. The budget is a
structural volume profile; syntax-tree depth and node limits are applied before
the response is rendered.

Code Moniker keeps bounded defaults for interactive and agent use. The MCP
volume profile supplies the default `max_nodes` and caps an explicit larger
value before the query runs: `20` for `small`, `80` for `medium`, and `500` for
`full`. `max_depth` remains client-selected. The typed daemon query has no
additional universal node maximum; direct daemon clients remain responsible
for choosing a limit that fits their latency and memory constraints.

### MCP response

Indexed response:

```text
uri: syntax.tree
completeness: bounded
file: crates/core/src/lang/mod.rs
language: rs
focus: crates/core/src/lang/mod.rs
nodes: 20/1571 max_depth:3 parse_error:false
tree:
- source_file 1:0-470:0
  - mod_item 1:0-1:23
    - visibility_modifier 1:0-1:3
    - identifier 1:8-1:22
```

`completeness` is `full` when every node under the selected root was emitted,
and `bounded` when a depth or node limit omitted part of the tree.
`nodes` is `emitted_nodes/total_nodes`. Positions use one-based lines and
zero-based UTF-8 byte columns. Nodes can carry a language marker such as
`plpgsql`, plus `anonymous`, `error`, or `missing` flags. Leaf text appears
only when requested.

A direct parse has the same tree shape but starts with `uri: syntax.parse`;
`file` and `focus` contain the supplied parser hint or a generated name such as
`snippet.plpgsql`.

## TypeScript daemon client

`@code-moniker/client` exposes the same protocol through its typed generic
query method:

```ts
const parsed = await client.queryData(
	{
		op: "syntax_parse",
		language: "plpgsql",
		source: "BEGIN RETURN 1; END;",
		uri: null,
		max_depth: 6,
		max_nodes: 100,
		named_only: true,
		include_text: false,
		max_text_chars: 80,
	},
	"syntax_tree",
);

const tree = await client.queryData(
	{
		op: "syntax_tree",
		workspace: null,
		focus: "crates/core/src/lang/mod.rs",
		max_depth: 6,
		max_nodes: 100,
		named_only: true,
		include_text: false,
		max_text_chars: 80,
	},
	"syntax_tree",
);
```

The result is a generated `SyntaxTreeResult`:

```ts
interface SyntaxTreeResult {
	file: string;
	language: string;
	focus: string;
	focus_line_range?: [number, number] | null;
	root: SyntaxNodeDto;
	emitted_nodes: number;
	total_nodes: number;
	max_depth: number;
	truncated: boolean;
	has_error: boolean;
}

interface SyntaxNodeDto {
	kind: string;
	language?: string | null;
	named: boolean;
	error: boolean;
	missing: boolean;
	byte_range: [number, number];
	start: { line: number; column: number };
	end: { line: number; column: number };
	text?: string | null;
	children: SyntaxNodeDto[];
}
```

`language` is omitted for ordinary nodes and present at the root of an
embedded-language tree.

The TypeScript client currently exposes this through `queryData`; there is no
separate `client.syntax.tree()` convenience facade.

## Errors

The typed daemon response uses stable error codes:

| Code | Meaning |
| --- | --- |
| `source_not_found` | no indexed source matches the requested path |
| `source_ambiguous` | the relative path exists in more than one selected root |
| `symbol_not_found` | the moniker or symbol id does not resolve |
| `symbol_not_in_workspace` | the symbol is outside the selected workspace |
| `syntax_language_unsupported` | the indexed source has no supported parser |
| `syntax_source_too_large` | direct source exceeds 1 MiB |
| `invalid_syntax_node_limit` | `max_nodes` is `0` |
| `invalid_syntax_text_limit` | `max_text_chars` exceeds 1000 |
| `syntax_tree_empty` | the parser produced no renderable root |

The MCP schema rejects invalid field types and out-of-range values before
running the query.

## Authoritative artifacts

- MCP request and rendered Markdown response:
  `crates/cli/src/mcp/tools/read.rs`
- Typed Rust query and response DTOs:
  `crates/query/src/lib.rs`
- Generated JSON Schema:
  `docs/schema/daemon.schema.json`
- Generated TypeScript types:
  `packages/client/src/generated.ts`
