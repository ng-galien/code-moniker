# Performance

Wall-clock measurements of `code-moniker check` / `code-moniker stats`,
release build, warm OS file cache unless stated. Hardware: MacBook Pro,
Apple M2 Pro, 10 CPU cores (6 performance, 4 efficiency), 16 GB RAM,
macOS 26.2, arm64.

## Single file — agent hook latency

`code-moniker check <root> --file <file>` is what generated agent hooks run
after each edit. It keeps project-mode moniker anchors but skips the full tree
extraction of every project file. When a tool touches several source files,
the hook repeats `--file`; when it touches no supported source file, it exits
without running a broad scan.

The table below measures the scripts produced by the installed 0.5.0 binary
on this repository on 2026-07-26. Each row is 40 warm-cache runs after five
warmups. The repository has 659 supported source files and 89 effective Rust
rules.

| Installed hook payload | Mean | p50 | p95 |
| --- | ---: | ---: | ---: |
| Codex, no touched source file | 23.1 ms | 20.7 ms | 31.0 ms |
| Codex, `crates/cli/src/mcp/tools/mod.rs` | 216.8 ms | 215.4 ms | 225.0 ms |
| Codex, `crates/cli/src/args.rs` | 267.5 ms | 267.9 ms | 276.0 ms |
| Claude, `crates/cli/src/args.rs` | 273.5 ms | 272.4 ms | 287.7 ms |

The generated shell wrapper is not the bottleneck. A direct project-mode
check of `args.rs` averages 249.2 ms, versus 267.5 ms through the Codex hook.
Relevant probes on the same binary are:

| Probe | Mean |
| --- | ---: |
| Extract `args.rs` directly with `stats` | 17.8 ms |
| Load and compile the project rules with `rules show` | 70.0 ms |
| Check `args.rs` as a standalone file | 95.1 ms |
| Check `args.rs` in project mode with `--file` | 249.2 ms |

Project-mode lazy evaluation now builds a cheap file-root moniker catalogue
and extracts only descendant candidates requested by a reached rule. It does
not extract the complete repository. The remaining project-mode delta is
still material: catalogue discovery, candidate selection/extraction, and the
89-rule evaluation add roughly 154 ms over the standalone-file probe for
`args.rs`.

The descendant extraction path does use Rayon. On
`crates/cli/src/mcp/tools/mod.rs`, fixing the Rayon pool to 1, 2, 4, and 8
threads produced respectively 265.4, 234.2, 207.0, and 206.9 ms. The work
scales through four threads and then saturates on this corpus. This rules out
a missing parallel iterator as the current bottleneck.

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

## Daemon lifecycle at scale — Apache Pulsar fork

The tables above are the parser/extractor path (`stats`/`check`, no daemon).
This section measures the daemon lifecycle an agent actually depends on:
cold start, incremental refresh, and query latency, on a real multi-module
Maven monorepo — local fork of Apache Pulsar (`~/dev/projects/fork/pulsar`).
Same machine as above, release binary (`cargo install`), warm OS cache.
4193 recognised files (mostly Java, some Go/Python/TS), 215 343 symbols,
1 066 335 references.

| Measurement | Value |
|---|---|
| Full census, no daemon (`code-moniker stats .`) | 3.64 s |
| Daemon cold start → initial index ready | ~13-14 s |
| Daemon RSS after initial index | ~208 MB |
| `identity.graph` on the largest scope (43 386-def `srcset:main/lang:java`) | 0.36 s |
| `workspace.status` round-trip (warm) | ~0.1 s |
| Incremental refresh after touching 1 file (`--live-refresh auto`) | ~0.5-0.8 s |

No wall is visible at this scale: query and incremental-refresh latency stay
sub-second at roughly 5x the reference/symbol count of the other corpora in
this document. Cold start is the only cost that scales with workspace size —
still under 15 s for a 1M-reference monorepo.

Resolution coverage is scope-dependent, not a flat number. On
`srcset:main/lang:java`, unlinked decomposition shows ~93 951 unresolved
(`no_candidate`) refs against ~92 468 external and 110 932 correctly-resolved
incoming test→main edges — a materially higher unresolved rate than the
`trust` corpus (0.44%, see `evolutions/resolution-coverage-diagnostic.md`,
local). This fork makes heavy use of Lombok (1730 files import it, 1601
`@Data`/`@Builder`/`@Getter`/`@Setter`/`@Slf4j` usages), which is strong
empirical evidence for the R4 backlog item (Java untyped receiver + Lombok
accessor synthesis).

## Cache (`--cache <DIR>`)

The cache stores `(path, mtime, size, anchor) -> encoded graph` on
disk. The single source of truth is `core::code_graph::encoding`.

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

## Workspace memory

Use the workspace memory bench before choosing memory optimizations. It loads
the same source catalog, semantic index, linkage, and change overlay used by
the workspace facade, then reports native process RSS after each phase plus a
model-level retained heap estimate by domain structure.

```sh
cargo run --release -p code-moniker-workspace --example bench_memory -- <path>
```

Useful variants:

```sh
cargo run --release -p code-moniker-workspace --example bench_memory -- --lang java <path>
cargo run --release -p code-moniker-workspace --example bench_memory -- --skip-changes <path>
cargo run -p code-moniker-workspace --features heap-profile --example bench_memory -- <path>
```

`rss_mib` is the observed resident set. `estimated_heap_mib` is deliberately a
lower bound over retained workspace data: snapshot records, index material,
code graphs, source text, linkage edges, and change records. A large gap
between both numbers means the next investigation should use the heap-profile
feature or Instruments/heaptrack rather than guessing from record counts.

## Implications

- `check crates/` is fast enough to gate every commit and every CI
  job up to several thousand source files in well under a second.
- A no-file hook callback costs about 23 ms. A project-mode file check on this
  repository currently costs 217-274 ms through the installed Codex/Claude
  scripts. This is correct and bounded, but it is not yet a sub-100-ms edit
  loop.
- Direct extraction of the edited Rust file is only about 18 ms. The next
  performance work should target repeated rule/config discovery and the lazy
  project catalogue/candidate path, not the generated shell script and not
  Rayon enablement.
- Large generated or multi-language repositories are a different tier:
  full-root scans can take several seconds. Scope hooks to changed files
  or active modules, and reserve full-root checks for explicit review/CI
  runs.
- Throughput varies 8-28 MB/s depending on language and density.
  TypeScript with heavy JSX is the slowest, Rust the fastest.

## Reproduce

```sh
cargo build --release -p code-moniker --bin code-moniker
git clone --depth 1 https://github.com/date-fns/date-fns.git /tmp/date-fns
time ./target/release/code-moniker check /tmp/date-fns
```
