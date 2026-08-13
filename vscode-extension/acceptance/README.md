# VS Code Playwright acceptance

This suite drives the real VS Code Electron workbench and the real Code Moniker
webview with Playwright. Product scenarios use TreeView rows, editor tabs,
Command Palette entries, webview buttons, pointer movement, wheel input and
keyboard input. They do not call extension feature APIs or panel handlers.

## Structure

- `fixtures/` owns the isolated profile, deterministic Rust workspace, CLI
  build, VS Code lifecycle, tracing, video and cleanup.
- `pages/` owns stable VS Code and Code Cockpit UI vocabulary.
- `specs/` describes complete Cockpit V3 user journeys.

The campaign is deliberately serial and fail-fast. Each scenario closes all
editor tabs, restores the 1440x900 native window and collapses the TreeView.
The worker keeps one VS Code process and one daemon to avoid replacing real
lifecycle behavior with per-test mocks.

## Evidence

Playwright retains traces, screenshots and video on failure under
`test-results/`. The inspector journey also attaches a successful full-window
screenshot so layout review is part of the acceptance artifact.

## Run

```bash
npm run test:acceptance
npm run test:acceptance:cockpit
```

Set `CODE_MONIKER_ACCEPTANCE_VSCODE_VERSION` to exercise a specific VS Code
version instead of the current stable runtime.
