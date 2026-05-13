#!/usr/bin/env bash
# Architecture self-check: build `code-moniker` and lint the whole repo
# (every Rust source under `.gitignore`-aware walk) against the embedded
# rule set plus `.code-moniker.toml`. Used by `.githooks/pre-commit` and
# the CI `arch-check` job.
#
# Exit 0 = clean, 1 = at least one violation (or per-file read error),
# 2 = usage error (binary missing, bad path, malformed user TOML).

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

exec cargo run --quiet -p code-moniker --bin code-moniker -- check .
