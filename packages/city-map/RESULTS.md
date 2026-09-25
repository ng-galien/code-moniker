# First indexed experiments

Measured locally on 22 September 2026 with the existing client and local daemon.
These are scoped subgraphs from the Code Moniker project, not complete-project
or cross-project validation. No Project Views were used. All selected symbols,
including local variables, parameters and generated declarations, were retained.

| Scope | Symbols | Internal resolved reference occurrences | Incoming references outside scope | Isolates | Nontrivial directed SCCs |
| --- | ---: | ---: | ---: | ---: | ---: |
| `packages/client/src/**` | 2,084 | 3,529 | 0 | 972 | 0 |
| `crates/workspace/src/snapshot/**` | 2,189 | 3,638 | 2,732 | 567 | 2 |

Zero source-identity ambiguity omissions occurred in either capture. The client
capture excluded 319 indirect alias expansions. Unresolved and external outgoing
references are not in the incoming resolved projection; they cannot be counted
as absent dependencies.

## Algorithm comparison

Counts below include singleton communities. Modularity is measured on the same
undirected weighted graph for each scope, at resolution 1.

| Algorithm | Client groups | Client modularity | Snapshot groups | Snapshot modularity |
| --- | ---: | ---: | ---: | ---: |
| Connected components | 1,092 | 0.129 | 597 | 0.064 |
| Jaccard single linkage, threshold 0.5 | 1,843 | 0.321 | 2,059 | 0.048 |
| Weighted label propagation | 1,233 | 0.744 | 790 | 0.765 |
| Modularity local moves | 1,266 | 0.726 | 861 | 0.737 |
| Multilevel Louvain, disconnected groups split | 1,110 | 0.798 | 629 | 0.831 |

Louvain produces 138 non-singleton client groups and 62 non-singleton snapshot
groups. Its largest group contains 6.5% and 6.1% of the respective inventories.
The connectivity baseline instead joins 37.7% and 69.9% into one group.

The five algorithms together took approximately 58–63 ms on the stored client
graph and 87 ms on the snapshot graph. Computing all five original circular layouts
took about 3.4 s and 4.8 s. These are single local observations, not controlled
benchmark averages. Browser selection reuses the precomputed layouts.

Fresh index startup plus capture took approximately 15.4 s for the client scope
and 33.4 s for the snapshot scope. These times are not comparable: the latter
index also saw newly generated analysis JSON before an ignore rule was added.
The generated captures are now ignored to prevent reindexing their own results.

## Sensitivity probes

On the client graph, Louvain at resolution 0.5 / 1 / 2 yields 133 / 138 / 146
non-singleton groups. This is a first scale probe; it does not establish stability
under source edits or random ordering.

Keeping only calls and method calls leaves 1,930 isolated symbols out of 2,084.
Keeping only type usage leaves 1,561. The all-relations graph is dominated by
type usages (2,105 occurrences) and reads (1,119), with 186 call occurrences.
Thus a high modularity score for one relation subset is not evidence that it
provides a comprehensive architectural view.

## Interpretation

The immediate issue is graph granularity and relation semantics, not polygon
styling. Local syntax and generated type declarations produce many tiny groups.
The next projection should compare aggregation into navigable owners while
accounting for every underlying symbol. Type-use similarity and call-flow
communities should remain distinguishable. Do not interpret isolated symbols
as dead code or infer an architectural violation from a crossing alone.

The original circular districts and phyllotactic member placement have been
replaced with relationship-driven internal placement, degree-based central
attraction, variable footprints, parcel hulls and obstacle-avoiding streets.
The current geometry is still an approximate graph embedding, not a recovered
architectural map or a realistic urban generator. Shared street corridors and
cross-revision layout stability remain unimplemented.

Validation includes synthetic weak-bridge recovery, directed cycles, deterministic
input permutations, empty/isolated graphs, source generation mismatch, inventory
pagination, explicit omission counts, and a Three.js scene construction test.
Additional regressions cover hub placement, parcel separation, road clearance,
indexed witnesses for every street, and empty geographic layouts.
The browser preview was inspected through screenshots; algorithm switching and
the inspector were exercised. This is a usable first comparison, not a finished
architectural city renderer.

## Geographic revision

The relationship-driven layout was replayed on the two stored captures; no new
daemon capture was performed. Computing all five layouts took 13.7 s for the
client and 22.7 s for snapshot in concurrent local runs, not a controlled benchmark.

For Louvain, the client routes all 44 inter-community arteries and all 974 local
backbone streets. Snapshot routes 79 of the 80 attempted arteries (146 total),
and all 1,200 attempted local streets (1,560 backbone candidates). One attempted
snapshot artery exceeds the bounded routing search. Unattempted paths are
display-budget omissions, not missing index relationships. Pairwise checks found
no overlapping building footprints in either Louvain layout.
An exact segment/rectangle check on all 4,898 client street segments found no
intersection with any building footprint. The preview was inspected at overview
and close-up scales; the floating information panel and building selection were
verified on `CodeMonikerClient`.

Fourteen focused tests pass, including central placement of a synthetic hub,
exact segment/footprint clearance on an obstacle fixture, and proof that every
street endpoint pair has an indexed relationship. These tests do not establish
semantic correctness of centrality or optimal geographic placement.
