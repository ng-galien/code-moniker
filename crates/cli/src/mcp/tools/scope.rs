use code_moniker_core::core::shape::Shape;
use code_moniker_query::{Page, QueryCursor, WorkspaceGeneration, split_csv};
use regex::Regex;
use serde::Serialize;
use serde_json::Value;

use code_moniker_workspace::glob::FilePathFilter;

use super::common::AgentOutputOptions;

pub(super) const MAX_LIMIT: usize = 500;

#[derive(Clone, Debug, Default)]
pub(in crate::mcp) struct ScopeFilter {
	pub(in crate::mcp) paths: Vec<String>,
	pub(in crate::mcp) langs: Vec<String>,
}

impl ScopeFilter {
	pub(in crate::mcp) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let paths = string_list(arguments, "path")?;
		let langs = string_list(arguments, "lang")?
			.into_iter()
			.map(|lang| lang.to_ascii_lowercase())
			.collect::<Vec<_>>();
		FilePathFilter::compile(&paths)?;
		Ok(Self { paths, langs })
	}

	pub(super) fn describe(&self) -> Vec<String> {
		let mut lines = Vec::new();
		if !self.paths.is_empty() {
			lines.push(format!("  path: {}", self.paths.join(", ")));
		}
		if !self.langs.is_empty() {
			lines.push(format!("  lang: {}", self.langs.join(", ")));
		}
		if lines.is_empty() {
			lines.push("  path: *".to_string());
			lines.push("  lang: *".to_string());
		}
		lines
	}

	pub(super) fn append_call_args(&self, output: &mut String) {
		append_repeated_call_args(output, "path", &self.paths);
		append_repeated_call_args(output, "lang", &self.langs);
	}
}

#[derive(Clone, Debug)]
pub(in crate::mcp) struct SymbolScopeFilter {
	pub(in crate::mcp) files: ScopeFilter,
	pub(in crate::mcp) kinds: Vec<String>,
	pub(in crate::mcp) shapes: Vec<Shape>,
	pub(in crate::mcp) name: Option<Regex>,
	pub(in crate::mcp) include_non_navigable: bool,
}

impl SymbolScopeFilter {
	pub(in crate::mcp) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let files = ScopeFilter::from_arguments(arguments)?;
		let kinds = string_list(arguments, "kind")?;
		let shapes = string_list(arguments, "shape")?
			.into_iter()
			.map(|shape| {
				shape
					.parse::<Shape>()
					.map_err(|err| anyhow::anyhow!("invalid shape `{shape}`: {err}"))
			})
			.collect::<anyhow::Result<Vec<_>>>()?;
		let name = regex_argument(arguments, "name", "name")?;
		let include_non_navigable = arguments
			.get("include_non_navigable")
			.and_then(Value::as_bool)
			.unwrap_or(false);
		Ok(Self {
			files,
			kinds,
			shapes,
			name,
			include_non_navigable,
		})
	}

	pub(super) fn describe(&self) -> Vec<String> {
		let mut lines = self.files.describe();
		if !self.kinds.is_empty() {
			lines.push(format!("  kind: {}", self.kinds.join(", ")));
		}
		if !self.shapes.is_empty() {
			let shapes = self
				.shapes
				.iter()
				.map(|shape| shape.as_str())
				.collect::<Vec<_>>();
			lines.push(format!("  shape: {}", shapes.join(", ")));
		}
		if let Some(name) = &self.name {
			lines.push(format!("  name: {}", name.as_str()));
		}
		lines
	}

	pub(super) fn append_call_args(&self, output: &mut String) {
		self.files.append_call_args(output);
		append_repeated_call_args(output, "kind", &self.kinds);
		let shapes = self
			.shapes
			.iter()
			.map(|shape| shape.as_str().to_string())
			.collect::<Vec<_>>();
		append_repeated_call_args(output, "shape", &shapes);
		if let Some(name) = &self.name {
			append_call_string_arg(output, "name", name.as_str());
		}
		if self.include_non_navigable {
			append_call_bool_arg(output, "include_non_navigable", true);
		}
	}
}

#[derive(Serialize)]
pub(super) struct ScopeRowView {
	label: String,
	value: String,
}

pub(super) fn scope_rows(scope: &SymbolScopeFilter) -> Vec<ScopeRowView> {
	scope
		.describe()
		.into_iter()
		.filter_map(|line| {
			let (label, value) = line.trim().split_once(": ")?;
			Some(ScopeRowView {
				label: label.to_string(),
				value: value.to_string(),
			})
		})
		.collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) struct Paging {
	pub(in crate::mcp) cursor: usize,
	pub(in crate::mcp) generation: Option<WorkspaceGeneration>,
	pub(in crate::mcp) limit: usize,
}

impl Paging {
	pub(in crate::mcp) fn from_arguments_for_volume(
		arguments: &Value,
		output: AgentOutputOptions,
	) -> anyhow::Result<Self> {
		let profile_limit = output.default_page_limit();
		let mut paging = Self::from_arguments_with_default(arguments, profile_limit)?;
		paging.limit = paging.limit.min(profile_limit);
		Ok(paging)
	}

	fn from_arguments_with_default(
		arguments: &Value,
		default_limit: usize,
	) -> anyhow::Result<Self> {
		let (cursor, generation) = cursor_argument(arguments, "cursor")?.unwrap_or((0, None));
		let limit = positive_number_argument(arguments, "limit")?
			.unwrap_or(default_limit)
			.min(MAX_LIMIT);
		Ok(Self {
			cursor,
			generation,
			limit,
		})
	}

	pub(super) fn daemon_page(&self) -> Page {
		Page {
			cursor: (self.cursor > 0).then(|| QueryCursor::new(self.cursor, self.generation)),
			limit: self.limit,
		}
	}
}

pub(super) fn string_list(arguments: &Value, key: &str) -> anyhow::Result<Vec<String>> {
	let Some(value) = arguments.get(key) else {
		return Ok(Vec::new());
	};
	match value {
		Value::String(value) => Ok(split_csv(value)),
		Value::Array(values) => values
			.iter()
			.map(|value| {
				value
					.as_str()
					.map(split_csv)
					.ok_or_else(|| anyhow::anyhow!("`{key}` entries must be strings"))
			})
			.collect::<anyhow::Result<Vec<_>>>()
			.map(|nested| nested.into_iter().flatten().collect()),
		_ => anyhow::bail!("`{key}` must be a string or string array"),
	}
}

pub(super) fn regex_argument(
	arguments: &Value,
	key: &str,
	label: &str,
) -> anyhow::Result<Option<Regex>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	let raw = value
		.as_str()
		.ok_or_else(|| anyhow::anyhow!("`{key}` must be a string"))?;
	Regex::new(raw)
		.map(Some)
		.map_err(|err| anyhow::anyhow!("invalid {label} regex: {err}"))
}

pub(super) fn append_call_string_arg(output: &mut String, key: &str, value: &str) {
	output.push(' ');
	output.push_str(key);
	output.push('=');
	output
		.push_str(&serde_json::to_string(value).expect("serializing a string to JSON cannot fail"));
}

pub(super) fn append_call_number_arg(output: &mut String, key: &str, value: usize) {
	output.push(' ');
	output.push_str(key);
	output.push('=');
	output.push_str(&value.to_string());
}

pub(super) fn append_call_cursor_arg(output: &mut String, key: &str, cursor: &QueryCursor) {
	if let Some(generation) = cursor.generation {
		append_call_string_arg(output, key, &format!("{}:{}", generation.0, cursor.offset));
	} else {
		append_call_number_arg(output, key, cursor.offset);
	}
}

pub(super) fn append_call_bool_arg(output: &mut String, key: &str, value: bool) {
	output.push(' ');
	output.push_str(key);
	output.push('=');
	output.push_str(if value { "true" } else { "false" });
}

pub(super) fn append_repeated_call_args(output: &mut String, key: &str, values: &[String]) {
	for value in values {
		append_call_string_arg(output, key, value);
	}
}

fn number_argument(arguments: &Value, key: &str) -> anyhow::Result<Option<usize>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	match value {
		Value::Number(number) => number
			.as_u64()
			.map(|n| Some(n as usize))
			.ok_or_else(|| anyhow::anyhow!("`{key}` must be a positive integer")),
		Value::String(raw) => raw
			.parse::<usize>()
			.map(Some)
			.map_err(|err| anyhow::anyhow!("invalid `{key}` value `{raw}`: {err}")),
		_ => anyhow::bail!("`{key}` must be an integer"),
	}
}

pub(super) fn cursor_argument(
	arguments: &Value,
	key: &str,
) -> anyhow::Result<Option<(usize, Option<WorkspaceGeneration>)>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	match value {
		Value::Number(number) => number
			.as_u64()
			.map(|n| Some((n as usize, None)))
			.ok_or_else(|| anyhow::anyhow!("`{key}` must be a positive integer")),
		Value::String(raw) => parse_cursor(raw).map(Some),
		_ => anyhow::bail!("`{key}` must be an integer or generation cursor"),
	}
}

fn parse_cursor(raw: &str) -> anyhow::Result<(usize, Option<WorkspaceGeneration>)> {
	if let Some((generation, offset)) = raw.split_once(':') {
		let generation = generation
			.parse::<u64>()
			.map_err(|err| anyhow::anyhow!("invalid `cursor` generation `{generation}`: {err}"))?;
		let offset = offset
			.parse::<usize>()
			.map_err(|err| anyhow::anyhow!("invalid `cursor` offset `{offset}`: {err}"))?;
		Ok((offset, Some(WorkspaceGeneration(generation))))
	} else {
		let offset = raw
			.parse::<usize>()
			.map_err(|err| anyhow::anyhow!("invalid `cursor` value `{raw}`: {err}"))?;
		Ok((offset, None))
	}
}

fn positive_number_argument(arguments: &Value, key: &str) -> anyhow::Result<Option<usize>> {
	let value = number_argument(arguments, key)?;
	if matches!(value, Some(0)) {
		anyhow::bail!("`{key}` must be greater than zero");
	}
	Ok(value)
}
