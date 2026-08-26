use serde_json::Value;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum OutputBudget {
	Small,
	Medium,
	Full,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) enum OutputFormat {
	Text,
	Json,
}

impl OutputFormat {
	pub(in crate::mcp) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments.get("format") {
			None => Ok(Self::Text),
			Some(Value::String(value)) if value == "text" => Ok(Self::Text),
			Some(Value::String(value)) if value == "json" => Ok(Self::Json),
			Some(Value::String(value)) => anyhow::bail!("unknown output format `{value}`"),
			Some(_) => anyhow::bail!("`format` must be a string"),
		}
	}
}

impl OutputBudget {
	pub(in crate::mcp) fn as_str(self) -> &'static str {
		match self {
			Self::Small => "small",
			Self::Medium => "medium",
			Self::Full => "full",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) struct AgentOutputOptions {
	pub(in crate::mcp) compact: bool,
	pub(in crate::mcp) budget: OutputBudget,
}

impl AgentOutputOptions {
	pub(in crate::mcp) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let compact = compact_argument(arguments)?;
		let budget = output_budget(arguments)?;
		Ok(Self { compact, budget })
	}

	pub(in crate::mcp) fn for_format(
		arguments: &Value,
		format: OutputFormat,
	) -> anyhow::Result<Self> {
		if format == OutputFormat::Json {
			return Ok(Self {
				compact: false,
				budget: OutputBudget::Full,
			});
		}
		Self::from_arguments(arguments)
	}

	pub(in crate::mcp) fn default_page_limit(self) -> usize {
		match self.budget {
			OutputBudget::Small => 20,
			OutputBudget::Medium => 80,
			OutputBudget::Full => super::scope::MAX_LIMIT,
		}
	}
}

pub(in crate::mcp) fn add_output_budget_schema(schema: &mut Value) {
	let Some(object) = schema.as_object_mut() else {
		return;
	};
	let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) else {
		return;
	};
	properties.insert(
		"budget".to_string(),
		serde_json::json!({
			"type": "string",
			"enum": ["small", "medium", "full"],
			"default": "small",
			"description": "Markdown volume profile. Tools map small, medium, and full to bounded result counts, traversal depth, witnesses, and optional detail before text rendering. Ignored when format=json because structured output always uses the full profile."
		}),
	);
}

pub(in crate::mcp) fn add_output_format_schema(schema: &mut Value) {
	let Some(object) = schema.as_object_mut() else {
		return;
	};
	let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) else {
		return;
	};
	properties.insert(
		"format".to_string(),
		serde_json::json!({
			"type": "string",
			"enum": ["text", "json"],
			"default": "text",
			"description": "Response representation. text returns Markdown or plain text content only; json returns structuredContent only. The response never contains both."
		}),
	);
}

pub(in crate::mcp) fn add_compact_output_schema(schema: &mut Value) {
	let Some(object) = schema.as_object_mut() else {
		return;
	};
	let Some(properties) = object.get_mut("properties").and_then(Value::as_object_mut) else {
		return;
	};
	properties.insert(
		"compact".to_string(),
		serde_json::json!({
			"type": "boolean",
			"default": true,
			"description": "Render compact agent output by default; false preserves canonical verbose output."
		}),
	);
}

fn output_budget(arguments: &Value) -> anyhow::Result<OutputBudget> {
	match arguments.get("budget") {
		None => Ok(OutputBudget::Small),
		Some(Value::String(value)) if value == "small" => Ok(OutputBudget::Small),
		Some(Value::String(value)) if value == "medium" => Ok(OutputBudget::Medium),
		Some(Value::String(value)) if value == "full" => Ok(OutputBudget::Full),
		Some(Value::String(value)) => anyhow::bail!("unknown output budget `{value}`"),
		Some(_) => anyhow::bail!("`budget` must be a string"),
	}
}

pub(in crate::mcp) fn is_workspace_uri(uri: &str, scheme: &str, default_uri: &str) -> bool {
	let value = uri.trim();
	value.is_empty()
		|| value == default_uri
		|| value == format!("{scheme}workspace")
		|| value == format!("{scheme}.")
		|| value == scheme.trim_end_matches('/')
}

pub(in crate::mcp) fn normalize_workspace_uri(
	scheme: &str,
	request_uri: &str,
	default_uri: &str,
) -> String {
	let trimmed = request_uri.trim();
	if trimmed.is_empty() || trimmed == default_uri {
		format!("{scheme}workspace")
	} else {
		trimmed.to_string()
	}
}

pub(in crate::mcp) fn line_range_suffix(range: Option<(u32, u32)>) -> String {
	range
		.map(|(start, end)| format!(":{start}-{end}"))
		.unwrap_or_default()
}

pub(in crate::mcp) fn compact_argument(arguments: &Value) -> anyhow::Result<bool> {
	match arguments.get("compact") {
		None => Ok(true),
		Some(Value::Bool(value)) => Ok(*value),
		Some(_) => anyhow::bail!("`compact` must be a boolean"),
	}
}

#[cfg(test)]
mod tests {
	use super::{AgentOutputOptions, OutputBudget, OutputFormat, compact_argument};
	use serde_json::json;

	#[test]
	fn compact_defaults_true_and_rejects_non_boolean_values() {
		assert!(compact_argument(&json!({})).unwrap());
		assert!(!compact_argument(&json!({"compact": false})).unwrap());
		assert!(compact_argument(&json!({"compact": "yes"})).is_err());
	}

	#[test]
	fn output_budget_is_a_typed_volume_profile() {
		let small = AgentOutputOptions::from_arguments(&json!({})).unwrap();
		let medium = AgentOutputOptions::from_arguments(&json!({"budget": "medium"})).unwrap();
		let full = AgentOutputOptions::from_arguments(&json!({"budget": "full"})).unwrap();

		assert_eq!(small.budget, OutputBudget::Small);
		assert_eq!(small.default_page_limit(), 20);
		assert_eq!(medium.default_page_limit(), 80);
		assert_eq!(full.default_page_limit(), super::super::scope::MAX_LIMIT);
	}

	#[test]
	fn output_budget_rejects_non_string_values() {
		assert!(AgentOutputOptions::from_arguments(&json!({"budget": 42})).is_err());
		assert!(AgentOutputOptions::from_arguments(&json!({"budget": false})).is_err());
		assert!(AgentOutputOptions::from_arguments(&json!({"budget": null})).is_err());
	}

	#[test]
	fn json_output_ignores_the_budget_and_uses_full_projection() {
		for arguments in [
			json!({"format": "json"}),
			json!({"format": "json", "budget": "small"}),
			json!({"format": "json", "budget": "not-a-budget"}),
		] {
			let output = AgentOutputOptions::for_format(&arguments, OutputFormat::Json)
				.expect("JSON full projection");
			assert_eq!(output.budget, OutputBudget::Full);
			assert!(!output.compact);
		}
	}
}
