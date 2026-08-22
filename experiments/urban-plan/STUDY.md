# 2.5D urban plan from the Code Moniker index

Ticket: [#12](https://github.com/ng-galien/code-moniker/issues/12)
Branch: `feat/12-urban-plan`

This experiment is **outside** the Rust crates and the VS Code extension.
It exists to prove an intermediate scene model, then a Three.js read-out.
Issue 12 already forbids blending layout math into the renderer.

## What #12 actually asks for

Not a photorealistic city. A **navigable 2.5D plan**:

- stable ground (districts / zones)
- height for a chosen metric
- coupling as roads, not spaghetti
- drill-down into Cockpit later
- primitives first, named view DSL later
- Three.js via R3F in the webview *eventually*; this folder is a Node
  capture + a dumb viewer so the IR can be tested without VS Code.

Related: #6 (Cockpit) is the 2D ego graph. #12 is the **repository-wide
orientation** layer that should *hand off* to Cockpit, not replace it.

## Why an intermediate snapshot

The live index is the wrong thing to feed Three.js.

| Index fact | Scene primitive | Why not bind directly |
|---|---|---|
| `identity.graph` node (`defs`, `has_children`) | building | needs a stable (x, z) the daemon does not own |
| `identity.graph` edge (`count`, `kinds`) | road | needs budgeting, direction, LOD |
| view / alias / fragment scope | zone / district | architectural, not a graph node |
| `metrics.coupling` | road weight | pairwise, not a city layout |
| `symbol.graph` | interior / drill | ego graph, too local for the plan |
| generation + prefix + path + min_count | snapshot identity | the scene must be replayable |

If the viewer talks to the daemon, every camera move risks mixing
**layout**, **metric**, and **index refresh**. The snapshot freezes:

1. which aggregates were selected
2. which metric drove height/footprint
3. which coupling edges survived the budget
4. the 2D placement that must stay still when only height changes

Static file (`snapshot.json`) = CI, demos, two-repo comparison.
Same schema streamed from a watcher = dynamic, later.

## Proposed pipeline

```text
daemon snapshot (generation N)
        │
        ▼
  SceneCapture  (@code-moniker/client)
        │  identity.graph + optional coupling
        ▼
  SceneIR  (zones, buildings, roads, metrics, layout)
        │
        ├─ write snapshot.json          (static)
        └─ later: watch generation      (dynamic)
                │
                ▼
         SceneRenderer (Three.js)
         orthographic / light isometric
         instanced boxes + budgeted lines
```

The IR does **not** contain meshes, materials, or camera. Only:

- identities (prefix / compact later)
- integer metrics (`defs`, edge `count`)
- layout slots (`u`, `v` on a grid, or a treemap rect)
- visual channels as *bindings* (`height: defs`, `footprint: log defs`)

Changing the height metric recomputes `height` from stored metrics.
It must **not** reshuffle `u,v` unless aggregation or the zone set changed.

## Capture sources (v0)

Enough to render a city of crates:

- `workspace.status` — generation, file/symbol counts
- `identity.graph prefix:"lang:rs/dir:crates" min_count:5` — buildings + roads
- later: `identity.children` to split a selected building
- later: fragment/view aliases as district masks (see `references/fragments.md`)
- later: `metrics.coupling` for a selected pair of districts

Do **not** start from `symbol.graph` of the whole repo.

## Layout (v0, deliberately dumb)

Issue 12 wants *stable geography*. First layout:

- sort buildings by identity (deterministic)
- place on a square grid (`ceil(sqrt(n))` columns)
- spacing constant; footprint `1 + log1p(defs) * k`
- height `log1p(defs) * h`

A treemap or force layout can wait. Force layout destroys the mental map
when a zone is toggled.

## Coupling (v0)

Use identity-graph edges already aggregated at the same prefix depth.
Budget: keep the top `maxRoads` by `count`, drop the rest, record
`roads_omitted` in the snapshot coverage. Do not draw 14k crate-internal
calls as individual cables.

## Open choices (for the prototype, not a DSL yet)

1. **Aggregation** — identity prefix vs fragment/view scope vs path glob.
   v0 = one identity prefix.
2. **Height metric** — `defs` now; `refs`, `unresolved`, `lines` later if
   the index exposes them on the same node.
3. **Districts** — none in v0; one color per node kind (`dir`, `module`).
4. **Dynamic** — file snapshot first. A later `watch` can replace the JSON
   when generation changes without moving the camera.
5. **Host** — this experiment is Node + Vite. The VS Code webview (R3F)
   should consume the **same IR**, not recode layout.

## What this folder is not

- not a second daemon
- not a view DSL
- not Blender/glTF content
- not Cockpit
- not a reason to put Three.js in `crates/`
