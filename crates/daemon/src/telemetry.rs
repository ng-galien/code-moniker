use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use code_moniker_workspace::memory::SnapshotMemoryEstimate;
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use tracing::Span;

static INDEX_OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) fn detached_operation_span(operation: &'static str) -> Span {
	let span = tracing::info_span!(
		parent: None,
		"workspace.background_operation",
		operation.name = operation,
		operation.async = true,
	);
	link_current_span(&span, operation);
	span
}

pub(crate) fn index_operation_span(mode: &'static str, previous_generation: u64) -> Span {
	let sequence = INDEX_OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed) + 1;
	tracing::info_span!(
		"workspace.index",
		index.mode = mode,
		index.sequence = sequence,
		index.previous_generation = previous_generation,
		index.generation = tracing::field::Empty,
		index.generation_delta = tracing::field::Empty,
		index.result = tracing::field::Empty,
		index.files = tracing::field::Empty,
		index.symbols = tracing::field::Empty,
		index.references = tracing::field::Empty,
		index.source_bytes = tracing::field::Empty,
		index.estimated_bytes = tracing::field::Empty,
		graph.estimated_bytes = tracing::field::Empty,
		process.rss_bytes = tracing::field::Empty,
	)
}

pub(crate) fn finish_index_operation(
	span: &Span,
	mode: &'static str,
	previous_generation: u64,
	elapsed: Duration,
	succeeded: bool,
	snapshot: Option<&WorkspaceSnapshot>,
) {
	let outcome = if succeeded { "ok" } else { "error" };
	span.record("index.result", outcome);
	if let Some(snapshot) = snapshot {
		let measurements = WorkspaceMeasurements::from_snapshot(snapshot);
		let generation = snapshot.generation.value();
		let generation_changed = generation > previous_generation;
		span.record("index.generation", generation);
		span.record(
			"index.generation_delta",
			generation.saturating_sub(previous_generation),
		);
		span.record("index.files", measurements.files);
		span.record("index.symbols", measurements.symbols);
		span.record("index.references", measurements.references);
		span.record("index.source_bytes", measurements.source_bytes);
		span.record("index.estimated_bytes", measurements.index_bytes);
		span.record("graph.estimated_bytes", measurements.graph_bytes);
		if let Some(rss_bytes) = measurements.rss_bytes {
			span.record("process.rss_bytes", rss_bytes);
		}
		record_metrics(
			mode,
			outcome,
			elapsed,
			snapshot,
			measurements,
			generation_changed,
		);
		if generation_changed {
			record_phase_events(snapshot);
		}
		tracing::info!(
			index.mode = mode,
			index.result = outcome,
			index.generation = generation,
			index.files = measurements.files,
			index.symbols = measurements.symbols,
			index.references = measurements.references,
			index.source_bytes = measurements.source_bytes,
			index.estimated_bytes = measurements.index_bytes,
			graph.estimated_bytes = measurements.graph_bytes,
			process.rss_bytes = measurements.rss_bytes,
			"workspace index operation completed"
		);
	} else {
		record_operation_metric(mode, outcome, elapsed);
		tracing::warn!(
			index.mode = mode,
			index.result = outcome,
			"workspace index operation completed without a published snapshot"
		);
	}
}

fn record_phase_events(snapshot: &WorkspaceSnapshot) {
	for (phase, duration) in [
		("source_catalog", snapshot.timings.source_catalog),
		("extract_sources", snapshot.timings.extract_sources),
		("semantic_index", snapshot.timings.semantic_index),
		("code_index", snapshot.timings.code_index),
		("linkage", snapshot.timings.linkage),
		("change_overlay", snapshot.timings.change_overlay),
		("total", snapshot.timings.total),
	] {
		tracing::info!(
			index.phase = phase,
			index.phase.duration_ms = duration.as_secs_f64() * 1_000.0,
			"workspace index phase measured"
		);
	}
}

pub(crate) fn record_daemon_request(
	kind: &'static str,
	operation: &'static str,
	result: &'static str,
	elapsed: Duration,
) {
	#[cfg(feature = "telemetry")]
	{
		use opentelemetry::KeyValue;

		let attributes = [
			KeyValue::new("request.kind", kind),
			KeyValue::new("request.operation", operation),
			KeyValue::new("request.result", result),
		];
		let metrics = metrics();
		metrics.daemon_requests.add(1, &attributes);
		metrics
			.daemon_request_duration
			.record(elapsed.as_secs_f64() * 1_000.0, &attributes);
		if let Some(rss_bytes) = process_rss_bytes() {
			metrics.process_rss_bytes.record(rss_bytes, &[]);
		}
	}
	#[cfg(not(feature = "telemetry"))]
	let _ = (kind, operation, result, elapsed);
}

#[derive(Clone, Copy)]
struct WorkspaceMeasurements {
	files: u64,
	symbols: u64,
	references: u64,
	source_bytes: u64,
	index_bytes: u64,
	graph_bytes: u64,
	rss_bytes: Option<u64>,
}

impl WorkspaceMeasurements {
	fn from_snapshot(snapshot: &WorkspaceSnapshot) -> Self {
		let estimate = SnapshotMemoryEstimate::from_snapshot(snapshot);
		Self {
			files: to_u64(snapshot.index.sources.len()),
			symbols: to_u64(snapshot.index.symbols.len()),
			references: to_u64(snapshot.index.references.len()),
			source_bytes: to_u64(estimate.source_bytes),
			index_bytes: to_u64(estimate.index_bytes),
			graph_bytes: to_u64(estimate.graph_bytes),
			rss_bytes: process_rss_bytes(),
		}
	}
}

fn to_u64(value: usize) -> u64 {
	u64::try_from(value).unwrap_or(u64::MAX)
}

#[cfg(feature = "telemetry")]
fn link_current_span(span: &Span, operation: &'static str) {
	use opentelemetry::KeyValue;
	use opentelemetry::trace::TraceContextExt as _;
	use tracing_opentelemetry::OpenTelemetrySpanExt as _;

	let context = Span::current().context();
	let span_context = context.span().span_context().clone();
	span.add_link_with_attributes(
		span_context,
		vec![KeyValue::new("operation.name", operation)],
	);
}

#[cfg(not(feature = "telemetry"))]
fn link_current_span(_span: &Span, _operation: &'static str) {}

#[cfg(feature = "telemetry")]
fn record_metrics(
	mode: &'static str,
	outcome: &'static str,
	elapsed: Duration,
	snapshot: &WorkspaceSnapshot,
	measurements: WorkspaceMeasurements,
	generation_changed: bool,
) {
	use opentelemetry::KeyValue;

	let metrics = metrics();
	let generation = snapshot.generation.value();
	metrics.generation.record(generation, &[]);
	metrics.source_files.record(measurements.files, &[]);
	metrics.source_bytes.record(measurements.source_bytes, &[]);
	metrics.index_bytes.record(measurements.index_bytes, &[]);
	metrics.graph_bytes.record(measurements.graph_bytes, &[]);
	if let Some(rss_bytes) = measurements.rss_bytes {
		metrics.process_rss_bytes.record(rss_bytes, &[]);
	}
	for (kind, count) in [
		("file", measurements.files),
		("symbol", measurements.symbols),
		("reference", measurements.references),
	] {
		metrics
			.index_entries
			.record(count, &[KeyValue::new("entry.kind", kind)]);
	}
	for (state, count) in [
		("resolved", snapshot.linkage.resolved_refs),
		("candidate", snapshot.linkage.candidate_refs),
		("external", snapshot.linkage.external_refs),
		("dynamic", snapshot.linkage.dynamic_refs),
		("blocked", snapshot.linkage.blocked_refs),
		("manifest_blocked", snapshot.linkage.manifest_blocked_refs),
		("unresolved", snapshot.linkage.unresolved_refs),
	] {
		metrics
			.graph_references
			.record(to_u64(count), &[KeyValue::new("reference.state", state)]);
	}
	if generation_changed {
		for (phase, duration) in [
			("source_catalog", snapshot.timings.source_catalog),
			("extract_sources", snapshot.timings.extract_sources),
			("semantic_index", snapshot.timings.semantic_index),
			("code_index", snapshot.timings.code_index),
			("linkage", snapshot.timings.linkage),
			("change_overlay", snapshot.timings.change_overlay),
			("total", snapshot.timings.total),
		] {
			metrics.index_phase_duration.record(
				duration.as_secs_f64() * 1_000.0,
				&[KeyValue::new("index.phase", phase)],
			);
		}
	}
	record_operation_metric(mode, outcome, elapsed);
}

#[cfg(not(feature = "telemetry"))]
fn record_metrics(
	_mode: &'static str,
	_outcome: &'static str,
	_elapsed: Duration,
	_snapshot: &WorkspaceSnapshot,
	_measurements: WorkspaceMeasurements,
	_generation_changed: bool,
) {
}

#[cfg(feature = "telemetry")]
fn record_operation_metric(mode: &'static str, outcome: &'static str, elapsed: Duration) {
	use opentelemetry::KeyValue;

	let metrics = metrics();
	let attributes = [
		KeyValue::new("index.mode", mode),
		KeyValue::new("index.result", outcome),
	];
	metrics
		.index_operation_duration
		.record(elapsed.as_secs_f64() * 1_000.0, &attributes);
	metrics.index_operations.add(1, &attributes);
}

#[cfg(not(feature = "telemetry"))]
fn record_operation_metric(_mode: &'static str, _outcome: &'static str, _elapsed: Duration) {}

#[cfg(feature = "telemetry")]
struct WorkspaceMetrics {
	generation: opentelemetry::metrics::Gauge<u64>,
	source_files: opentelemetry::metrics::Gauge<u64>,
	source_bytes: opentelemetry::metrics::Gauge<u64>,
	index_entries: opentelemetry::metrics::Gauge<u64>,
	index_bytes: opentelemetry::metrics::Gauge<u64>,
	graph_references: opentelemetry::metrics::Gauge<u64>,
	graph_bytes: opentelemetry::metrics::Gauge<u64>,
	process_rss_bytes: opentelemetry::metrics::Gauge<u64>,
	index_operations: opentelemetry::metrics::Counter<u64>,
	index_operation_duration: opentelemetry::metrics::Histogram<f64>,
	index_phase_duration: opentelemetry::metrics::Histogram<f64>,
	daemon_requests: opentelemetry::metrics::Counter<u64>,
	daemon_request_duration: opentelemetry::metrics::Histogram<f64>,
}

#[cfg(feature = "telemetry")]
fn metrics() -> &'static WorkspaceMetrics {
	use std::sync::OnceLock;

	static METRICS: OnceLock<WorkspaceMetrics> = OnceLock::new();
	METRICS.get_or_init(|| {
		let meter = opentelemetry::global::meter("code-moniker-daemon");
		WorkspaceMetrics {
			generation: meter.u64_gauge("code_moniker.workspace.generation").build(),
			source_files: meter
				.u64_gauge("code_moniker.workspace.source.files")
				.build(),
			source_bytes: meter
				.u64_gauge("code_moniker.workspace.source.bytes")
				.with_unit("By")
				.build(),
			index_entries: meter
				.u64_gauge("code_moniker.workspace.index.entries")
				.build(),
			index_bytes: meter
				.u64_gauge("code_moniker.workspace.index.estimated_bytes")
				.with_unit("By")
				.build(),
			graph_references: meter
				.u64_gauge("code_moniker.workspace.graph.references")
				.build(),
			graph_bytes: meter
				.u64_gauge("code_moniker.workspace.graph.estimated_bytes")
				.with_unit("By")
				.build(),
			process_rss_bytes: meter
				.u64_gauge("process.memory.rss")
				.with_unit("By")
				.build(),
			index_operations: meter
				.u64_counter("code_moniker.workspace.index.operations")
				.build(),
			index_operation_duration: meter
				.f64_histogram("code_moniker.workspace.index.operation.duration")
				.with_unit("ms")
				.build(),
			index_phase_duration: meter
				.f64_histogram("code_moniker.workspace.index.phase.duration")
				.with_unit("ms")
				.build(),
			daemon_requests: meter.u64_counter("code_moniker.daemon.requests").build(),
			daemon_request_duration: meter
				.f64_histogram("code_moniker.daemon.request.duration")
				.with_unit("ms")
				.build(),
		}
	})
}

#[cfg(all(feature = "telemetry", target_os = "macos"))]
fn process_rss_bytes() -> Option<u64> {
	let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::uninit();
	let expected = std::mem::size_of::<libc::proc_taskinfo>();
	let written = unsafe {
		libc::proc_pidinfo(
			libc::getpid(),
			libc::PROC_PIDTASKINFO,
			0,
			info.as_mut_ptr().cast(),
			i32::try_from(expected).ok()?,
		)
	};
	if usize::try_from(written).ok()? != expected {
		return None;
	}
	Some(unsafe { info.assume_init() }.pti_resident_size)
}

#[cfg(all(feature = "telemetry", target_os = "linux"))]
fn process_rss_bytes() -> Option<u64> {
	let statm = std::fs::read_to_string("/proc/self/statm").ok()?;
	let resident_pages = statm.split_whitespace().nth(1)?.parse::<u64>().ok()?;
	let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
	let page_size = u64::try_from(page_size).ok()?;
	resident_pages.checked_mul(page_size)
}

#[cfg(any(
	not(feature = "telemetry"),
	all(
		feature = "telemetry",
		not(any(target_os = "macos", target_os = "linux"))
	)
))]
fn process_rss_bytes() -> Option<u64> {
	None
}
