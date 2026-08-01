use std::path::Path;

use anyhow::{Context as _, ensure};
use serde::Deserialize;

pub const PROJECT_CONFIG_FILE: &str = ".code-moniker.toml";

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TelemetryConfig {
	#[serde(default)]
	pub enabled: bool,
	#[serde(default)]
	pub endpoint: Option<String>,
	#[serde(default)]
	pub metric_export_interval_ms: Option<u64>,
}

impl TelemetryConfig {
	fn validate(self) -> anyhow::Result<Self> {
		if let Some(endpoint) = self.endpoint.as_deref() {
			ensure!(
				!endpoint.trim().is_empty(),
				"telemetry.endpoint cannot be empty"
			);
		}
		if let Some(interval) = self.metric_export_interval_ms {
			ensure!(
				interval > 0,
				"telemetry.metric_export_interval_ms must be greater than zero"
			);
		}
		Ok(self)
	}
}

#[derive(Default, Deserialize)]
struct ProjectConfig {
	#[serde(default)]
	telemetry: TelemetryConfig,
}

pub fn load_telemetry_config(path: &Path) -> anyhow::Result<TelemetryConfig> {
	if !path.exists() {
		return Ok(TelemetryConfig::default());
	}
	let text = std::fs::read_to_string(path)
		.with_context(|| format!("read project config {}", path.display()))?;
	let config: ProjectConfig = toml::from_str(&text)
		.with_context(|| format!("parse project config {}", path.display()))?;
	config.telemetry.validate()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn telemetry_config_is_optional_and_project_scoped() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join(PROJECT_CONFIG_FILE);
		assert_eq!(
			load_telemetry_config(&path).unwrap(),
			TelemetryConfig::default()
		);
		std::fs::write(
			&path,
			r#"
[aliases]
root = "workspace"

[telemetry]
enabled = true
endpoint = "http://127.0.0.1:4318"
metric_export_interval_ms = 5000
"#,
		)
		.unwrap();
		assert_eq!(
			load_telemetry_config(&path).unwrap(),
			TelemetryConfig {
				enabled: true,
				endpoint: Some("http://127.0.0.1:4318".to_string()),
				metric_export_interval_ms: Some(5000),
			}
		);
	}

	#[test]
	fn telemetry_config_rejects_invalid_owned_fields() {
		let directory = tempfile::tempdir().unwrap();
		let path = directory.path().join(PROJECT_CONFIG_FILE);
		std::fs::write(&path, "[telemetry]\nmetric_export_interval_ms = 0\n").unwrap();
		assert!(load_telemetry_config(&path).is_err());
		std::fs::write(&path, "[telemetry]\nunknown = true\n").unwrap();
		assert!(load_telemetry_config(&path).is_err());
	}
}
