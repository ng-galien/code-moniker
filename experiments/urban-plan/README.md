# Urban plan experiment (issue #12)

Isolated Node + Three.js prototype. Not part of the VS Code extension.

See [STUDY.md](STUDY.md) for the index → scene-IR → renderer split.

## Run the viewer (offline fixture)

The checked-in `public/snapshot.json` is a crate-level capture of this repo
(`lang:rs/dir:crates`). Height and footprint follow `defs`. Roads are the
strongest identity-graph edges, budgeted.

```sh
cd experiments/urban-plan
npm install
npm run dev
```

Open the printed localhost URL. Orbit is constrained; the ground plane stays
put when you only change height later.

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
- `URBAN_PLAN_MIN_COUNT` default `5`
- `URBAN_PLAN_MAX_ROADS` default `24`
