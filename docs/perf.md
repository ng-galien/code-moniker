# Performance

Wall-clock measurements of `code-moniker check`, release build,
warm OS file cache (the second run on a given path). Hardware:
Apple silicon laptop, 10 cores.

## Single file — agent hook latency

`code-moniker check <file>` is what a Claude Code `PostToolUse` hook
runs after each edit. Cost is dominated by process startup, not by
the extractor.

| File                              | Bytes  | Time   |
|-----------------------------------|--------|--------|
| `src/cli/check/eval.rs`           | 62 KB  | 14 ms  |
| `src/lang/ts/walker.rs`           | 18 KB  | 6 ms   |
| `src/core/moniker/mod.rs`         | 3 KB   | 4 ms   |

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

The cold-cache run on this repo (`check src/`, first invocation
after dropping the OS file cache) takes ~500 ms; subsequent runs
return to 25 ms.

## Implications

- `check src/` is fast enough to gate every commit and every CI
  job up to several thousand source files in well under a second.
- Per-file mode in a `PostToolUse` hook adds <15 ms of latency to
  an agent edit cycle.
- Throughput varies 8–28 MB/s depending on language and density.
  TypeScript with heavy JSX is the slowest, Rust the fastest.

## Reproduce

```sh
cargo build --release --features cli --bin code-moniker
./scripts/dogfood.sh --reset            # clones the panel into ./dogfood/
time ./target/release/code-moniker check dogfood/ts/date-fns
```
