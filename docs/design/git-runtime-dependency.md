# Git runtime dependency

Status: accepted.

## Decision

Code Moniker keeps the Git CLI as its Git backend. This is a deliberate product
boundary, not an implementation accident. Revision parsing, merge bases,
worktrees, configuration, rename detection, partial fetches and diff output must
retain the behavior of the Git installation that owns the repository.

A Rust replacement was evaluated below. A second backend is not introduced
until it can be proven against the same operation and platform matrix. The
current reliability boundary is a single, bounded CLI launcher.

## Backend evaluation

Evidence was refreshed on 2026-08-26 against the current [`gix`
documentation](https://docs.rs/gix/latest/gix/), its documented [feature
surface](https://docs.rs/crate/gix/latest/features), the [libgit2
reference](https://libgit2.org/docs/reference/main/), and each project's
published licensing. “API exists” below does not mean behavioral parity with
the repository owner's Git configuration.

| Code Moniker operation or semantic dependency | Git CLI | `gix` | `libgit2` / `git2` | consequence |
|---|---|---|---|---|
| discovery, worktree/bare state, HEAD, branch, rev-parse and merge-base | canonical behavior of the installed Git | corresponding repository, revision and merge-base APIs exist | corresponding repository and revision APIs exist | both libraries are plausible for local reads, but must pass linked-worktree, config and revspec differential tests first |
| status, untracked and ignored files | uses Git index, excludes and config directly | status, attributes and excludes are feature-gated; status exposes an interrupt flag | status API exists; pathspec-filtered rename results carry a documented accuracy warning | replacing this path changes more than process transport and needs corpus parity tests |
| tree/worktree diff, hunks and rename detection | current source of truth | blob/tree diff and rename tracking exist behind features | diff and similarity APIs exist | thresholds, pathspec behavior, binary classification and hunk boundaries are observable product data and cannot silently drift |
| blob reads and worktree conversion | `git show` follows the selected repository and object format | object reads and attribute/filter pipelines exist | blob and filter APIs exist | `.gitattributes`, CRLF, ident and custom clean/smudge drivers require explicit equivalence tests |
| worktree config, safe-directory and trust | same policy as the user's Git | has its own documented trust model and configuration filtering | has its own config and ownership behavior | security differences must be product decisions, not compatibility bugs hidden by fallback |
| submodules | Git status/diff semantics | submodule state/status APIs exist, with documented differences from `git2` status | submodule APIs exist | recursive, ignore and dirty-state behavior remain an unproved compatibility area |
| shallow and partial-clone fetch (`blob:none`) | already used by the Node adapter | network/protocol features exist, but this exact acquisition contract is not proven here | fetch depth is exposed; current fetch options do not expose the same partial-clone filter contract | remote acquisition stays on CLI until auth, proxy, promisor, missing-object and cancellation tests pass |
| cancellation and deadlines | enforced externally for every command by process group or Job Object | selected APIs expose interrupt controls; cancellation is otherwise operation-specific and in-process | callbacks can stop selected remote operations; cancellation remains operation-specific and in-process | an in-process backend needs a mandatory cooperative deadline contract for every used call |
| memory and concurrency isolation | bounded pipes; OS process owns transient Git memory | Rust memory/caches live in the Code Moniker process; thread-safe and thread-local repository forms exist | native allocations and global library state live in-process | replacements need peak-memory, parallel-root and cancellation-after-load measurements |
| packaging and supply chain | relies on the user's separately installed Git | pure-Rust dependency graph; MIT OR Apache-2.0 | C library plus Rust bindings; GPLv2 with linking exception | `gix` enlarges the Rust dependency graph; libgit2 adds native builds, CVE/update ownership and cross-compilation work |

The evaluation therefore rejects a backend switch for this issue, not the
libraries themselves. `gix` is the preferred candidate for a future local-read
experiment because it avoids a C toolchain; libgit2 remains a valid comparator
where its mature API is useful. Neither is currently a drop-in replacement for
the full local-and-remote contract above.

A future migration must be explicit and reversible:

1. introduce a backend-neutral fixture contract for each row above, including
   Windows paths with spaces, linked worktrees, attributes/ignore/filter
   behavior, submodules, shallow/partial clones and hostile configuration;
2. run the candidate only behind an explicit experimental backend selection
   and compare normalized outputs, failures, cancellation, peak memory and
   parallel-root behavior with the CLI;
3. publish any semantic delta as a product decision and version affected
   artifacts or protocols;
4. switch a complete capability only after its platform matrix is green. Never
   fall back silently per command or combine facts from two backends in one
   review, because that would make provenance and failures ambiguous;
5. retain the bounded CLI backend as the explicit rollback until the new
   backend has shipped successfully across the supported release matrix.

## Dependency contract

The daemon supports Git-backed capabilities, but Git is available only when the
current process has resolved and diagnosed a compatible executable. Supported
and available are separate facts.

Resolution order is intentionally narrow:

1. explicit `CODE_MONIKER_GIT_BINARY`, which must be an absolute executable
   path;
2. the exact `git` executable on Unix or `git.exe` on Windows found on the
   process's inherited `PATH`.

An invalid explicit path is an error and never falls back to `PATH`. Resolution
does not consult the Windows registry, conventional installation directories,
shell aliases, wrappers, or VS Code's `git.path`. The resolved path is
canonicalized and every later command launches that absolute path without a
shell.

The supported version range is `>=2.22.0`. The process-scoped diagnostic runs
`git --version` with a 2 second timeout and a 64 KiB output cap. Its complete
version-and-roots pass has a 4 second budget, so many roots cannot accumulate
per-command timeouts. It probes each selected root with bounded `rev-parse`
commands and records `worktree`,
`repository_only`, `not_repository`, or `unavailable`. The diagnostic reports:

- `checking`, `available`, `unavailable`, `incompatible`, or `timed_out`;
- resolution source, canonical executable path and version;
- supported range and compatibility;
- typed failure category and sanitized message;
- check timestamp, duration and per-root repository state.

The published `process_scope` is `daemon` for a detached daemon and
`stdio-worker` for the in-process MCP worker, because each owns a distinct
environment and executable resolution.

## Readiness and diagnostic gate

Workspace readiness depends only on publishing the initial index. The runtime
publishes the snapshot, lifecycle state and refreshed event before waking the
Git diagnostic task. The probe runs as blocking work outside the workspace
mutex. Missing or hanging Git therefore cannot hide an otherwise usable index.

The diagnostic remains mandatory for capabilities that launch Git. A request
made while no process-scoped result exists may wait only for the bounded probe;
it never starts an unbounded readiness loop. Failures use typed query errors:
`runtime_dependency_checking`, `runtime_dependency_unavailable`,
`runtime_dependency_incompatible`, `runtime_dependency_timed_out`, or
`git_worktree_unavailable`.

Operational failures and mutable root states, including `not_repository`, are
re-probed only after a one-second cooldown and only for the selected roots.
This keeps repeated non-Git queries fast while allowing a newly initialized
repository or a recovered executable to become usable without restarting the
daemon. Invalid explicit configuration, incompatible versions and malformed
probe output remain fail-closed for that process.

Effective capability mapping is explicit:

| capability | Git requirement | degraded behavior |
|---|---|---|
| `change.review` | compatible Git and a selected worktree | unavailable, or degraded across mixed roots |
| `change.context` | Git only for its change evidence | graph, notes and rules remain available; the change portion is empty and the capability is degraded |
| `metrics.coupling` | none for indexed facts | available but degraded when Git revision metadata is absent |
| `diff-impact.compare` | none in the daemon | always consumes caller-provided virtual source sets |

`change.context` never represents a failed refresh as “no changes”. Its response
always includes `change_dependency`; when the Git portion fails, that field
carries the selected-root provenance plus the typed failure while graph, notes
and rules remain usable. `change.review` instead fails closed before returning
review facts.

The Node `diffImpactGit` adapter is a separate client-side acquisition stage. It
uses the same resolution, version and process bounds before the daemon starts,
and records that client-side Git provenance in the versioned artifact. The
daemon-side comparison itself never launches Git.

## Command reliability

Every production Git operation passes through the same launcher. This includes:

- `--version` and repository/worktree probes;
- revision, branch and dirty-state reads;
- merge-base and revision verification;
- changed-file and rename discovery;
- untracked-file discovery;
- blob reads with `show`;
- Node remote repository initialization, configuration and bounded partial
  fetches.

Diagnostic commands and local metadata reads (`rev-parse`, branch and `status`)
have a 2 second timeout and 64 KiB output cap. Diff, history, blob and network
commands default to 30 seconds and 32 MiB per output stream. Standard input is
closed, stdout and stderr are drained concurrently, credentials are redacted
from surfaced failures, and `GIT_OPTIONAL_LOCKS=0` prevents read-only probes
from contending on optional repository locks. Unix launches use a dedicated
process group and Rust Windows launches use a kill-on-close Job Object, so the
Rust launcher kills the process tree and keeps the deadline active until both
output pipes close. On Windows the Node adapter never launches Git directly:
it invokes the hidden, versioned `__git-runtime` mode of the same packaged
Code Moniker binary selected for the owned daemon. That helper attaches Git to
the Job Object before resuming it, owns the captured pipes, and returns one
strict JSON envelope with bounded base64 stdout. A missing, old, partial or malformed
helper response fails closed; there is no direct-Node fallback.
The helper bypasses CLI telemetry so revision arguments and remote URLs never
enter a command-span payload.

The Rust command deadline remains authoritative. Node gives the helper one
additional second to return its envelope, then gives direct helper termination
at most one further second. Closing the helper closes its kill-on-close Job
Object and therefore terminates Git and its descendants. A normal Windows
timeout returns at the Git deadline; the exceptional helper-failure path remains
bounded by the command budget plus two seconds. Direct Unix cleanup has one
separate one-second ceiling. The Node adapter keeps the same fixed two-second
metadata budget and thirty-second general command budget; this reliability
contract is not exposed as a new public tuning option.

## Production process inventory

Git is the only optional third-party executable dependency launched by shipped
runtime behavior. Every production launch has one of these owners:

| owner | launched process | classification |
|---|---|---|
| `workspace::git_runtime` | resolved absolute Git executable | optional dependency |
| client `diff-impact::runDirectProcess` | resolved absolute Git executable on Unix; packaged Code Moniker supervisor on Windows | optional dependency or self-runtime, client process scope |
| CLI `git_runtime_supervisor` | resolved absolute Git executable | hidden protocol v1 helper; Windows Job Object owner for the client |
| `daemon-client::start_daemon_process` | Code Moniker daemon executable | self-runtime |
| client `node::tryLaunchDetached` | packaged Code Moniker daemon | self-runtime |
| CLI MCP supervisor | Code Moniker stdio worker | self-runtime |
| VS Code CLI runner | packaged Code Moniker CLI | self-runtime |

Hook executors, acceptance fixtures, schema/release scripts and installer smoke
tests are development or installation tooling, not runtime dependencies. Adding
another production `Command::new`, `spawn`, or equivalent launch requires this
inventory and the effective-capability mapping to be updated in the same change.

The structural rules
`workspace-git-call-flow-enters-bounded-runtime`,
`workspace-git-fast-metadata-uses-probe-budget`,
`workspace-git-ownership-process-launches-stay-in-runtime`,
`workspace-git-lifecycle-bounded-process-execution`,
`sdk-git-call-flow-enters-bounded-runtime`,
`sdk-git-fast-metadata-uses-probe-budget`,
`sdk-git-lifecycle-bounded-process-execution`,
`sdk-git-ownership-process-launches-stay-in-runtime`,
`sdk-git-lifecycle-windows-git-uses-native-supervisor`,
`daemon-git-lifecycle-diagnostic-waits-for-readiness`, and
`daemon-git-call-flow-gates-only-dependent-queries` make these boundaries
executable architecture.
