# Observability

Code Moniker can export process traces through OpenTelemetry. The exporter is
compiled into the default CLI binary but remains disabled until explicitly
enabled:

```sh
CODE_MONIKER_TELEMETRY=true \
OTEL_EXPORTER_OTLP_ENDPOINT=http://127.0.0.1:4318 \
code-moniker daemon start .
```

The current exporter uses OTLP over HTTP with protobuf payloads. Standard
OpenTelemetry variables configure the endpoint and resource, including
`OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_TRACES_ENDPOINT`,
`OTEL_SERVICE_NAME`, and `OTEL_RESOURCE_ATTRIBUTES`. The default service name
is `code-moniker` and the package version is exported as `service.version`.

Telemetry is fail-open. Export runs on a dedicated batch thread with a bounded
queue; a missing, slow, or invalid collector does not turn a successful command
into a failure. Process shutdown attempts a bounded flush and then lets the
command exit. Invalid `CODE_MONIKER_TELEMETRY` values or exporter configuration
disable OTLP for that process and write a diagnostic to stderr. The first
asynchronous export failure is also reported locally; repetitions are
suppressed.

The trace model currently exposes three stable boundaries:

- `cli.command` records the command, its bounded request and outcome;
- `mcp.tool.call` records the tool name, bounded arguments, outcome and response
  volume;
- `daemon.request` records query/command type, operation, consistency, bounded
  payload, outcome and workspace generation.

Request and argument payloads are capped at 4096 Unicode characters. Context
propagation between processes, long-running operation links, OpenTelemetry
logs, performance metrics, and memory gauges are added by subsequent slices.

Build without the exporter when a minimal binary is required:

```sh
cargo build -p code-moniker --no-default-features
```
