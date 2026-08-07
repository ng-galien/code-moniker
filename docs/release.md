# Release and binary distribution

Starting with `0.6.0`, one `vX.Y.Z` tag drives both crates.io publication and
precompiled CLI distribution. The public install paths are:

```sh
# Recommended: no Rust toolchain.
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/ng-galien/code-moniker/releases/latest/download/code-moniker-installer.sh | sh

# Rust ecosystem alternative, using the same GitHub release artifacts.
cargo binstall code-moniker

# Source-build fallback and custom feature selection.
cargo install code-moniker --features mcp
```

## Distribution contract

`cargo-dist` is pinned in `dist-workspace.toml` and owns the generated
`.github/workflows/v-release.yml`. Re-run `dist init` after changing the dist
version or configuration; do not hand-edit the generated workflow.

The `v` tag namespace deliberately excludes VS Code extension tags such as
`extension-v0.2.0`, which have their own release workflow.

The release contains binaries with MCP support for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-unknown-linux-gnu`

Windows stays excluded while the workspace daemon is Unix-only. The generated
release contains one archive and SHA-256 checksum per target, the universal
`code-moniker-installer.sh`, the dist manifest, and source metadata. GitHub
artifact attestations remain enabled.

`cargo-binstall` discovers the target-triple archives through the repository
metadata published with the `code-moniker` crate. Its crate metadata disables
third-party QuickInstall artifacts and implicit source compilation: an
unsupported platform fails explicitly.

### One-time grammar crate bootstrap

Trusted Publishing cannot allocate a new crates.io package name. Before the
first release containing `code-moniker-tree-sitter-plpgsql`:

1. From the exact validated `main` commit, publish
   `code-moniker-tree-sitter-plpgsql` manually with a crates.io account token.
2. On crates.io, configure its Trusted Publisher for repository
   `ng-galien/code-moniker`, workflow `publish-crates.yml`, and environment
   `release`.
3. Confirm that the published package version matches the intended release,
   then push the release tag.

The release workflow fails early with bootstrap guidance while the package name
does not exist. For the bootstrap release, it detects the manually published
version and skips it; subsequent versions use the same OIDC path as the other
workspace crates.

The dist workflow follows five gates:

1. `plan` validates the tag, package, targets, features, and artifacts.
2. `build-local` compiles and archives each target independently.
3. `build-global` creates checksums, manifests, and the shell installer.
4. `host` consolidates the release artifacts without making the GitHub release
   public yet.
5. `publish` calls `.github/workflows/publish-crates.yml`, which verifies the
   tag and publishes the eight crates in dependency order through crates.io
   OIDC. `announce` creates the GitHub release only after that job succeeds.

## `0.6.0` acceptance checklist

- [ ] `main` is clean, CI is green, and no `v0.6.0` tag exists.
- [ ] All workspace crates that are published share version `0.6.0`.
- [ ] `code-moniker-tree-sitter-plpgsql@0.6.0` has completed the one-time
      manual bootstrap and its Trusted Publisher is configured before tagging.
- [ ] `dist plan --tag=v0.6.0` lists exactly the three supported targets,
      `code-moniker-installer.sh`, and a `code-moniker` build with `mcp`.
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --quiet`
- [ ] `cargo clippy --workspace --tests --no-deps -- -D warnings`
- [ ] `cargo test -p code-moniker --features mcp --no-default-features --lib`
- [ ] From `packages/client/`: `npm ci --ignore-scripts`, `npm test`, then
      `npm run test:daemon -- <daemon-endpoint> <workspace-root>` and
      `npm run test:daemon:owned -- <code-moniker-binary>`.
- [ ] From `vscode-extension/`: `npm test`, `npm run compile`, then
      `npm run test:integration`.
- [ ] Push `v0.6.0` only after the preceding gates pass.
- [ ] Confirm the Release workflow completes through `announce` and that all
      eight crates exist on crates.io at `0.6.0`.
- [ ] On clean macOS and Linux environments, exercise the direct installer and
      `cargo binstall code-moniker --version 0.6.0`.
- [ ] Run `code-moniker --version`, `code-moniker mcp --help`, and an agent
      skill/MCP install smoke test.
- [ ] Publish `@code-moniker/client@0.6.0` from the verified package and confirm
      that a clean ESM and CommonJS consumer can install it from npm.
- [ ] Verify every archive checksum and GitHub attestation.
