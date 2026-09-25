# Code Moniker City Map

Node/TypeScript package for automatic software communities and a Three.js preview.
It consumes the existing `@code-moniker/client`. No Project View is required.

## Run

From this directory, after the sibling client has been built:

```sh
npm install
npm test
npm run analyze -- --root ../.. --path 'packages/client/src/**' --output ../../work/research/city-map-client.json
npm run dev
```

The preview URL is printed by Vite. Its default port is 5183; it selects another
port when occupied. `CITY_MAP_DATA` selects another captured analysis JSON file.
Source changes and changes to that JSON trigger reloads. The capture is explicit;
the preview does not subscribe to daemon changes or automatically refresh the index.

`--path` is an optional test scope, not an architectural configuration. Omitting
it captures the whole indexed project, subject to a 20,000-symbol safety budget.
`--max-symbols` changes that budget. Exceeding a budget fails without publishing a
partial result. `--binary` selects a local Code Moniker executable for a newly
owned daemon. Existing daemons are reused through `NodeDaemonRuntime` and never
stopped by this package. Daemons launched by this package stop after capture.

```sh
npm run analyze -- --input ../../work/research/city-map-client.json --threshold 0.4 --resolution 0.8 --output ../../work/research/city-map-comparison.json
```

## Public entry points

- `@code-moniker/city-map`: `analyze(graph)` and `layoutCity(analysis, algorithm)`.
- `@code-moniker/city-map/node`: `captureIndex(client)` for an existing connection,
  or `captureProject(root)` using the existing client's daemon lifecycle.
- `@code-moniker/city-map/three`: `createCityScene(analysis, algorithm, layout?)`.
  Returns the group, instanced building mesh, picking identities, and `dispose()`.

The package is private pending wider validation. `npm run build` produces ESM and
declarations. `npm run build:preview` produces browser assets in `preview-dist`;
serving that build requires supplying `/data.json` separately.

## Analysis contract

The input contains all selected symbols, including non-navigable symbols, and
direct resolved incoming references. Capture pages both inventories and usages,
checks the generation on every response, excludes indirect alias expansions, and
records outside-scope references and ambiguous source identities. It uses the
resolved query target, never a guessed name match. All selected symbols remain
in the result, even if no edge is observed.

Grouping uses an undirected weighted projection; the original direction and
relation kinds remain in the captured graph. Repeated occurrences add weight;
self-references remain as evidence but do not affect community detection. Five
partitions use the same projection and metrics:

1. Connected components: connectivity baseline, not architectural communities.
2. Closed-neighborhood Jaccard followed by single linkage on existing edges.
3. Deterministic asynchronous weighted label propagation.
4. Single-level modularity local moves.
5. Graphology Louvain, with random traversal disabled and disconnected groups split.

The local-move implementation is not full Louvain. Strongly connected components
are computed separately on the directed graph. Modularity shown for comparison
uses resolution 1, even when the optimizer is tested at another resolution.

Jaccard currently uses JavaScript sets as a correctness baseline. This package
does not expose or benchmark Rust Roaring operations. Roaring stays in the
existing Rust index; a future bulk analysis query should reuse that owner and the
existing client. No new binding or transport has been added.

## Geographic projection

The community graph drives a deterministic D3 force simulation. Within each
community, relationships attract their endpoints; normalized internal degree
adds central attraction. This is a degree-based heuristic, not betweenness or
an assertion about semantic importance. Cross-community connections orient
gateway attraction using the first district layout. Final district repacking can
change that orientation; it is not a constrained boundary-port solution.

Parcel width is `1.2 + min(3, 0.5 * log2(1 + unique neighbors))`; depth is 80% of
width. A deterministic clearance pass prevents parcel overlap. District spacing
uses their actual occupied radius, with another non-overlap pass. Ground shapes
are convex parcel hulls, not circular plates. Unconnected symbols occupy a
separate compact inventory area, without invented roads or community membership.

Inter-community arteries attach near a pair of actually related symbols. The
displayed endpoints are representative witnesses, not all boundary symbols.
Local streets use a maximum-weight spanning forest of observed relations: cycles
remain in the graph, but are not all drawn. A bounded A* search avoids occupied
parcel cells; conservative line-of-sight simplification removes staircase paths.
Tiny enclosed free-cell pockets are excluded as access points. Unroutable paths
are counted, not replaced with lines through buildings. Crossings do not create
new graph connections. There is no claim that these static dependencies represent
execution traffic, or that all geometric access points are application entry points.

Three.js uses instanced bodies, podiums, shallow gable roofs for low buildings,
and setback caps for taller buildings. These architectural details are visual
treatment, not additional metrics. Height is `1 + log2(1 + incoming reference
occurrences)`. At most 80 aggregate arteries, 1,200 backbone streets and 200
selected neighbor links are shown, with counts in the UI. Hover displays the
symbol, type, source file, unique neighbors and incoming references; clicking
retains the detailed inspector. Direction and relation kinds are not yet encoded
on the road geometry.

The same ordered input is reproducible. Layout continuity across code revisions,
optimal road routing, shared street corridors, semantic zoom and calibrated
distances are not implemented. A high modularity score alone does not establish useful
architecture. Isolated nodes are isolated in the observed selected resolved
graph; they are not established dead code.

## Current scale and next measurements

Capture uses one paginated incoming-usage request per symbol. This is sufficient
for the first scoped experiments but is not a bulk export API. A complete large
workspace needs performance measurement and probably an existing-client query
extension exposing a generation-bound graph projection.

The preview is a comparison surface. Next algorithm measurements should separate
reference kinds, test coarsening local syntax into meaningful owners, compare
resolution scales, quantify sensitivity to edits, and evaluate actual navigation
tasks. The baseline must retain every node or explicitly account for aggregation.

Algorithm references: [Graphology Louvain](https://graphology.github.io/standard-library/communities-louvain.html),
[D3 force simulation](https://d3js.org/d3-force/simulation).
