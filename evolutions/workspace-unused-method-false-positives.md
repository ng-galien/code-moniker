# Workspace unused-method false positives

Date: 2026-08-03

## Signal

This temporary warning rule was used while cleaning
`crates/workspace/src/linkage`:

```toml
[[workspace.symbol.where]]
id = "unreferenced-linkage-callable"
severity = "warn"
expr = "NOT (moniker ~ '**/dir:crates/dir:workspace/dir:src/*:linkage/**' AND shape = 'callable' AND kind != 'test' AND visibility != 'public') OR count(in_refs) > 0"
message = "Internal linkage callable `{name}` has no indexed incoming reference."
```

It reported 79 internal callables without incoming references, with only 88%
linkage coverage. Six methods in `linkage/catalog/candidate.rs` were selected
for a compiler counter-check.

## Compiler counter-check

The six methods were temporarily changed from `pub(in crate::linkage)` to
private, then checked with:

```sh
cargo check -p code-moniker-workspace --lib
```

Rust emitted 13 `E0624` errors, proving these production consumers:

| Method | Consumers rejected by rustc |
| --- | --- |
| `CandidateCatalog::refresh_files` | `linkage/change/refresh.rs:72` |
| `CandidateCatalog::candidate_for_symbol_id` | `linkage/change/rebind.rs:186`, `:550`, `:564` |
| `CandidateIndexes::symbol_by_moniker` | `linkage/resolve/reference_resolver.rs:294`, `linkage/resolve/refinement/receivers/methods.rs:116`, `linkage/resolve/refinement/receivers.rs:27` |
| `CandidateIndexes::source_candidate_keys` | `linkage/change/rebind.rs:597` |
| `CandidateIndexes::symbols_by_language_key` | `linkage/resolve/scope.rs:56` |
| `CandidateIndexes::symbols_by_source_key` | `linkage/language/c/includes.rs:138`, `:166`, `linkage/resolve/scope.rs:39`, `:138` |

The visibility changes were reverted. The compiler result proves that the six
unused-method warnings are false positives. It does not prove that all 79
warnings are false positives.

## Corrected diagnosis

The six false positives do not all come from one extraction defect. They cover
three receiver shapes:

| Shape | Example | Missing capability |
| --- | --- | --- |
| local sum binding | `Some(candidates) => candidates; candidates.refresh_files()` | preserve the receiver type through the local `match` binding |
| typed field receiver | `graph.candidates.candidate_for_symbol_id(...)` | preserve or reconstruct the `graph -> candidates` field chain and its type |
| method-result receiver | `catalog.indexes().symbols_by_source_key(...)` | resolve `indexes()`, obtain its return type, then resolve the outer method |

The first shape was fixed in commit `0b67dce`. A release dogfood comparison
against the `main` oracle gained exactly one resolved reference while the
displayed linkage score remained `91.49%`. That result proves the fix is valid
for `refresh_files`, but also proves it is not the complete six-method bug.

The remaining calls are present in the index as `method_call` references with
the correct call name and arity. Their targets remain under the caller module,
for example:

```text
definition:
.../struct:CandidateIndexes/method:symbol_by_moniker(moniker:&Moniker)

call target:
.../module:resolve/module:reference_resolver/method:symbol_by_moniker
confidence: unresolved
receiver_hint: call
```

The Rust extractor currently collapses compound receivers to generic hints:

```text
field_expression -> member
call_expression  -> call
```

The linkage then tries to reconstruct the receiver chain from reference spans
and type facts. Two gaps prevent it from completing the job:

1. `member` does not identify the actual field `candidates`, so a fact such as
   `EditedGraph.candidates: CandidateCatalog` cannot be selected reliably.
2. receiver-chain refinement runs before structural receiver refinement and
   consumes only uniquely resolved or external inner calls. The inner
   `indexes()` call is therefore not usable when the outer method is examined.

## Responsibility boundary

Retargeting a call to a declaration in another file belongs to linkage, not to
the file extractor.

The local Rust analysis must emit a precise residue:

```text
receiver path + locally deducible type + call name + arity + call nesting
```

For example:

```text
graph -> field candidates -> CandidateCatalog -> candidate_for_symbol_id/1
catalog -> indexes/0 -> CandidateIndexes -> symbol_by_moniker/1
```

The linkage consumes that residue together with exported definitions and
`returns_type` facts, then binds the call to the declaration owned by
`CandidateCatalog` or `CandidateIndexes`.

The current defect crosses this boundary: local extraction discards receiver
structure as `member`/`call`, while linkage attempts to recreate it too late
and without a fixed-point dependency order.

## Broader linkage problem

The six methods are a witness, not the unit of work. Linkage has historically
improved through local fixes without an exhaustive classification of the
references that remain weak, dynamic, blocked, or unresolved. A final count
does not explain which stage lost information, which refinements were
applicable, or why they failed.

The original `linkage_census` compounded this problem. It recognized only
resolved, external, blocked, and unresolved references. Candidate and dynamic
decisions were labelled `untracked`, then omitted from detailed output. The
diagnostic tool therefore hid important classes of imperfect resolution.

The census must first expose every final decision using the existing snapshot
contract:

```text
status + reason + language + reference kind + extraction confidence
+ receiver shape + target count + candidate identities
```

The next level of explanation must account for the full decision path:

```text
extracted facts
-> query construction
-> candidates produced
-> candidates rejected by policy
-> applicable refinements
-> refinement result
-> final decision and reason
```

Stable failure classes can then be derived from evidence instead of individual
call sites: incomplete extraction metadata, missing query, no candidates,
multiple candidates, weak name match, policy rejection, interrupted field
receiver, interrupted return chain, unsupported language semantics, or
genuinely dynamic behavior.

## First exhaustive dogfood census

The corrected final-decision census on the Code Moniker repository classified
all 231,441 references, with zero `unclassified` decisions:

| Final status | Count |
| --- | ---: |
| resolved | 148,865 |
| external | 67,141 |
| candidate | 4,960 |
| dynamic | 1,593 |
| blocked | 6 |
| unresolved | 8,876 |

The largest imperfect-resolution buckets, grouped without interpreting them
yet, are:

| Status | Language | Kind | Receiver shape | Count |
| --- | --- | --- | --- | ---: |
| candidate | Rust | reads | none | 1,965 |
| unresolved | Rust | calls | none | 1,801 |
| candidate | Rust | uses_type | none | 1,767 |
| dynamic | Rust | method_call | identifier | 1,481 |
| unresolved | Rust | method_call | member | 1,361 |
| unresolved | C | reads | none | 994 |
| unresolved | TypeScript | method_call | identifier | 977 |
| unresolved | Rust | method_call | call | 840 |
| unresolved | Rust | method_call | identifier | 763 |
| unresolved | TypeScript | method_call | member | 661 |

This establishes the correct scale: the catalog methods belong to the Rust
`method_call/member|call` family, but that family must be analysed as a whole
before changing resolution code.

## Dominant corpus-wide problem classes

The raw buckets consolidate into five dominant classes covering 14,103 of the
15,435 imperfect decisions (91.4%):

| Class | Count | Share of imperfect decisions | Evidence |
| --- | ---: | ---: | --- |
| Rust lexical and namespace ambiguity | 4,901 | 31.8% | multiple-target candidates, dominated by 1,965 reads and 1,767 type uses |
| Rust receiver-type propagation failure | 4,560 | 29.5% | 3,079 unresolved method calls plus 1,481 duck-typed method fallbacks |
| Rust direct or associated callable binding failure | 1,801 | 11.7% | unresolved `calls`; 979 have one structural method candidate, 654 several, 168 none |
| TypeScript method ownership absent from the usable catalog | 1,844 | 11.9% | 1,807 unresolved plus 37 dynamic method calls; unresolved calls have zero workspace method candidates |
| C member-read extraction failure | 997 | 6.5% | 994 unresolved reads plus 3 preprocessor-dynamic reads; 959 target `lookahead` in generated parser code |

The first class already exposes two distinct namespace defects. Local Rust
reads commonly see both a parameter and a synthetic local with the same name.
Rust type references also compete with value-namespace symbols such as enum
constants (`Config` the type versus `DefaultRulesSelection::Config`). Candidate
selection is therefore not respecting lexical binding and symbol namespace
strongly enough.

The second class is the general receiver problem: identifier, field/member,
and method-result receivers lose or fail to propagate their type. The catalog
witness is only one small sample of this 4,560-reference class.

The third class is separate from receiver calls. It contains associated,
trait-provided, generated, and ordinary callable targets represented as
`calls`. Common examples include `default`, `evaluate`, `parse`, and
`from_view`; the class still needs subdivision by exact target ownership before
any correction.

The TypeScript class is dominated by collection, standard-library, VS Code,
and local API methods such as `push`, `join`, `map`, `registerCommand`, and
`getChildren`. Zero workspace method candidates does not by itself distinguish
an SDK/catalog omission from lost local type information, so that distinction
must be measured next.

The C class is highly concentrated rather than general: generated parser code
accounts for almost all `lookahead` reads. It should be treated as a C
field/member extraction class, not as a global no-candidate heuristic.

Comparing unresolved calls with workspace method declarations of the same name
and arity splits that family again:

| Receiver shape | No structural candidate | One candidate | Multiple candidates |
| --- | ---: | ---: | ---: |
| `member` | 163 | 697 | 501 |
| `call` | 114 | 136 | 590 |

These are not interchangeable failures. A unique structural candidate may be
recoverable without guessing, multiple candidates require receiver-type
evidence, and zero candidates may indicate an external/unindexed method or
missing extraction. The census now records the compatible method identities so
these classes can be verified instead of inferred from a single call site.

The six catalog methods now have an exact accounting:

- one `refresh_files` call is resolved after preserving its local `match`
  binding type;
- eleven calls are indexed as `unresolved/no_candidate`, and every one has
  exactly one workspace method with compatible name and arity;
- one of the thirteen rustc-proven call sites is not present as a distinct
  indexed reference and must be investigated at extraction traversal level.

Thus the false-unused witness spans two measured classes: incomplete receiver
linkage for eleven references and one missing extracted call. Treating either
class alone as the complete bug would leave the diagnostic unsound.

## Plan

1. Keep `linkage_census` exhaustive for every final decision and fail the
   diagnostic if any reference is unclassified.
2. Add decision-path evidence: candidate counts before and after policy,
   refinements considered, and the terminal reason for each failed refinement.
3. Run the full dogfood census and group by language, reference kind, receiver
   shape, decision stage, and reason.
4. Select a complete failure class by cardinality and semantic coherence. Add
   one regression representing the class, not one regression per call site.
5. Correct the class at its owning boundary. A local-analysis fix must improve
   extracted residue; a linkage fix must consume that residue generically.
6. Re-run the census and require the complete bucket to shrink or disappear
   without growth in neighbouring buckets. Remove specialized paths made
   redundant by the general correction.
7. For the catalog witness, reconcile all 13 compiler-proven call sites with
   indexed references, require every call to target its declaration, and
   require all six false unused-method warnings to disappear.

## Acceptance

The work is complete only when:

- all 13 rustc-proven production call sites are accounted for by the index;
- each call is linked to its `CandidateCatalog` or `CandidateIndexes`
  declaration;
- each of the six methods has at least one linked `in_ref`;
- the temporary unused-method rule reports none of the six as unused;
- Core and Workspace extraction/linkage regressions remain green.

If precise receiver linkage cannot be established, an unused-method analysis
must be inconclusive when a compatible unresolved call exists. Missing linked
edges must never be presented as definitive proof that a method is unused.
