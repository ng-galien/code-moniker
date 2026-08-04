# Observability

Code Moniker can export traces, logs, and metrics through OpenTelemetry. The
project's `.code-moniker.toml` owns the normal exporter configuration:

```toml
[telemetry]
enabled = true
service_name = "code-moniker-daemon:my-project"
endpoint = "http://127.0.0.1:4318"
metric_export_interval_ms = 5000
```

`endpoint` is the OTLP/HTTP base URL; Code Moniker appends `/v1/traces`,
`/v1/metrics`, and `/v1/logs`. The exporter is compiled into the default CLI
binary but remains disabled unless the project enables it.

`service_name` identifies the project process in traces, logs, metrics, and
Grafana selectors. `OTEL_SERVICE_NAME` and a `service.name` entry in
`OTEL_RESOURCE_ATTRIBUTES` remain explicit operational overrides. Without any
of these settings, Code Moniker derives a service name from the command.

`CODE_MONIKER_TELEMETRY=true|false` is an explicit operational override of the
project switch. When the project does not set an endpoint or metric interval,
the standard OpenTelemetry variables remain available, including
`OTEL_EXPORTER_OTLP_ENDPOINT`, signal-specific endpoint variables,
`OTEL_METRIC_EXPORT_INTERVAL`, `OTEL_SERVICE_NAME`, and
`OTEL_RESOURCE_ATTRIBUTES`. Package version is exported as `service.version`;
service names distinguish daemon, MCP, and one-shot CLI processes.

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

Coupling measurements are exported only when both process telemetry and the
individual query opt in:

```text
metrics.coupling from:"lang:java/package:com/package:acme" to:"lang:java/package:com/package:storage" relation:calls snapshot:current export:true
```

The query records these gauges:

- `code_moniker.analysis.coupling.references` and
  `code_moniker.analysis.coupling.references_by_kind`;
- `code_moniker.analysis.coupling.references_by_target`, grouped by the
  `target.moniker` called on the selected boundary;
- `code_moniker.analysis.coupling.connections`;
- `code_moniker.analysis.coupling.source_symbols` and
  `code_moniker.analysis.coupling.target_symbols`;
- `code_moniker.analysis.coupling.same_symbol_references`;
- `code_moniker.analysis.coupling.source_references`, split by resolution
  state.

The per-target series carries `target.moniker`; because this is an
explicit query-level drill-down, callers should export it only for the bounded
component boundaries they intend to review. Same-scope measurements do not
emit per-target series.

All carry the normalized `coupling.from`, `coupling.to`, and
`coupling.relation` attributes, the explicit `metric.snapshot` comparison
label, and the automatically discovered `git.branch`, `git.commit`, and
`git.dirty` source coordinates. The per-kind and coverage series additionally
carry `reference.kind` and `reference.state`. Because the scope attributes form
the time-series identity, exported queries should use a bounded, stable set of
declared scopes. A successful `export_recorded` response confirms submission to
the active OTel SDK; delivery remains fail-open and asynchronous as described
above.

Source bytes and RSS are direct measurements. Index and graph byte metrics are
explicit estimates of the allocations visible from the immutable snapshot;
they are intended for baselines and regression comparisons, not allocator-level
accounting.

Telemetry is initialized once when each process starts. A long-running daemon
or MCP server keeps one provider and exports all subsequent operations. A
one-shot CLI command necessarily owns one short-lived provider. With MCP over
standard I/O, the reloadable worker owns the provider and the stable supervisor
does not export duplicate telemetry. Enabling or changing project telemetry
requires restarting the daemon or reloading the MCP worker once, never once per
request.

Build without the exporter when a minimal binary is required:

```sh
cargo build -p code-moniker --no-default-features
```
