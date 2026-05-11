#!/usr/bin/env bash
# Architecture self-check: build `code-moniker` and lint this repo's own
# `src/` against the embedded rule set (plus `.code-moniker.toml` overlay
# if present). Used by `.githooks/pre-commit` and the CI `arch-check` job.
#
# Exit 0 = clean, 1 = at least one violation (or per-file read error),
# 2 = usage error (binary missing, bad path, malformed user TOML).

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

exec cargo run --quiet --features cli --bin code-moniker -- check src/
