# Urban plan experiment (issue #12)

Isolated Node + React prototype. Not part of the VS Code extension.

- **Capture** talks to the daemon with `@code-moniker/client/node`
  (`NodeDaemonRuntime` + recursive `client.graph.children()` + crate-level
  `client.graph.identity()` for coupling arteries).
- **Scene IR** (`src/scene.ts`) has no renderer imports.
- **Layout** is a padded squarified treemap: crates → src/tests → modules
  as nested quartiers, types as buildings, gutters as rues.
- **Viewer** is React 19 + Vite + React Three Fiber + Drei (orthographic
  city, click-to-zoom a quartier, hover state). Issue #12's VS Code webview
  should reuse this IR later.

See [STUDY.md](STUDY.md) for the index → scene-IR → renderer split.

## Run the viewer (offline fixture)

The checked-in `public/snapshot.json` is a nested capture of this repo
(`lang:rs/dir:crates`). Quartiers follow the identity tree; building height
follows `defs`. Gold L-shaped artères are the strongest crate-level
identity-graph edges. Click a quartier to zoom in.

```sh
cd experiments/urban-plan
npm install
npm run dev
```

Vite prints a localhost URL (default `http://localhost:5179/`) and opens it.
The right-hand panel retouches aggregation, zones, height metric, street
gaps and coupling live (HMR + localStorage). Recapture only if the index
changed.

## Recapture from a live daemon

Capture walks up to the Cargo workspace root, then looks up that daemon:

```sh
code-moniker query "workspace.status"   # connect-or-start if needed
cd experiments/urban-plan
npm run capture
```

The Node client must match the daemon protocol (handshake). Recycle an older
daemon after a CLI install.

Then refresh the viewer. Optional env:

- `URBAN_PLAN_PREFIX` default `lang:rs/dir:crates`
- `URBAN_PLAN_MAX_DEPTH` default `4` (crates → src → module → types)
- `URBAN_PLAN_MIN_COUNT` default `8` (crate-to-crate artères)
- `URBAN_PLAN_MAX_ROADS` default `16`
- `URBAN_PLAN_CITY_SIZE` default `110`
