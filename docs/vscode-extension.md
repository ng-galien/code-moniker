# VS Code Extension

The Code Moniker extension brings the rule workflow and daemon-backed workspace
navigation into VS Code.

Use it to browse local rule files, validate `.code-moniker.toml` fragments, run
checks on the opened workspace, inspect extracted symbols, and open executable
`.cm.md` learning scenarios from the bundled catalog.

## Requirements

- VS Code `1.120.0` or newer.

The beta extension runs the `code-moniker` binary. A platform-specific VSIX
embeds the matching CLI, so Rust, Cargo, Node.js, and npm are not end-user
requirements. An explicit `codeMoniker.binaryPath` overrides that bundled CLI;
source builds otherwise fall back to `code-moniker` on `PATH` and
`~/.cargo/bin/code-moniker`.

## Install a Beta VSIX

Download the VSIX matching the machine from the GitHub release assets:

- `darwin-arm64` or `darwin-x64` for macOS;
- `linux-x64` for Linux;
- `win32-x64` for Windows.

Each release also ships `SHA256SUMS` and GitHub build attestations. Verify a
download before installing it (use `shasum -a 256 -c SHA256SUMS` on macOS, or
`Get-FileHash` in PowerShell on Windows):

```sh
sha256sum -c SHA256SUMS
gh attestation verify code-moniker-<extension-version>-<platform>.vsix \
  --repo ng-galien/code-moniker
code --install-extension code-moniker-<extension-version>-<platform>.vsix
```

The platform VSIX is beta: it provides the current daemon-backed workflows,
while extractor maturity remains language-specific as documented in the root
README.

## Install From Sources

From the repository root:

```sh
cargo install --path crates/cli --features tui,mcp
cd vscode-extension
npm ci
npm run package
code --install-extension code-moniker-0.1.0.vsix
```

Restart VS Code, or run **Developer: Reload Window**, after installing a new
build.

`npm run package` runs the extension build before producing the `.vsix`.

If your local package version changes, the generated `.vsix` filename follows
the `version` field in `vscode-extension/package.json`.

## Use The Extension

Open a repository in VS Code and select the **Code Moniker** activity bar item.

- **Workspace** shows rule files, daemon sessions, extracted symbols, and check
  results for the current workspace.
- **Catalog** opens bundled learning and sample scenarios as editable `.cm.md`
  notebooks.
- `.code-moniker.toml` and `*.fragment.toml` files get Code Moniker rule syntax
  highlighting.
- `.cm.md` files open with the Code Moniker scenario notebook renderer.

Useful commands from the Command Palette:

- **Code Moniker: Connect Workspace Daemon**
- **Code Moniker: Refresh Daemons**
- **Code Moniker: Refresh Symbols**
- **Code Moniker: Run Check**
- **Code Moniker: Open Catalog Sample**
- **Code Moniker: Validate Rules**
- **Code Moniker: Run on Project**

To use the bundled file icons for rule files, select
**Preferences: File Icon Theme -> Code Moniker**.

## Configure The CLI Path

To override the embedded binary with a development or troubleshooting build,
set the extension setting:

```json
{
  "codeMoniker.binaryPath": "/absolute/path/to/code-moniker"
}
```

For a checkout install, the Cargo location is usually:

```json
{
  "codeMoniker.binaryPath": "~/.cargo/bin/code-moniker"
}
```

Disable automatic daemon startup when opening a folder with:

```json
{
  "codeMoniker.daemon.autoConnect": false
}
```

## Develop The Extension

For extension work:

```sh
cd vscode-extension
npm ci
npm run typecheck
npm run validate
npm test
npm run compile
```

Open the repository in VS Code and use the extension launch configuration, or
run the packaged `.vsix` flow above when you want to test the installed
extension exactly as a user would.

The extension bundle includes the scenario catalog from `samples/learn/` and
`samples/catalog/`; run `npm run compile` again after changing those files.
