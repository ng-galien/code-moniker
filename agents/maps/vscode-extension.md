# Map — VS Code Extension

Operational map for agents working under `vscode-extension/`. All commands
run from that directory. Human-facing reference lives in
`docs/vscode-extension.md`; this map is how to act on the system.

## Cartography

- `src/daemon/`: session (connect-or-start, consistency, capabilities), WebSocket RPC, registry reader, `generated.ts` (regenerated from the daemon JSON schema — never hand-edited).
- `src/symbols/`: identity tree (Symbols section) + detail webview host (`detail/panel.ts`, `detail/webview/`).
- `src/setup/`: Setup section of the workspace tree — what a freshly opened
  folder is missing (CLI, `.code-moniker.toml`, per-client agent integrations)
  and the commands that fix each row. The CLI owns the truth: the state comes
  from `code-moniker agent status`, never from guessing at files. Adding a
  module that calls the CLI facade means extending `$setup_source` in
  `vscode-extension/.code-moniker.toml` — the boundary rule is an allowlist.
- `src/explorer/`: Graph Explorer — scoped exploration webview (host `panel.ts`/`manager.ts`/`repository.ts`, React app under `webview/`). `src/shared/identity.ts` owns identity-path parsing for both sides of the bridge and for the symbol tree. Node boxes are declared to ELK before render, so a card taller than its declared box overlaps the rank below: the card geometry lives in `explorer.css` as `--cm-card-*` custom properties, read once by `cardMetrics()` — change the CSS, not a TS constant.
- `src/webview-lib/`: shared React pieces (`CodeBlock` + `code.css`, `parse.ts`, `symbolGlyph.ts`). Shared components own their styles here — duplicated CSS *will* diverge.
- `src/workbench/`: unified workspace tree wrapping the feature providers.
- `media/`: committed webview bundles (esbuild output). `test/`: integration harness and suites.
- Webviews are pure React (no hybrid); host↔webview contracts live in `protocol.ts` modules.

## Ship Routine

Use this routine when packaging or installation is in the authorized task.
During implementation, select the checks and acceptance journeys that exercise
the changed behavior; run the full suite for broad changes or release validation.

```sh
npm test && npm run compile && npm run test:acceptance
npx vsce package -o code-moniker.vsix
code --install-extension code-moniker.vsix     # then Reload Window
```

If the authorized installation also includes Rust changes, reinstall the binary
and verify the resolved executable and version. Follow the Rust map's lifecycle
guidance for the affected transport: stdio reloads its worker after executable
replacement; the `cm-mcp` tmux session is only for explicit HTTP dogfood. Scope
any necessary restart to the runtime involved in the task.

## Verification

How the extension's UI is verified, from fast unit checks to complete user
journeys. Playwright drives the real VS Code Electron workbench, TreeView,
editor and webview; traces, video and screenshots make the rendered run the
acceptance evidence.

The founding incident (2026-07-11): the Graph Explorer shipped with dead click
handlers and a detail view whose meta grid collapsed values into a 15px
sliver. Compile, typecheck and even a scope-level e2e test were green. Both
bugs were only caught by the layers documented here. The doctrine applies to
our own UI: do not trust the green build, trust the rendered run.

## Layers

| Layer | Command | Catches |
|---|---|---|
| Typecheck + samples | `npm test` | type drift, unimported samples |
| Compile | `npm run compile` | build errors, bundle freshness |
| Playwright acceptance | `npm run test:acceptance` | real TreeView/editor/webview gestures, graph layout, clicks, pan/zoom, sync and visual evidence |
| Browser harness | manual, below | focused pixel experiments outside VS Code when diagnosis needs a synthetic payload |

All commands run from `vscode-extension/`.

## Playwright Acceptance Suite

`acceptance/` follows the PostgreSQL Workbench model: global setup builds the
CLI and prepares VS Code; a worker fixture launches an isolated Electron
profile and deterministic Rust workspace; page objects own VS Code and React
Flow selectors; specs describe user journeys. The campaign is serial and
fail-fast, and scenarios never call extension feature APIs or panel handlers.

The extension exposes only an acceptance-mode lifecycle rendezvous for
activation and editor cleanup. Cockpit actions are performed through the
Workspace header, TreeView, editor, Command Palette and webview controls.
See `acceptance/README.md` for structure and evidence retention.

## Browser Harness (pixel-level)

The webview bundles are plain browser JS. Load them in a real browser with a
stubbed VS Code API, feed them the same messages the host would post, then
screenshot and audit computed styles. This catches what no assertion-based
test sees: exploded grids, diverged duplicate CSS, oversized markers,
illegible colors.

Recipe:

1. **Harness directory** with copies of the built assets:

   ```sh
   mkdir /tmp/webview-harness
   cp media/symbols/detail.{js,css} media/explorer/explorer.{js,css} /tmp/webview-harness/
   ```

2. **HTML page** that stubs the webview environment, loads the bundle, and
   posts host messages. Skeleton:

   ```html
   <style>
     :root { /* stub the --vscode-* variables the CSS consumes:
                fonts, foreground/background, panel-border, charts-*  */ }
     html, body, #root { height: 100%; }
   </style>
   <link rel="stylesheet" href="detail.css">
   <body class="vscode-light">
   <div id="root"></div>
   <script>
     window.__sent = [];
     window.acquireVsCodeApi = () => ({
       postMessage: (m) => window.__sent.push(m),
       getState: () => null,
       setState: () => {},
     });
   </script>
   <script src="detail.js"></script>
   <script>
     // post the exact message the extension host would send
     setTimeout(() => window.postMessage({ type: "detail", payload }, "*"), 150);
   </script>
   ```

   Payload shapes come from the protocol modules
   (`src/symbols/detail/panel.ts`, `src/explorer/protocol.ts`). To exercise a
   request/response flow (e.g. code insets), poll `window.__sent` for the
   outgoing message and answer it with the host-shaped response.

3. **Serve and drive** with any static server plus Puppeteer: navigate,
   simulate the user's real gestures (`page.$('.fncard').click()`),
   screenshot, and audit computed styles:

   ```js
   const audit = await page.evaluate(() => {
     const meta = document.querySelector(".meta");
     return {
       cols: getComputedStyle(meta).gridTemplateColumns,
       h: Math.round(meta.getBoundingClientRect().height),
     };
   });
   ```

   Numbers make regressions objective: the meta-grid bug read as
   `cols: "888px 15px", h: 638` before the fix and `"45px 819px", h: 83`
   after.

4. **Read the screenshot before shipping.** The audit numbers verify the
   fix; the image verifies you did not break the rest of the page.

## Probing the Daemon Like the Extension Does

When the UI misbehaves, decide first whether the data layer is at fault. The
daemon speaks JSON-RPC over WebSocket; registry entries (endpoint, pid,
workspace root) live in `$TMPDIR/code-moniker-daemons/*.json`. The query wire
shape is the extension's exactly:

```js
call("moniker_handshake", ["probe"]);
call("moniker_query", [{
  query: { op: "identity_graph", workspace: null, prefix: "" },
  consistency: "stale_ok",
  page: { cursor: null, limit: 200 },
}]);
```

Two hard-won rules:

- **Separate protocol compatibility from capabilities.** `protocol_version`
  guards the wire shape and must match exactly. Mismatch handling is
  direction-aware: an *older* daemon is recycled once (it likely predates a
  binary upgrade); a *newer* daemon is left running (it serves up-to-date CLI/
  MCP clients and relaunching cannot help). A second mismatch — or any
  newer-daemon mismatch — is an installation error: the session enters a
  sticky `protocolFault` state where `connectOrStart` fails fast, so nothing
  restarts daemons in a loop. Only the explicit reconnect command clears the
  fault. Capabilities guard verb availability: a long-running daemon can
  predate a query verb while reporting the same package version string, so
  the extension still gates the explorer with
  `session.supportsQuery("identity.graph")`.
- **Suspect other workspaces' daemons.** Every open project (fixtures,
  sibling repos) registers its own daemon. A stale one elsewhere reproduces
  "it works here, fails there" perfectly.

## Golden Rules

- Never certify UI behavior without a successful Playwright journey and its
  rendered evidence; a green compile proves nothing about clicks or layout.
- Screenshot the harness *after* the async chain settles: the explorer waits
  for the graph, its container outlines and the ELK pass before the canvas
  has nodes, and a capture taken earlier shows an empty canvas that is not a
  bug. Container outlines ride in the `scope` message precisely because they
  decide card heights — shipping them later would run ELK twice.
- A node card must show what it contains. Making the user dive to discover a
  class's members is the failure the scoped graph exists to avoid.
- Overlays anchor to a fixed corner of the canvas, never to the click point:
  an anchored panel overflows the viewport and covers what it describes.
- One data shape per rendering surface, unwrapped in one place.
- Shared components own their styles (`src/webview-lib/code.css` imported by
  `CodeBlock.tsx`): duplicated CSS *will* diverge.
- Errors posted to a webview before it mounts are lost; store them and
  replay on the `ready` handshake.
