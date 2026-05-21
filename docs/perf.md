# Performance

Wall-clock measurements of `code-moniker check` / `code-moniker stats`,
release build, warm OS file cache unless stated. Hardware: MacBook Pro,
Apple M2 Pro, 10 CPU cores (6 performance, 4 efficiency), 16 GB RAM,
macOS 26.2, arm64.

## Single file — agent hook latency

`code-moniker check <file>` is what a Claude Code `PostToolUse` hook
runs after each edit. Cost is dominated by process startup, not by
the extractor.

| File                                      | Bytes  | Time   |
|-------------------------------------------|--------|--------|
| `crates/cli/src/check/eval.rs`            | 62 KB  | 14 ms  |
| `crates/core/src/lang/ts/strategy.rs`     | 43 KB  | 6 ms   |
| `crates/core/src/core/moniker/mod.rs`     | 3 KB   | 4 ms   |

## Project scan

`code-moniker check <dir>` walks the tree (respecting `.gitignore`
via the `ignore` crate) and processes recognised extensions in
parallel via `rayon`. The first column is files matching a
supported extension.

| Project          | Lang  | Files | Bytes    | Time   | Throughput          |
|------------------|-------|------:|---------:|-------:|---------------------|
| date-fns         | ts    |  1410 | 2475 KB  | 278 ms | 5070 files/s, 8.7 MB/s  |
| clap             | rs    |   343 | 2359 KB  |  87 ms | 3940 files/s, 26 MB/s   |
| gson             | java  |   249 | 1782 KB  |  91 ms | 2740 files/s, 19 MB/s   |
| zod              | ts    |   169 |  930 KB  |  55 ms | 3070 files/s, 17 MB/s   |
| commandline      | cs    |   190 |  873 KB  |  53 ms | 3580 files/s, 16 MB/s   |
| httpx            | py    |    61 |  572 KB  |  40 ms | 1500 files/s, 14 MB/s   |
| bytes            | rs    |    33 |  289 KB  |  21 ms | 1570 files/s, 13 MB/s   |
| mux              | go    |    16 |  202 KB  |  26 ms |  615 files/s, 7.6 MB/s  |
| code-moniker     | rs    |    96 |  708 KB  |  25 ms | 3840 files/s, 28 MB/s   |

The cold-cache run on this repo (`check crates/`, first invocation
after dropping the OS file cache) takes ~500 ms; subsequent runs
return to 25 ms.

The bare `code-moniker extract <dir>` probe (summary or filtered list) shares
the same walker + rayon pool, so wall-time tracks the table above
within ±10 ms. Summary is marginally faster (no rule eval); filter
mode does the same extraction with a kind/predicate sieve over the
graph, dominated by the extractor like `check`.

## Java ratios on local forks

The table below uses Java-only scopes from local forks. The reported time is
`code-moniker stats --format json`, so it measures walking, parsing, and graph
extraction without rule-specific differences. LOC is a physical line count
over `.java` files. Records are `defs + refs`.

| Corpus / scope | Files | LOC | Records | Time | ms/KLOC | KLOC/s | Records/LOC | M records/s |
|----------------|------:|----:|--------:|-----:|--------:|-------:|------------:|------------:|
| OpenAPI Maven plugin | 1 | 1158 | 1360 | 13 ms | 11.23 | 89 | 1.17 | 0.10 |
| OpenAPI CLI | 15 | 2615 | 4471 | 21 ms | 8.03 | 125 | 1.71 | 0.21 |
| RSQL JPA Specification | 36 | 2901 | 5852 | 27 ms | 9.31 | 107 | 2.02 | 0.22 |
| OpenAPI Core | 34 | 5115 | 4757 | 21 ms | 4.11 | 244 | 0.93 | 0.23 |
| Pulsar Common | 286 | 38986 | 45104 | 88 ms | 2.26 | 443 | 1.16 | 0.51 |
| Pulsar Client | 258 | 47689 | 64864 | 112 ms | 2.35 | 426 | 1.36 | 0.58 |
| Pulsar Broker | 624 | 143024 | 210551 | 248 ms | 1.73 | 577 | 1.47 | 0.85 |
| OpenAPI Generator module | 347 | 163050 | 272252 | 242 ms | 1.48 | 674 | 1.67 | 1.13 |
| Pulsar `src/main/java` aggregate | 2562 | 462838 | 604203 | 785 ms | 1.70 | 590 | 1.31 | 0.77 |

Observed Java behaviour on this machine:

- Below ~10 KLOC, the process/walk overhead dominates. The ratio is noisy:
  4.1-11.2 ms/KLOC.
- From ~40 KLOC to ~463 KLOC, the ratio is much steadier: 1.48-2.35 ms/KLOC.
- The larger Java scopes process 426-674 KLOC/s and 0.51-1.13 M records/s.
- No explosive curve is visible in this Java sample up to 463 KLOC / 604k
  records. The ratio improves once the fixed cost is amortized, then stays in
  the same order of magnitude.
- Record density is material: the measured Java scopes range from 0.93 to
  2.02 records/LOC, so LOC alone is not enough to predict exact time.

For context, the full OpenAPI Generator checkout is not a Java-only ratio:
it is generated and polyglot. It produced 29383 supported files, about 40 MB
of recognised source, and 4.36 M graph records. Repeated warm-cache `stats`
runs were around 7-8 s.

## Cache (`--cache <DIR>`)

The cache stores `(path, mtime, size, anchor) → encoded graph` on
disk. Same encoding as the PG extension's `code_graph` Datum (single
source of truth: `core::code_graph::encoding`).

Measured on date-fns (1410 ts files, M1, warm CPU cache, best of 3):

| Scenario                                       | Wall    |
|------------------------------------------------|---------|
| No cache, cold OS                              | 0.84 s  |
| Cache **populating** (first run, all writes)   | 2.77 s  |
| Cache all hits, cold OS page cache             | 0.98 s  |
| Cache all hits, warm OS page cache             | ~0.20 s |
| **Agent edit (1 file changed, 1409 hits)**     | **0.20 s** |

Cache size: ~7 KB per file (10 MB total for 1410 files).

The win is concentrated in the agent-edit cycle: the hook fires after
each file edit, the toolchain re-scans, and only one file misses while
the rest are hits served from the OS page cache (warm). For ad-hoc
single-run scans, the cache hurts more than it helps — leave it off.

## Implications

- `check crates/` is fast enough to gate every commit and every CI
  job up to several thousand source files in well under a second.
- Per-file mode in a `PostToolUse` hook adds <15 ms of latency to
  an agent edit cycle.
- Large generated or multi-language repositories are a different tier:
  full-root scans can take several seconds. Scope hooks to changed files
  or active modules, and reserve full-root checks for explicit review/CI
  runs.
- Throughput varies 8-28 MB/s depending on language and density.
  TypeScript with heavy JSX is the slowest, Rust the fastest.

## Reproduce

```sh
cargo build --release -p code-moniker --bin code-moniker
./scripts/dogfood/run.sh ingest --reset # clones the panel into ./dogfood/
time ./target/release/code-moniker check dogfood/ts/date-fns
```
