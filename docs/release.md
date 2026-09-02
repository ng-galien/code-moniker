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
cargo install code-moniker
```

## Distribution contract

`cargo-dist` is pinned in `dist-workspace.toml` and owns the generated
`.github/workflows/v-release.yml`. Re-run `dist init` after changing the dist
version or configuration; do not hand-edit the generated workflow.

The `v` tag namespace deliberately excludes VS Code extension tags such as
`extension-v0.2.0`, which have their own release workflow.

The `vX.Y.Z` release gate covers the Rust workspace, CLI, daemon, MCP, reusable
Node.js client, native npm runtimes, and distribution builds. In `.github/workflows/ci.yml`,
the required jobs are `lint-and-unit`, `typescript-client`, `windows-runtime`,
and `linux-static-runtime`. Extension acceptance and VSIX packaging gate only
the separate `extension-vX.Y.Z` release workflow.

The release contains binaries with MCP support for:

- `aarch64-apple-darwin`
- `x86_64-apple-darwin`
- `x86_64-pc-windows-msvc`
- `x86_64-unknown-linux-gnu`
- `x86_64-unknown-linux-musl`

The generated release contains one archive and SHA-256 checksum per target, the
universal `code-moniker-installer.sh`, the dist manifest, and source metadata.
GitHub artifact attestations remain enabled.

Every artifact that embeds the CLI binary carries `THIRD_PARTY_NOTICES`: the
cargo-dist archives, the four native npm packages, and each platform-specific
VSIX. Packaging checks reject native npm packages and VSIX files that omit the
notice.

`cargo-binstall` discovers the target-triple archives through the repository
metadata published with the `code-moniker` crate. Its crate metadata disables
third-party QuickInstall artifacts and implicit source compilation: an
unsupported platform fails explicitly.

## npm client and native binaries

Installing `@code-moniker/client` installs one matching optional native package
and lets `@code-moniker/client/node` launch Code Moniker without a separate CLI
installation:

- `@code-moniker/cli-darwin-arm64`
- `@code-moniker/cli-darwin-x64`
- `@code-moniker/cli-linux-x64`
- `@code-moniker/cli-win32-x64`

The client resolves the packaged executable first and falls back to
`code-moniker` on `PATH`. All five npm package versions must exactly match the
release tag. `.github/workflows/publish-npm.yml` downloads each already-built,
attested cargo-dist archive and stages that exact executable in its native npm
package. This keeps the npm and GitHub Release binaries byte-identical and
avoids raising Linux's glibc floor through a second build on a newer runner.
The npm Linux package deliberately uses the statically linked musl artifact;
the GNU artifact remains available for the shell installer and `cargo-binstall`.
The publish job rejects a Linux npm executable with a dynamic interpreter and
runs it before publication. Before any native npm package is published, a
Windows runner also extracts the exact cargo-dist ZIP, runs the executable, and
launches it through a packed client install while checking the daemon's binary
fingerprint. The client is published only after all native packages succeed,
before release announcement.

### One-time npm registry bootstrap

npm requires every package to exist before a Trusted Publisher can be attached.
Bootstrap the namespace with a release candidate rather than the first stable
release:

1. Publish `v0.6.0-rc.1` as a GitHub prerelease. Registry publication is skipped
   for prerelease tags, but cargo-dist still builds and announces the exact
   five-platform artifact set. Run the packaged smoke checks before the manual
   npm bootstrap.
2. Sign in to npm with an account that can publish under `@code-moniker`, then
   publish the four native packages manually from those exact cargo-dist
   artifacts using dist-tag `next`. Publish `@code-moniker/client` last, also
   with dist-tag `next`. The initial scoped publishes must use `--access public`.
3. Configure each package's npm Trusted Publisher for repository
   `ng-galien/code-moniker`, workflow `v-release.yml`, environment `release`, and
   permission `npm publish`.
4. The stable `v0.6.0` workflow then publishes through OIDC and assigns npm's
   default `latest` dist-tag without any long-lived npm token.

With npm 11.15 or newer, step 3 can be performed for each of the five packages
from an authenticated, 2FA-enabled npm session:

```sh
npm trust github <package> \
  --repo ng-galien/code-moniker \
  --file v-release.yml \
  --env release \
  --allow-publish \
  --yes
```

The generated cargo-dist workflow is the caller of the reusable npm workflow,
so npm validates `v-release.yml`, not `publish-npm.yml`. The workflow uses Node.js
26, GitHub-hosted runners and `id-token: write`, as required by npm OIDC.

### One-time crates.io Trusted Publisher setup

Configure each of the seven crates with repository `ng-galien/code-moniker`,
workflow `v-release.yml`, and environment `release`. The generated cargo-dist
workflow is also the caller of the reusable crates workflow, so the crates.io
JWT contains `v-release.yml`, not `publish-crates.yml` or `release.yml`.

The dist workflow follows five gates:

1. `plan` validates the tag, package, targets, features, and artifacts.
2. `build-local` compiles and archives each target independently.
3. `build-global` creates checksums, manifests, and the shell installer.
4. `host` consolidates the release artifacts without making the GitHub release
   public yet.
5. `publish` calls `.github/workflows/publish-crates.yml` and
   `.github/workflows/publish-npm.yml`. They publish the seven crates and five
   npm packages through OIDC. `announce` creates the GitHub release only after
   both jobs succeed.

## Windows validation

Docker Desktop on macOS cannot provide a Windows-kernel acceptance test.
Windows containers use operating-system features on a Windows host. The CI
`windows-runtime` job therefore runs on `windows-2022`, matching cargo-dist's
release builder. It times native Git commands, exercises the release CLI's Git
failure categories, deadlines, and process cleanup, runs the Windows Node
supervision tests, launches an explicit `.exe` through the owned-daemon smoke
test, then installs the packed client plus native package in a clean consumer
and launches the resolved executable. Both lifecycle smokes index 512 generated TypeScript
files under deep paths containing spaces and non-ASCII characters, require
non-zero symbols and references, mutate a file after `ready`, require a stale
event and higher refreshed generation, then prove cold/warm stop and registry
cleanup. The PR job stages npm tarballs from a release-profile `.exe` and
retains environment and lifecycle logs. An empty temporary directory is not an
acceptable Windows lifecycle gate. The npm
publication workflow independently repeats the packaged smoke against the exact
Windows cargo-dist release artifact before it publishes any native package.

For faster local feedback on macOS, `cargo-xwin` can cross-compile the MSVC
target and can optionally execute tests through Wine. That is useful as a
compile/smoke gate, but the GitHub-hosted Windows VM remains the release
acceptance environment for process supervision, file locking and path behavior.

## `0.10.1` acceptance checklist

- [ ] `main` is clean, the four release-gate CI jobs listed above are green,
      and no `v0.10.1` tag exists.
- [ ] All workspace crates that are published share version `0.10.1`.
- [ ] The client and four native npm packages share version `0.10.1`.
- [ ] `dist plan --tag=v0.10.1` lists exactly the five supported targets,
      `code-moniker-installer.sh`, and a `code-moniker` build with `mcp`.
- [ ] `cargo fmt --all -- --check`
- [ ] `cargo test --workspace --quiet`
- [ ] `cargo clippy --workspace --all-targets --no-deps -- -D warnings`
- [ ] `cargo moniker-check`
- [ ] From `packages/client/`: `npm ci --omit=optional`,
      `npm test`, then
      `npm run test:daemon -- <daemon-endpoint> <workspace-root>` and
      `npm run test:daemon:owned -- <code-moniker-binary>`.
- [ ] A plain `cargo install code-moniker` exposes `code-moniker mcp --help`.
- [ ] `agent install --client codex` writes `required = false`, and an
      unavailable Code Moniker MCP does not prevent the agent session opening.
- [ ] Push `v0.10.1` only after the preceding gates pass.
- [ ] Confirm the Windows CI job installs the two npm tarballs in a clean
      consumer and completes the packaged owned-daemon smoke test.
- [ ] Confirm the Release workflow completes through `announce`, all seven
      crates exist on crates.io, and all five packages exist on npm at `0.10.1`.
- [ ] On clean macOS, Linux and Windows environments, exercise the direct
      installer and `cargo binstall code-moniker --version 0.10.1`.
- [ ] Run `code-moniker --version`, `code-moniker mcp --help`, and an agent
      skill/MCP install smoke test.
- [ ] Confirm clean ESM and CommonJS consumers install
      `@code-moniker/client@0.10.1` and receive the matching native package.
- [ ] Verify every archive checksum and GitHub attestation.
- [ ] Verify `THIRD_PARTY_NOTICES` is present in every cargo-dist archive and
      native npm package.
