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

## Language-independent operating map

The Rust investigation defines a linkage method, not a Rust implementation
recipe. Apply the same loop to every supported language and to every dogfood
corpus. Language-specific knowledge enters only when assigning ownership or
interpreting the emitted residue.

### Required instruments

Keep three independent sources of evidence:

1. The exhaustive `linkage_census` classifies every extracted reference. It
   must report zero `unclassified` decisions and retain the final status,
   reason, language, reference kind, extractor confidence, receiver shape,
   target count, candidate identities, and structurally compatible methods.
2. A source-level witness establishes what the program means. Prefer the
   language compiler or type checker when it can provide a counter-check;
   otherwise use a minimal source fixture whose declaration and consumer are
   unambiguous.
3. The extractor output for the witness establishes what local information
   survived. It must be inspected before changing linkage. A correct source
   program and an unresolved edge do not prove that linkage owns the defect.

The benchmark and the census answer different questions. The benchmark gives
elapsed time and aggregate quality. The census explains the aggregate and is
the acceptance surface for a failure class.

### Iteration protocol

For one language, one discovery corpus, and a fixed qualification panel:

1. Record the exact revision, corpus root, source count, elapsed phases, total
   references, and counts for all six final statuses: `resolved`, `external`,
   `candidate`, `dynamic`, `blocked`, and `unresolved`.
2. Partition imperfect decisions first by status, reference kind, extraction
   confidence, receiver shape, and terminal reason. Do not group by call name
   first: a repeated name is a symptom, not a semantic class.
3. Subdivide the dominant bucket with evidence that changes the remedy:
   candidate cardinality, owner availability, namespace, lexical scope,
   source-set boundary, receiver type, argument types, inheritance, import
   forwarding, generated code, SDK ownership, or dynamic dispatch.
4. Select a complete and semantically coherent class. Keep several source
   witnesses from the corpus, including at least one case that must not resolve,
   so that a tempting name-only fallback cannot pass acceptance.
5. Inspect the extracted graph for those witnesses and assign the earliest
   boundary that lost information using the responsibility matrix below.
6. Add one focused regression for the class contract. Correct the owning
   boundary generically; do not add a call-name, project, or file-specific
   exception.
7. Re-run the same corpus census. The selected bucket must shrink by the
   expected cardinality, neighbouring buckets must not grow unexpectedly, no
   formerly certain edge may become weaker or retarget incorrectly, and
   `unclassified` must remain zero.
8. Run the same before/after census on at least two other corpora for that
   language: one small and one medium when available. A zero delta is useful
   evidence for a syntax class absent from a corpus; any non-zero delta must be
   explained by the same membership predicate, not merely accepted as a score
   improvement.
9. Compare the aggregate benchmark with the fixed oracle. Quality must not
   regress; performance must be recorded but cannot compensate for false
   edges. Remove any older specialization made redundant by the correction.
10. Commit the validated class before selecting the next one. The commit subject
   names the semantic correction, not the witness project.

### Responsibility matrix

| Observed loss | Owning boundary | Required correction |
| --- | --- | --- |
| reference absent, wrong kind/arity/span, receiver collapsed, local type or nesting lost | language extractor / local semantic pass | emit the missing locally knowable fact or residue |
| local or parameter name competes outside its lexical scope | local scope resolution | bind or eliminate the intra-file alternative before global linkage |
| import, alias, package, module, source set, or visibility is misrepresented | language binding model | normalize the language's binding and namespace rules |
| correct residue exists but compatible declarations are absent | catalog / workspace surface | expose the required workspace, SDK, manifest, generated, or inherited symbols |
| correct residue and candidates exist but owner/type evidence is not propagated | linkage refinement | consume receiver, return, argument, heritage, or forwarding evidence |
| correct candidates remain indistinguishable under available facts | final decision | retain `candidate` or `dynamic`; never guess from uniqueness in the corpus |
| declaration is outside the analysed workspace or SDK model | external classification | classify as external with a causal origin rather than unresolved |

The boundary rule is deliberately language-neutral: extraction reports facts
that are decidable inside one source unit; linkage combines those facts with
other source units and catalogs. Java overload resolution, Rust namespaces,
TypeScript structural APIs, Python dynamic dispatch, C declarations, C# type
metadata, Go selectors, and SQL schema lookup differ in residue vocabulary, not
in this diagnostic sequence.

### Failure-class acceptance

A class is complete only when:

- its membership predicate is explicit and reproducible from census evidence;
- every selected positive witness resolves to the source-proven declaration;
- negative witnesses remain unresolved, candidate, dynamic, or external as
  their semantics require;
- the before/after delta accounts for the whole selected bucket, rather than
  one convenient call site;
- no weaker aggregate status is hidden by a stronger but incorrect edge;
- extractor, linkage acceptance, formatting, and lint gates relevant to the
  changed boundary remain green.

If precise linkage cannot be established, an unused-symbol analysis must be
inconclusive when a compatible imperfect reference exists. Missing linked edges
must never be presented as definitive proof that a symbol is unused.

## First Java application: Gson baseline

The first cross-language run applies the operating map to
`dogfood/java/gson` on this branch. The command is:

```sh
cargo run -q -p code-moniker-workspace --release \
  --example linkage_census -- \
  dogfood/java/gson /tmp/code-moniker-linkage-census-gson-baseline.jsonl
```

The census classified all 56,441 references with zero `unclassified` or
`blocked` decisions:

| Final status | Count |
| --- | ---: |
| resolved | 32,433 |
| external | 21,828 |
| candidate | 1,581 |
| dynamic | 70 |
| unresolved | 529 |

The first partition of the 2,180 imperfect decisions is:

| Class witness | Count | Current interpretation | Next evidence |
| --- | ---: | --- | --- |
| imported identifier `method_call`, multiple targets | 1,467 | owner is known; Java overload remains ambiguous | argument expression types and applicability |
| name-matched `method_call`, receiver `call` | 135 | interrupted return chain or external API | inner-call decision and return type |
| name-matched `method_call`, receiver `member` | 120 | interrupted field/member receiver | field path and declared type |
| resolved-confidence `uses_type`, no candidate | 86 | local/nested type or target-shape/source-set mismatch | exact extracted definition identity |
| name-matched `method_call`, identifier receiver | 59 | missing receiver binding, inheritance, or SDK owner | receiver declaration and catalog owner |
| duck-typed identifier method set | 56 | owner not proven; dynamic is conservative | local type and heritage facts |
| resolved-confidence direct `calls`, multiple targets | 54 | overload or static-owner ambiguity | owner plus argument types |
| resolved-confidence `instantiates`, no candidate | 34 | constructor/type identity mismatch or absent catalog | type definition and constructor identities |

This baseline already separates two very different dominant cases. Of the
1,467 imported identifier-method candidates, 1,347 are `fromJson` or `toJson`:
the `Gson` owner is found, but name and arity retain several overloads. That is
an overload-refinement class, not receiver resolution. Conversely, the 314
unresolved Java method calls include 119 with no structurally compatible
workspace method, 84 with exactly one, and 111 with several. A unique method in
the corpus is diagnostic evidence only; it is not permission to link without
receiver or owner evidence.

The next Java iteration starts with the smallest coherent owner-proven class,
then moves toward receiver chains and overload applicability. Each correction
must update this table with its exact before/after delta.

### Java iteration 1: callable-scoped local types

The baseline's resolved-confidence type failures exposed an extractor-owned
class. Gson contains many Java records and classes declared inside methods. The
extractor emitted references such as `uses_type LocalRecord` and
`instantiates LocalRecord`, but emitted no corresponding local type definition.
Its predeclaration table was keyed only by simple name, so several methods that
each declared `LocalRecord` also collapsed onto one class-scoped identity.

The correction makes the Java type table lexical: declarations are keyed by
declaring scope and simple name, lookup walks the current callable/type ancestry,
and local class, interface, enum, record, and annotation declarations are
emitted beneath their callable. Reference traversal enters the same local type
scope. A regression uses two methods that each declare a different `record
Local`; both definitions and both sets of references must remain distinct.

The repeated Gson census produced this delta:

| Final status | Before | After | Delta |
| --- | ---: | ---: | ---: |
| resolved | 32,433 | 32,659 | +226 |
| external | 21,828 | 21,968 | +140 |
| candidate | 1,581 | 1,581 | 0 |
| dynamic | 70 | 70 | 0 |
| unresolved | 529 | 404 | -125 |
| total references | 56,441 | 56,682 | +241 |

The 241 new references come from local type bodies and record components that
were previously absent. Within the unresolved bucket, `uses_type` fell from
115 to 22, `instantiates` from 35 to 7, `method_call` from 337 to 335, and
`reads` from 6 to 4. All 26 unresolved `LocalRecord` type uses disappeared;
candidate and dynamic counts did not move. The Java extractor conformance,
contract, and snapshot suites remained green.

The fixed `main` oracle and the branch benchmark report a linkage score change
from 93.89% to 94.27%. In one run, linkage time moved from 163 ms to 142 ms,
while index time moved from 296 ms to 339 ms after adding 148 symbols and 241
references; total time moved from 527 ms to 540 ms. A single run at this scale
does not establish a performance regression or improvement, so these timings
are recorded as context rather than acceptance. The quality and cardinality
deltas are deterministic.

This is the intended operating-map outcome: one source-proven class was fixed
at extraction, its complete corpus delta was measured, and unrelated overload
ambiguity was left untouched rather than hidden by a global name fallback.

### Java qualification panel

Java corrections are not accepted on Gson alone. The initial panel fixes three
independent repositories and revisions:

| Role | Corpus | Revision | Java files | Baseline references |
| --- | --- | --- | ---: | ---: |
| discovery / library with overload-heavy API | `dogfood/java/gson` | `828a97b` | 249 | 56,441 before local-type extraction |
| small application/library | `../fork/rsql-jpa-specification` | `a405ec1` | 91 | 14,229 |
| medium driver and test suite | `../fork/pgjdbc` | `77837f80` | 1,131 | 119,562 |

The second Java class concerns qualified inner-class creation such as
`outer.new Inner()` and `parent.new Child()`. The extractor previously ignored
the qualifying expression's type and targeted a fictitious same-package
`module:Inner/path:Inner`. The correction preserves the outer receiver type and
targets its nested class for both `instantiates` and `uses_type`.

Its cross-corpus qualification is exact:

| Corpus | Resolved delta | Unresolved delta | Candidate/dynamic/blocked delta |
| --- | ---: | ---: | ---: |
| Gson | +12 | -12 | 0 |
| rsql-jpa-specification | 0 | 0 | 0 |
| pgjdbc | 0 | 0 | 0 |

The zero deltas mean the qualified-creation syntax is absent from the two
additional corpora under the measured facts; they also show that ordinary
creation and nested-type lookup were not perturbed. Future Java classes use the
same panel, extended with a targeted Cassandra or Pulsar subtree when the class
needs scale or framework-specific evidence. Full Cassandra and Pulsar are not
the default inner loop.

## Rust witness acceptance

The original catalog witness remains a concrete application of the general
method. It is complete only when all 13 rustc-proven production call sites are
accounted for, each call is linked to its `CandidateCatalog` or
`CandidateIndexes` declaration, each of the six methods has at least one linked
`in_ref`, the temporary rule reports none of them as unused, and Core and
Workspace regressions remain green.
