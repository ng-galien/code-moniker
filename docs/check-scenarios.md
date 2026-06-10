# Executable check scenarios

A scenario is a Markdown document that bundles a rules overlay, a file layout,
and the violations that layout is expected to produce. One document is at the
same time readable documentation, an acceptance fixture, and a CI-validated
sample. The reference samples live in [`samples/`](../samples/).

## Format

A scenario is CommonMark. The runner only interprets two things: an optional
front matter block, and top-level fenced code blocks whose info string carries
a `cm:` attribute. Everything else is prose and stays untouched.

````markdown
---
name: java-layer-boundaries
lang: java
blurb: Domain code never depends on infrastructure
published: true
---

# Free-form documentation

```toml cm:rules
[[refs.where]]
id   = "domain-depends-only-inward"
expr = "source ~ '**/*:domain/**' => NOT target ~ '**/*:infrastructure/**'"
```

```java cm:file=src/main/java/com/acme/domain/Order.java
package com.acme.domain;
...
```

```cm:expect
refs.domain-depends-only-inward @ src/main/java/com/acme/domain/Order.java:L3
```
````

### Front matter

Flat `key: value` lines between two `---` lines at the very top. Recognized
keys (anything else is an error):

| Key | Meaning |
| --- | --- |
| `name` | Scenario identifier. |
| `lang` | Primary language tag (informative). |
| `blurb` | One-line description for catalogs. |
| `published` | `true` to expose the scenario in user-facing catalogs. |
| `default_rules` | Override the built-in rules; defaults to `false` when a `cm:rules` block is present, `true` otherwise. |

### Significant fences

The info string is `<language> <attributes>`; the runner looks for one `cm:`
token per fence:

- `cm:rules` — the `.code-moniker.toml` overlay materialized at the workspace
  root. At most one per document.
- `cm:file=<relative/path>` — one workspace file. Paths must be clean relative
  paths: no `..`, no `.`, no absolute paths, no duplicates.
- `cm:expect` — the expected violations, one per line. At most one per
  document.

Constraints kept deliberately strict so any CommonMark renderer agrees with
the runner: significant fences are top-level, unindented, backtick-only, and
closed by a fence at least as long (use a longer fence to embed backticks).
Fences without a `cm:` token are ignored prose.

### Expectations

```
<rule-id> @ <relative/path>:L<start>[-L<end>]
```

Line numbers are 1-based and local to the `cm:file` fence content (the opening
fence line is not part of the file). Blank lines and `#` comments are allowed.
The comparison is an exact multiset: every expected violation must be observed
and every observed violation must be expected.

A rule that cannot be demonstrated in a small layout is excused explicitly,
with a reason:

```
! <rule-id> <reason>
```

The contract harness rejects silent rules that are not excused, and excused
rules that actually fire (a stale or misspelled marker). Bless preserves the
directives.

## Running

```sh
code-moniker check . --scenario samples/java-layer-boundaries.md
```

The runner materializes the layout in a temporary directory, replays the real
scan pipeline (project mode, so cross-file `refs` rules resolve), prints the
observed violations, and exits non-zero on any mismatch. Rules that never fire
are reported as `silent rules` — a sample whose rules are not demonstrated is
rejected by the contract harness.

### Bless

```sh
CM_SCENARIO_BLESS=1 code-moniker check . --scenario samples/java-layer-boundaries.md
CM_SCENARIO_BLESS=1 cargo test -p code-moniker --test samples_contract
```

Bless rewrites the `cm:expect` block in place from the observed violations
(appending one if missing). Review the resulting diff like any snapshot
update: an unexpected line shift is a signal, not noise.

## CI contract

`crates/cli/tests/samples_contract.rs` replays every `samples/*.md` document
and fails on: expectation mismatches, configured rules that never fire, and
samples that demonstrate nothing.
