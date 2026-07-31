//! Process-wide tracing and optional OpenTelemetry bootstrap.

use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

#[cfg(feature = "telemetry")]
use std::time::Duration;
#[cfg(feature = "telemetry")]
use std::{
	fmt::Debug,
	sync::atomic::{AtomicBool, Ordering},
};

#[cfg(feature = "telemetry")]
use opentelemetry::KeyValue;
#[cfg(feature = "telemetry")]
use opentelemetry::trace::TracerProvider as _;
#[cfg(feature = "telemetry")]
use opentelemetry_otlp::{Protocol, WithExportConfig};
#[cfg(feature = "telemetry")]
use opentelemetry_sdk::Resource;
#[cfg(feature = "telemetry")]
use opentelemetry_sdk::{
	error::OTelSdkResult,
	trace::{BatchConfigBuilder, BatchSpanProcessor, SdkTracerProvider, SpanData, SpanExporter},
};

#[cfg(feature = "telemetry")]
const EXPORT_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(feature = "telemetry")]
const MAX_QUEUE_SIZE: usize = 1024;
#[cfg(feature = "telemetry")]
const MAX_EXPORT_BATCH_SIZE: usize = 256;

#[cfg(feature = "telemetry")]
#[derive(Debug)]
struct DiagnosingExporter<E> {
	inner: E,
	failure_reported: AtomicBool,
}

#[cfg(feature = "telemetry")]
impl<E> DiagnosingExporter<E> {
	fn new(inner: E) -> Self {
		Self {
			inner,
			failure_reported: AtomicBool::new(false),
		}
	}
}

#[cfg(feature = "telemetry")]
impl<E: SpanExporter> SpanExporter for DiagnosingExporter<E> {
	async fn export(&self, batch: Vec<SpanData>) -> OTelSdkResult {
		let result = self.inner.export(batch).await;
		if let Err(error) = &result
			&& !self.failure_reported.swap(true, Ordering::Relaxed)
		{
			eprintln!(
				"code-moniker: OpenTelemetry export failed (further failures suppressed): {error}"
			);
		}
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

/// Keeps the OpenTelemetry provider alive until the process command completes.
#[derive(Debug, Default)]
pub(super) struct TelemetryGuard {
	#[cfg(feature = "telemetry")]
	provider: Option<SdkTracerProvider>,
}

impl Drop for TelemetryGuard {
	fn drop(&mut self) {
		#[cfg(feature = "telemetry")]
		if let Some(provider) = self.provider.take() {
			let _ = provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		}
	}
}

/// Installs the process subscriber and enables OTLP only after explicit opt-in.
///
/// Configuration and exporter failures are diagnostics, never command failures.
pub(super) fn init() -> TelemetryGuard {
	#[cfg(feature = "telemetry")]
	match telemetry_requested(std::env::var("CODE_MONIKER_TELEMETRY").ok().as_deref()) {
		Ok(true) => match init_otlp() {
			Ok(guard) => return guard,
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
fn init_otlp() -> Result<TelemetryGuard, Box<dyn std::error::Error + Send + Sync>> {
	let exporter = opentelemetry_otlp::SpanExporter::builder()
		.with_http()
		.with_protocol(Protocol::HttpBinary)
		.with_timeout(EXPORT_TIMEOUT)
		.build()?;
	let processor = BatchSpanProcessor::builder(DiagnosingExporter::new(exporter))
		.with_batch_config(
			BatchConfigBuilder::default()
				.with_max_queue_size(MAX_QUEUE_SIZE)
				.with_max_export_batch_size(MAX_EXPORT_BATCH_SIZE)
				.with_scheduled_delay(Duration::from_secs(1))
				.build(),
		)
		.build();
	let resource_builder = Resource::builder()
		.with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")));
	let resource = if explicit_service_name_configured() {
		resource_builder.build()
	} else {
		resource_builder.with_service_name("code-moniker").build()
	};
	let provider = SdkTracerProvider::builder()
		.with_resource(resource)
		.with_span_processor(processor)
		.build();
	let tracer = provider.tracer("code-moniker");
	let subscriber = tracing_subscriber::registry()
		.with(LevelFilter::INFO)
		.with(
			tracing_subscriber::fmt::layer()
				.with_writer(std::io::stderr)
				.with_target(false)
				.with_level(true)
				.compact(),
		)
		.with(tracing_opentelemetry::layer().with_tracer(tracer));
	if let Err(error) = subscriber.try_init() {
		let _ = provider.shutdown_with_timeout(EXPORT_TIMEOUT);
		return Err(Box::new(error));
	}

	tracing::info_span!(
		"telemetry.bootstrap",
		telemetry.exporter = "otlp",
		telemetry.protocol = "http/protobuf"
	)
	.in_scope(|| tracing::info!("OpenTelemetry export enabled"));
	Ok(TelemetryGuard {
		provider: Some(provider),
	})
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
fn telemetry_requested(value: Option<&str>) -> Result<bool, String> {
	let Some(value) = value else {
		return Ok(false);
	};
	match value.trim().to_ascii_lowercase().as_str() {
		"1" | "true" | "yes" | "on" => Ok(true),
		"" | "0" | "false" | "no" | "off" => Ok(false),
		_ => Err(format!(
			"CODE_MONIKER_TELEMETRY must be true/false, got `{value}`"
		)),
	}
}

#[cfg(all(test, feature = "telemetry"))]
mod tests {
	use super::{resource_attributes_define_service_name, telemetry_requested};

	#[test]
	fn telemetry_requires_explicit_valid_opt_in() {
		assert_eq!(telemetry_requested(None), Ok(false));
		assert_eq!(telemetry_requested(Some("")), Ok(false));
		assert_eq!(telemetry_requested(Some("off")), Ok(false));
		assert_eq!(telemetry_requested(Some("TRUE")), Ok(true));
		assert_eq!(telemetry_requested(Some("1")), Ok(true));
		assert!(telemetry_requested(Some("sometimes")).is_err());
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
