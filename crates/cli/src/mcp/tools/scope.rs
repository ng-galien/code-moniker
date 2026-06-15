use std::path::Path;

use code_moniker_core::core::shape::Shape;
use code_moniker_query::{Page, QueryCursor, WorkspaceGeneration};
use regex::Regex;
use serde_json::Value;

use code_moniker_workspace::glob::FilePathFilter;

pub(super) const DEFAULT_LIMIT: usize = 80;
pub(super) const MAX_LIMIT: usize = 500;

#[derive(Clone, Debug, Default)]
pub(in crate::mcp) struct ScopeFilter {
	pub(in crate::mcp) paths: Vec<String>,
	pub(in crate::mcp) langs: Vec<String>,
	path_filter: FilePathFilter,
}

impl ScopeFilter {
	pub(in crate::mcp) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let paths = string_list(arguments, "path")?;
		let langs = string_list(arguments, "lang")?
			.into_iter()
			.map(|lang| lang.to_ascii_lowercase())
			.collect::<Vec<_>>();
		let path_filter = FilePathFilter::compile(&paths)?;
		Ok(Self {
			paths,
			langs,
			path_filter,
		})
	}

	pub(super) fn matches_file(&self, rel_path: &str, language: Option<&str>) -> bool {
		self.path_filter.matches(rel_path)
			&& (self.langs.is_empty()
				|| language.is_some_and(|lang| self.langs.iter().any(|allowed| allowed == lang)))
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

	pub(super) fn matches_symbol(&self, symbol: SymbolMatch<'_>) -> bool {
		self.matches_symbol_base(symbol) && self.matches_kind_and_shape(symbol.kind)
	}

	pub(super) fn matches_tui_search_symbol(&self, symbol: SymbolMatch<'_>) -> bool {
		self.matches_symbol_base(symbol) && self.matches_kind_or_shape(symbol.kind)
	}

	fn matches_symbol_base(&self, symbol: SymbolMatch<'_>) -> bool {
		(self.include_non_navigable || symbol.navigable)
			&& self
				.name
				.as_ref()
				.is_none_or(|regex| regex.is_match(symbol.name))
	}

	fn matches_kind_and_shape(&self, kind: &str) -> bool {
		(self.kinds.is_empty() || self.kinds.iter().any(|allowed| allowed == kind))
			&& (self.shapes.is_empty()
				|| self
					.shapes
					.iter()
					.any(|shape| *shape == Shape::for_kind(kind.as_bytes())))
	}

	fn matches_kind_or_shape(&self, kind: &str) -> bool {
		let has_kind_filter = !self.kinds.is_empty() || !self.shapes.is_empty();
		!has_kind_filter
			|| self.kinds.iter().any(|allowed| allowed == kind)
			|| self
				.shapes
				.iter()
				.any(|shape| *shape == Shape::for_kind(kind.as_bytes()))
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

#[derive(Clone, Copy, Debug)]
pub(super) struct SymbolMatch<'a> {
	pub(super) name: &'a str,
	pub(super) kind: &'a str,
	pub(super) navigable: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::mcp) struct Paging {
	pub(in crate::mcp) cursor: usize,
	pub(in crate::mcp) generation: Option<WorkspaceGeneration>,
	pub(in crate::mcp) limit: usize,
}

impl Paging {
	pub(super) fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		let (cursor, generation) = cursor_argument(arguments, "cursor")?.unwrap_or((0, None));
		let limit = positive_number_argument(arguments, "limit")?
			.unwrap_or(DEFAULT_LIMIT)
			.min(MAX_LIMIT);
		Ok(Self {
			cursor,
			generation,
			limit,
		})
	}

	pub(super) fn window<T>(&self, items: &[T]) -> (usize, usize, Option<usize>) {
		let start = self.cursor.min(items.len());
		let end = start.saturating_add(self.limit).min(items.len());
		let next = (end < items.len()).then_some(end);
		(start, end, next)
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
	output.push_str("=\"");
	for ch in value.chars() {
		match ch {
			'\\' => output.push_str("\\\\"),
			'"' => output.push_str("\\\""),
			_ => output.push(ch),
		}
	}
	output.push('"');
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

fn split_csv(value: &str) -> Vec<String> {
	value
		.split(',')
		.map(str::trim)
		.filter(|part| !part.is_empty())
		.map(ToOwned::to_owned)
		.collect()
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

fn cursor_argument(
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

pub(super) fn path_prefix(rel_path: &str) -> String {
	let mut parts = Path::new(rel_path)
		.components()
		.filter_map(|component| component.as_os_str().to_str())
		.take(2)
		.collect::<Vec<_>>();
	if parts.is_empty() {
		"<root>".to_string()
	} else if parts.len() == 1 {
		parts.remove(0).to_string()
	} else {
		parts.join("/")
	}
}
