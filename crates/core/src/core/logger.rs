use std::sync::Arc;

pub trait Logger: Send + Sync {
	fn trace(&self, target: &str, message: &str);
	fn debug(&self, target: &str, message: &str);
	fn info(&self, target: &str, message: &str);
	fn warn(&self, target: &str, message: &str);
	fn error(&self, target: &str, message: &str);
}

#[derive(Clone)]
pub struct NoopLogger;

impl Logger for NoopLogger {
	fn trace(&self, _target: &str, _message: &str) {}
	fn debug(&self, _target: &str, _message: &str) {}
	fn info(&self, _target: &str, _message: &str) {}
	fn warn(&self, _target: &str, _message: &str) {}
	fn error(&self, _target: &str, _message: &str) {}
}

pub fn noop_logger() -> Arc<dyn Logger> {
	Arc::new(NoopLogger)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::sync::Mutex;

	struct MockLogger {
		logs: Mutex<Vec<(String, String, String)>>,
	}

	impl Logger for MockLogger {
		fn trace(&self, target: &str, message: &str) {
			self.logs.lock().unwrap().push((
				"trace".to_string(),
				target.to_string(),
				message.to_string(),
			));
		}
		fn debug(&self, target: &str, message: &str) {
			self.logs.lock().unwrap().push((
				"debug".to_string(),
				target.to_string(),
				message.to_string(),
			));
		}
		fn info(&self, target: &str, message: &str) {
			self.logs.lock().unwrap().push((
				"info".to_string(),
				target.to_string(),
				message.to_string(),
			));
		}
		fn warn(&self, target: &str, message: &str) {
			self.logs.lock().unwrap().push((
				"warn".to_string(),
				target.to_string(),
				message.to_string(),
			));
		}
		fn error(&self, target: &str, message: &str) {
			self.logs.lock().unwrap().push((
				"error".to_string(),
				target.to_string(),
				message.to_string(),
			));
		}
	}

	#[test]
	fn test_mock_logger() {
		let logger = MockLogger {
			logs: Mutex::new(Vec::new()),
		};
		logger.info("test_target", "test message");
		let logs = logger.logs.lock().unwrap();
		assert_eq!(logs.len(), 1);
		assert_eq!(
			logs[0],
			(
				"info".to_string(),
				"test_target".to_string(),
				"test message".to_string()
			)
		);
	}
}
