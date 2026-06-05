use std::sync::Arc;

use code_moniker_core::core::logger::Logger;

#[allow(dead_code)]
#[derive(Clone)]
pub struct TracingLogger;

impl Logger for TracingLogger {
	fn trace(&self, target: &str, message: &str) {
		tracing::trace!(target: "code_moniker", module = target, "{}", message);
	}

	fn debug(&self, target: &str, message: &str) {
		tracing::debug!(target: "code_moniker", module = target, "{}", message);
	}

	fn info(&self, target: &str, message: &str) {
		tracing::info!(target: "code_moniker", module = target, "{}", message);
	}

	fn warn(&self, target: &str, message: &str) {
		tracing::warn!(target: "code_moniker", module = target, "{}", message);
	}

	fn error(&self, target: &str, message: &str) {
		tracing::error!(target: "code_moniker", module = target, "{}", message);
	}
}

#[allow(dead_code)]
pub fn tracing_logger() -> Arc<dyn Logger> {
	Arc::new(TracingLogger)
}
