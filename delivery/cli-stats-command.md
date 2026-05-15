# CLI Stats Command Delivery

## Goal

Add a `code-moniker stats` command that reports only extraction metrics,
without emitting graph rows or running architecture rules.

## Scope

- Count supported files by language for a file or directory.
- Aggregate def/ref totals, shape distribution, and concrete def/ref kind
  histograms.
- Report scan, extraction, and total wall-clock timings in milliseconds.
- Support compact TSV, machine-readable JSON, and colored tree output.
- Reuse existing `.gitignore`, project anchor, and cache behavior from
  extraction.

## Non-Goals

- No rule evaluation or violation reporting.
- No per-file graph output.
- No persistence of benchmark baselines.

## Validation

- TDD coverage in `crates/cli/tests/cli_e2e.rs` for TSV, JSON, tree, and
  read-error handling.
- Run targeted CLI tests, formatting, clippy, and architecture checks.
- Smoke-test on existing dogfood repositories such as `clap` and
  `date-fns`.
