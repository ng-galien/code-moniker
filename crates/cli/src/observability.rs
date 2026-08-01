//! Process-wide tracing and optional OpenTelemetry bootstrap.

#[cfg(feature = "telemetry")]
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[cfg(feature = "telemetry")]
use code_moniker_cli::DaemonCommand;
use code_moniker_cli::{Cli, Command};
use code_moniker_query::bounded_debug;
#[cfg(feature = "telemetry")]
use code_moniker_workspace::environment::{
	PROJECT_CONFIG_FILE, TelemetryConfig, load_telemetry_config,
};

#[cfg(feature = "telemetry")]
use std::path::{Path, PathBuf};
#[cfg(feature = "telemetry")]
use std::time::Duration;
#[cfg(feature = "telemetry")]
use std::{
	fmt::Debug,
	sync::atomic::{AtomicBool, Ordering},
};

#[cfg(feature = "telemetry")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "telemetry")]
use opentelemetry::{KeyValue, global};
#[cfg(feature = "telemetry")]
use opentelemetry_otlp::{Protocol, WithExportConfig};
#[cfg(feature = "telemetry")]
use opentelemetry_sdk::Resource;
#[cfg(feature = "telemetry")]
use opentelemetry_sdk::{
	error::OTelSdkResult,
	logs::{LogBatch, LogExporter, SdkLoggerProvider},
	metrics::{
		PeriodicReader, SdkMeterProvider, Temporality, data::ResourceMetrics,
		exporter::PushMetricExporter,
	},
	trace::{BatchConfigBuilder, BatchSpanProcessor, SdkTracerProvider, SpanData, SpanExporter},
};

#[cfg(feature = "telemetry")]
const EXPORT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(feature = "telemetry")]
const MAX_QUEUE_SIZE: usize = 1024;
#[cfg(feature = "telemetry")]
const MAX_EXPORT_BATCH_SIZE: usize = 256;
const COMMAND_PAYLOAD_LIMIT: usize = 4_096;

#[cfg(feature = "telemetry")]
#[derive(Debug)]
struct DiagnosingExporter<E> {
	inner: E,
	signal: &'static str,
	failure_reported: AtomicBool,
}

#[cfg(feature = "telemetry")]
impl<E> DiagnosingExporter<E> {
	fn new(inner: E, signal: &'static str) -> Self {
		Self {
			inner,
			signal,
			failure_reported: AtomicBool::new(false),
		}
	}

	fn diagnose(&self, result: &OTelSdkResult) {
		if let Err(error) = result
			&& !self.failure_reported.swap(true, Ordering::Relaxed)
		{
			eprintln!(
				"code-moniker: OpenTelemetry {} export failed (further failures suppressed): {error}",
				self.signal
			);
		}
	}
}

#[cfg(feature = "telemetry")]
impl<E: SpanExporter> SpanExporter for DiagnosingExporter<E> {
	async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
		let result = self.inner.export(batch).await;
		self.diagnose(&result);
		result
	}

	fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
		self.inner.shutdown_with_timeout(timeout)
	}

	fn force_flush(&self) -> OTelSdkResult {
		self.inner.force_flush()
	}

	fn set_resource(&mut self, resource: &Resource) {
		self.inner.set_resource(resource);
	}
}

#[cfg(feature = "telemetry")]
impl<E: LogExporter> LogExporter for DiagnosingExporter<E> {
	async fn export(&self, batch: LogBatch<'_>) -> OTelSdkResult {
		let result = self.inner.export(batch).await;
		self.diagnose(&result);
		result
	}

	fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
		self.inner.shutdown_with_timeout(timeout)
	}

	fn event_enabled(
		&self,
		level: opentelemetry::logs::Severity,
		target: &str,
		name: Option<&str>,
	) -> bool {
		self.inner.event_enabled(level, target, name)
	}

	fn set_resource(&mut self, resource: &Resource) {
		self.inner.set_resource(resource);
	}
}

#[cfg(feature = "telemetry")]
impl<E: PushMetricExporter> PushMetricExporter for DiagnosingExporter<E> {
	async fn export(&self, metrics: &ResourceMetrics) -> OTelSdkResult {
		let result = self.inner.export(metrics).await;
		self.diagnose(&result);
		result
	}

	fn force_flush(&self) -> OTelSdkResult {
		self.inner.force_flush()
	}

	fn shutdown_with_timeout(&self, timeout: Duration) -> OTelSdkResult {
		self.inner.shutdown_with_timeout(timeout)
	}

	fn temporality(&self) -> Temporality {
		self.inner.temporality()
	}
}

/// Keeps the OpenTelemetry provider alive until the process command completes.
#[derive(Debug, Default)]
pub(super) struct TelemetryGuard {
	#[cfg(feature = "telemetry")]
	tracer_provider: Option<SdkTracerProvider>,
	#[cfg(feature = "telemetry")]
	meter_provider: Option<SdkMeterProvider>,
	#[cfg(feature = "telemetry")]
	logger_provider: Option<SdkLoggerProvider>,
}

impl Drop for TelemetryGuard {
	fn drop(&mut self) {
		#[cfg(feature = "telemetry")]
		if let Some(provider) = self.logger_provider.take() {
			let _ = provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		}
		#[cfg(feature = "telemetry")]
		if let Some(provider) = self.meter_provider.take() {
			let _ = provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		}
		#[cfg(feature = "telemetry")]
		if let Some(provider) = self.tracer_provider.take() {
			let _ = provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		}
	}
}

/// Installs the process subscriber and enables OTLP only after explicit opt-in.
///
/// Configuration and exporter failures are diagnostics, never command failures.
pub(super) fn init(_cli: &Cli) -> TelemetryGuard {
	code_moniker_daemon::set_telemetry_export_enabled(false);
	#[cfg(feature = "telemetry")]
	if is_stdio_supervisor(_cli) {
		init_local_logging();
		return TelemetryGuard::default();
	}
	#[cfg(feature = "telemetry")]
	let telemetry = project_telemetry_config(_cli);
	#[cfg(feature = "telemetry")]
	match telemetry_requested(
		std::env::var("CODE_MONIKER_TELEMETRY").ok().as_deref(),
		telemetry.enabled,
	) {
		Ok(true) => match init_otlp(_cli, &telemetry) {
			Ok(guard) => {
				code_moniker_daemon::set_telemetry_export_enabled(true);
				return guard;
			}
			Err(error) => {
				eprintln!("code-moniker: OpenTelemetry disabled: {error}");
			}
		},
		Ok(false) => {}
		Err(error) => {
			eprintln!("code-moniker: OpenTelemetry disabled: {error}");
		}
	}

	init_local_logging();
	TelemetryGuard::default()
}

pub(super) fn command_span(cli: &Cli) -> tracing::Span {
	tracing::info_span!(
		"cli.command",
		command.name = command_name(&cli.command),
		command.request = %bounded_debug(&cli.command, COMMAND_PAYLOAD_LIMIT),
		command.status = tracing::field::Empty,
	)
}

fn command_name(command: &Command) -> &'static str {
	match command {
		Command::Extract(_) => "extract",
		Command::Stats(_) => "stats",
		Command::Check(_) => "check",
		Command::Diff(_) => "diff",
		Command::Rules(_) => "rules",
		#[cfg(feature = "tui")]
		Command::Ui(_) => "ui",
		#[cfg(feature = "mcp")]
		Command::Mcp(_) => "mcp",
		Command::Daemon(_) => "daemon",
		Command::Query(_) => "query",
		Command::Agent(_) => "agent",
		Command::Langs(_) => "langs",
		Command::Shapes(_) => "shapes",
		Command::Manifest(_) => "manifest",
	}
}

fn init_local_logging() {
	let _ = tracing_subscriber::registry()
		.with(LevelFilter::INFO)
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(std::io::stderr)
				.with_target(false)
				.with_level(true)
				.compact(),
		)
		.try_init();
}

#[cfg(feature = "telemetry")]
fn init_otlp(
	cli: &Cli,
	config: &TelemetryConfig,
) -> Result<TelemetryGuard, Box<dyn std::error::Error + Send + Sync>> {
	let span_exporter = opentelemetry_otlp::SpanExporter::builder()
		.with_http()
		.with_protocol(Protocol::HttpBinary)
		.with_timeout(EXPORT_TIMEOUT);
	let span_exporter = match config.endpoint.as_deref() {
		Some(endpoint) => span_exporter.with_endpoint(signal_endpoint(endpoint, "v1/traces")),
		None => span_exporter,
	}
	.build()?;
	let metric_exporter = opentelemetry_otlp::MetricExporter::builder()
		.with_http()
		.with_protocol(Protocol::HttpBinary)
		.with_timeout(EXPORT_TIMEOUT);
	let metric_exporter = match config.endpoint.as_deref() {
		Some(endpoint) => metric_exporter.with_endpoint(signal_endpoint(endpoint, "v1/metrics")),
		None => metric_exporter,
	}
	.build()?;
	let log_exporter = opentelemetry_otlp::LogExporter::builder()
		.with_http()
		.with_protocol(Protocol::HttpBinary)
		.with_timeout(EXPORT_TIMEOUT);
	let log_exporter = match config.endpoint.as_deref() {
		Some(endpoint) => log_exporter.with_endpoint(signal_endpoint(endpoint, "v1/logs")),
		None => log_exporter,
	}
	.build()?;
	let processor = BatchSpanProcessor::builder(DiagnosingExporter::new(span_exporter, "trace"))
		.with_batch_config(
			BatchConfigBuilder::default()
				.with_max_queue_size(MAX_QUEUE_SIZE)
				.with_max_export_batch_size(MAX_EXPORT_BATCH_SIZE)
				.with_scheduled_delay(Duration::from_secs(1))
				.build(),
		)
		.build();
	let mut resource_builder = Resource::builder()
		.with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")));
	if let Some(instance_id) = daemon_instance_id(cli) {
		resource_builder =
			resource_builder.with_attribute(KeyValue::new("service.instance.id", instance_id));
	}
	let resource = if explicit_service_name_configured() {
		resource_builder.build()
	} else {
		resource_builder
			.with_service_name(default_service_name(cli))
			.build()
	};
	let tracer_provider = SdkTracerProvider::builder()
		.with_resource(resource.clone())
		.with_span_processor(processor)
		.build();
	let metric_reader = PeriodicReader::builder(DiagnosingExporter::new(metric_exporter, "metric"));
	let metric_reader = match config.metric_export_interval_ms {
		Some(interval) => metric_reader.with_interval(Duration::from_millis(interval)),
		None => metric_reader,
	}
	.build();
	let meter_provider = SdkMeterProvider::builder()
		.with_resource(resource.clone())
		.with_reader(metric_reader)
		.build();
	global::set_meter_provider(meter_provider.clone());
	let logger_provider = SdkLoggerProvider::builder()
		.with_resource(resource)
		.with_batch_exporter(DiagnosingExporter::new(log_exporter, "log"))
		.build();
	let tracer = tracer_provider.tracer("code-moniker");
	let otel_log_layer =
		opentelemetry_appender_tracing::layer::OpenTelemetryTracingBridge::new(&logger_provider)
			.with_filter(tracing_subscriber::filter::filter_fn(|metadata| {
				let target = metadata.target();
				!target.starts_with("opentelemetry")
					&& !target.starts_with("hyper")
					&& !target.starts_with("reqwest")
			}));
	let subscriber = tracing_subscriber::registry()
		.with(LevelFilter::INFO)
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(std::io::stderr)
				.with_target(false)
				.with_level(true)
				.compact(),
		)
		.with(tracing_opentelemetry::layer().with_tracer(tracer))
		.with(otel_log_layer);
	if let Err(error) = subscriber.try_init() {
		let _ = logger_provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		let _ = meter_provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		let _ = tracer_provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		return Err(Box::new(error));
	}

	tracing::info_span!(
		"telemetry.bootstrap",
		telemetry.exporter = "otlp",
		telemetry.protocol = "http/protobuf"
	)
	.in_scope(|| tracing::info!("OpenTelemetry export enabled"));
	Ok(TelemetryGuard {
		tracer_provider: Some(tracer_provider),
		meter_provider: Some(meter_provider),
		logger_provider: Some(logger_provider),
	})
}

#[cfg(feature = "telemetry")]
fn default_service_name(cli: &Cli) -> String {
	match &cli.command {
		Command::Daemon(args) if matches!(&args.command, DaemonCommand::Start(_)) => format!(
			"code-moniker-daemon:{}",
			daemon_workspace_label(cli).unwrap_or_else(|| "workspace".to_string())
		),
		#[cfg(feature = "mcp")]
		Command::Mcp(_) => "code-moniker-mcp".to_string(),
		_ => "code-moniker-cli".to_string(),
	}
}

#[cfg(feature = "telemetry")]
fn daemon_instance_id(cli: &Cli) -> Option<String> {
	let Command::Daemon(args) = &cli.command else {
		return None;
	};
	let DaemonCommand::Start(args) = &args.command else {
		return None;
	};
	let roots = args
		.root
		.workspace_roots
		.iter()
		.map(|root| {
			std::fs::canonicalize(root)
				.unwrap_or_else(|_| root.clone())
				.display()
				.to_string()
		})
		.collect::<Vec<_>>()
		.join("+");
	Some(format!("{roots}#{}", std::process::id()))
}

#[cfg(feature = "telemetry")]
fn daemon_workspace_label(cli: &Cli) -> Option<String> {
	let Command::Daemon(args) = &cli.command else {
		return None;
	};
	let DaemonCommand::Start(args) = &args.command else {
		return None;
	};
	let mut label = args
		.root
		.workspace_roots
		.iter()
		.map(|root| {
			std::fs::canonicalize(root)
				.unwrap_or_else(|_| root.clone())
				.file_name()
				.and_then(|name| name.to_str())
				.unwrap_or("workspace")
				.to_string()
		})
		.collect::<Vec<_>>()
		.join("+");
	if let Some(project) = args
		.root
		.project
		.as_deref()
		.filter(|project| *project != ".")
	{
		label.push(':');
		label.push_str(project);
	}
	Some(label)
}

#[cfg(feature = "telemetry")]
fn explicit_service_name_configured() -> bool {
	std::env::var("OTEL_SERVICE_NAME")
		.ok()
		.is_some_and(|name| !name.is_empty())
		|| std::env::var("OTEL_RESOURCE_ATTRIBUTES")
			.ok()
			.is_some_and(|attributes| resource_attributes_define_service_name(&attributes))
}

#[cfg(feature = "telemetry")]
fn resource_attributes_define_service_name(attributes: &str) -> bool {
	attributes.split(',').any(|attribute| {
		attribute
			.split_once('=')
			.is_some_and(|(key, _)| key.trim() == "service.name")
	})
}

#[cfg(feature = "telemetry")]
fn telemetry_requested(value: Option<&str>, project_default: bool) -> Result<bool, String> {
	let Some(value) = value else {
		return Ok(project_default);
	};
	match value.trim().to_ascii_lowercase().as_str() {
		"1" | "true" | "yes" | "on" => Ok(true),
		"" | "0" | "false" | "no" | "off" => Ok(false),
		_ => Err(format!(
			"CODE_MONIKER_TELEMETRY must be true/false, got `{value}`"
		)),
	}
}

#[cfg(feature = "telemetry")]
fn signal_endpoint(base: &str, signal_path: &str) -> String {
	format!("{}/{}", base.trim_end_matches('/'), signal_path)
}

#[cfg(feature = "telemetry")]
fn project_telemetry_config(cli: &Cli) -> TelemetryConfig {
	let path = project_config_path(cli);
	match load_telemetry_config(&path) {
		Ok(config) => config,
		Err(error) => {
			eprintln!(
				"code-moniker: invalid telemetry configuration in {}: {error:#}; using environment defaults",
				path.display()
			);
			TelemetryConfig::default()
		}
	}
}

#[cfg(feature = "telemetry")]
fn project_config_path(cli: &Cli) -> PathBuf {
	let context = match &cli.command {
		Command::Extract(args) => Some(args.path.as_path()),
		Command::Stats(args) => args.paths.first().map(PathBuf::as_path),
		Command::Check(args) => Some(args.path.as_path()),
		Command::Diff(args) => args
			.path
			.as_deref()
			.or_else(|| (!args.target.contains("..")).then(|| Path::new(&args.target))),
		#[cfg(feature = "tui")]
		Command::Ui(args) => args.paths.first().map(PathBuf::as_path),
		#[cfg(feature = "mcp")]
		Command::Mcp(args) => args.paths.first().map(PathBuf::as_path),
		Command::Daemon(args) => match &args.command {
			DaemonCommand::Start(args) => args.root.workspace_roots.first().map(PathBuf::as_path),
			DaemonCommand::Status(args) | DaemonCommand::Stop(args) => {
				args.workspace_roots.first().map(PathBuf::as_path)
			}
			DaemonCommand::List => None,
		},
		Command::Query(args) => args.workspace_roots.first().map(PathBuf::as_path),
		Command::Manifest(args) => Some(args.path.as_path()),
		_ => None,
	}
	.unwrap_or_else(|| Path::new("."));
	project_root(context).join(PROJECT_CONFIG_FILE)
}

#[cfg(feature = "telemetry")]
fn project_root(path: &Path) -> &Path {
	if path.is_file() {
		path.parent()
			.filter(|parent| !parent.as_os_str().is_empty())
			.unwrap_or_else(|| Path::new("."))
	} else {
		path
	}
}

#[cfg(feature = "telemetry")]
fn is_stdio_supervisor(_cli: &Cli) -> bool {
	#[cfg(feature = "mcp")]
	if let Command::Mcp(args) = &_cli.command {
		return args.is_stdio_supervisor();
	}
	false
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
	use super::{resource_attributes_define_service_name, signal_endpoint, telemetry_requested};

	#[test]
	fn telemetry_requires_explicit_valid_opt_in() {
		assert_eq!(telemetry_requested(None, false), Ok(false));
		assert_eq!(telemetry_requested(None, true), Ok(true));
		assert_eq!(telemetry_requested(Some(""), true), Ok(false));
		assert_eq!(telemetry_requested(Some("off"), true), Ok(false));
		assert_eq!(telemetry_requested(Some("TRUE"), false), Ok(true));
		assert_eq!(telemetry_requested(Some("1"), false), Ok(true));
		assert!(telemetry_requested(Some("sometimes"), true).is_err());
	}

	#[test]
	fn project_endpoint_is_expanded_per_signal() {
		assert_eq!(
			signal_endpoint("http://127.0.0.1:4318/", "v1/traces"),
			"http://127.0.0.1:4318/v1/traces"
		);
	}

	#[test]
	fn service_name_resource_attribute_is_preserved() {
		assert!(resource_attributes_define_service_name(
			"deployment.environment=dev, service.name=my-index"
		));
		assert!(!resource_attributes_define_service_name(
			"deployment.environment=dev"
		));
	}
}
