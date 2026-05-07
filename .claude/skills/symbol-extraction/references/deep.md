# Deep extraction — parameters, locals, callbacks

Deep extraction captures symbols below the repo-wide public surface of a
module: function parameters, local variables, inline callbacks, function
expressions. These symbols are **in scope** for `pg_code_moniker`.

ESAC may choose not to flatten them into the repo-wide `esac.symbol` /
`esac.linkage` projections. That does not make them out-of-scope. They power
resource-scoped analysis: outline detail, local impact, coverage attribution,
local reasoning, and planning before code is written.

## What to emit

For each deep symbol:

- **parameters** — under their enclosing callable. Kind label `param`.
  Position is the parameter node's range. Type metadata from the annotation
  when present.
- **locals** — `lexical_declaration` / `variable_declaration` (TS),
  `assignment` at function scope (Python), local variable declarations
  (Java) inside callable bodies. Kind label `local`. Position is the
  declarator range.
- **inline callbacks / function expressions** — `arrow_function`,
  `function_expression`, Python `lambda`, Java lambda — when they are
  bound to a name (callback argument, named local), emit a `function`
  def. When they are anonymous (inline, no binding), emit anyway with a
  deterministic synthesized name.

## Synthesized names — determinism

Anonymous callables need a stable name so two extractions of the same
source produce the same moniker. Use lexical position:

```
__cb_<start_line>_<start_col>
```

`start_line` and `start_col` from the AST node — both 0-based or both
1-based, pick one and document it. Never use a counter that depends on
visit order across files; same source must produce the same name even if
the file is extracted in isolation.

For a callable that already has a name (named function expression, arrow
bound to a const), use the name. Only synthesize for truly anonymous
callables (e.g. `arr.map(x => x.id)`).

## Containment

Every deep symbol's `parent` is the innermost enclosing **def** in the
graph — usually the enclosing method/function, never the AST parent.
A local inside a method has the method as parent, not the block statement
or the variable declarator list.

When the enclosing scope is itself an anonymous callback, the parent is
that callback's def. The graph stays a strict tree — the walker emits the
callback's def before descending into its body, so parent containment
remains valid.

## Refs from deep symbols

Refs sourced from inside a method body anchor on the method, not on the
parameter or local that participates in the expression. The deep symbols
are **defs**, not preferred ref sources.

Exception: when a ref's source semantics genuinely belong to the local
(e.g. a typed local's `uses_type` ref points at the type annotation —
the source is the local itself). These are rare; default to the
enclosing callable.

## Projection Rule

Deep symbols should be present in the resource-scoped `code_graph`. ESAC's
projection can then choose:

- repo-wide index: public/module-level symbols and refs only;
- resource-scoped view: include `param`, `local`, anonymous callbacks and
  their local refs;
- planning view: include symbolic locals when a proposed implementation needs
  them.

Do not drop locals in the extractor just because the current repo-wide index
does not consume them.

## What Deep Extraction Is Not

- not a place to emit name-resolution refs for every identifier read.
  ESAC's projection decides whether identifier reads enter `esac.symbol_ref`.
- not a substitute for type inference. Annotations, yes. Inferred types,
  no.
- not a way to flatten nested control flow. Block statements, if/else,
  loops are containers, not defs. Only declarations become defs.

## Language Rollout

TypeScript already has a deep extraction expectation in ESAC. Java, Python and
SQL can land deep extraction after first graph parity, but the architecture
must not make locals impossible. If a language walker already sees locals
reliably, emitting them behind a preset/feature flag is acceptable.
