# Performance

Wall-clock measurements of `code-moniker check`, release build,
warm OS file cache (the second run on a given path). Hardware:
Apple silicon laptop, 10 cores.

## Single file — agent hook latency

`code-moniker check <file>` is what a Claude Code `PostToolUse` hook
runs after each edit. Cost is dominated by process startup, not by
the extractor.

| File                                      | Bytes  | Time   |
|-------------------------------------------|--------|--------|
| `crates/cli/src/check/eval.rs`            | 62 KB  | 14 ms  |
| `crates/core/src/lang/ts/walker.rs`       | 18 KB  | 6 ms   |
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

The bare `code-moniker <dir>` probe (summary or filtered list) shares
the same walker + rayon pool, so wall-time tracks the table above
within ±10 ms. Summary is marginally faster (no rule eval); filter
mode does the same extraction with a kind/predicate sieve over the
graph, dominated by the extractor like `check`.

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
- Throughput varies 8–28 MB/s depending on language and density.
  TypeScript with heavy JSX is the slowest, Rust the fastest.

## Reproduce

```sh
cargo build --release -p code-moniker --bin code-moniker
./scripts/dogfood.sh --reset            # clones the panel into ./dogfood/
time ./target/release/code-moniker check dogfood/ts/date-fns
```
