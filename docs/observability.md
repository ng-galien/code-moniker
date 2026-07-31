# Observability

Code Moniker can export traces, logs, and metrics through OpenTelemetry. The exporter is
compiled into the default CLI binary but remains disabled until explicitly
enabled:

```sh
CODE_MONIKER_TELEMETRY=true \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
code-moniker daemon start .
```

The current exporter uses OTLP over HTTP with protobuf payloads. Standard
OpenTelemetry variables configure the endpoint and resource, including
`OTEL_EXPORTER_OTLP_ENDPOINT`, the signal-specific endpoint variables,
`OTEL_SERVICE_NAME`, and `OTEL_RESOURCE_ATTRIBUTES`. The default service name
is `code-moniker` and the package version is exported as `service.version`.
`OTEL_METRIC_EXPORT_INTERVAL` controls the metric export period in milliseconds
(60 seconds by default; 5 seconds is convenient for local inspection).

Telemetry is fail-open. Export runs on a dedicated batch thread with a bounded
queue; a missing, slow, or invalid collector does not turn a successful command
into a failure. Process shutdown attempts a bounded flush and then lets the
command exit. Invalid `CODE_MONIKER_TELEMETRY` values or exporter configuration
disable OTLP for that process and write a diagnostic to stderr. The first
asynchronous export failure is also reported locally per signal; repetitions
are suppressed.

The trace model exposes stable operational boundaries:

- `cli.command` records the command, its bounded request and outcome;
- `mcp.tool.call` records the tool name, bounded arguments, outcome and response
  volume;
- `daemon.request` records query/command type, operation, consistency, bounded
  payload, outcome and workspace generation.
- `workspace.background_operation` is a detached trace for daemon and MCP
  preloads. It links back to the triggering trace so the long-running work does
  not pretend to be synchronous.
- `workspace.index` records full, stale, and live index operations, their
  sequence, generation delta, outcome, current sizes, RSS, and canonical phase
  timings as span events.

Request and argument payloads are capped at 4096 Unicode characters. Context
is propagated into blocking workers in-process. OTLP logs carry the existing
structured `tracing` events and their active trace/span identifiers.

The daemon publishes these performance metrics after every index operation;
request rate, latency, and RSS are also refreshed after every daemon request:

- `code_moniker.workspace.index.operation.duration` and
  `code_moniker.workspace.index.phase.duration` (milliseconds);
- `code_moniker.workspace.index.operations` and
  `code_moniker.workspace.generation` for repeated or duplicate indexing;
- `code_moniker.workspace.source.files` and
  `code_moniker.workspace.source.bytes`;
- `code_moniker.workspace.index.entries`, split by file, symbol, and reference;
- `code_moniker.workspace.index.estimated_bytes` and
  `code_moniker.workspace.graph.estimated_bytes`;
- `code_moniker.workspace.graph.references`, split by resolution state;
- `code_moniker.daemon.requests` and `code_moniker.daemon.request.duration`;
- `process.memory.rss` (bytes).

Source bytes and RSS are direct measurements. Index and graph byte metrics are
explicit estimates of the allocations visible from the immutable snapshot;
they are intended for baselines and regression comparisons, not allocator-level
accounting.

Telemetry is initialized once when each process starts. A long-running daemon
or MCP server keeps one provider and exports all subsequent operations. A
one-shot CLI command necessarily owns one short-lived provider. Enabling or
changing telemetry configuration requires restarting that process once, never
once per request.

Build without the exporter when a minimal binary is required:

```sh
cargo build -p code-moniker --no-default-features
```
