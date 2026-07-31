use std::collections::{BTreeMap, BTreeSet};

use code_moniker_workspace::code::compact_identity;
use serde_json::Value;

use code_moniker_workspace::snapshot::SymbolRecord;

const SMALL_OUTPUT_CHARS: usize = 8_000;
const MEDIUM_OUTPUT_CHARS: usize = 20_000;
const FULL_OUTPUT_CHARS: usize = 64_000;
const MIN_OUTPUT_CHARS: usize = 1_000;
const MAX_OUTPUT_CHARS: usize = 100_000;

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
			"description": "Hard response budget. small=8000, medium=20000, full=64000 characters; full is opt-in."
		}),
	);
	properties.insert(
		"max_chars".to_string(),
		serde_json::json!({
			"type": "integer",
			"minimum": MIN_OUTPUT_CHARS,
			"maximum": MAX_OUTPUT_CHARS,
			"description": "Explicit hard character ceiling overriding budget."
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

pub(in crate::mcp) fn apply_output_budget(
	output: String,
	arguments: &Value,
) -> anyhow::Result<String> {
	let max_chars = output_budget_chars(arguments)?;
	let original_chars = output.chars().count();
	if original_chars <= max_chars {
		return Ok(output);
	}
	let suffix = format!(
		"\n\nbudget:\n  max_chars: {max_chars}\n  original_chars: {original_chars}\n  truncated_by: max_chars\n"
	);
	let omission = "\n… output omitted by budget …\n";
	let reserved = suffix.chars().count() + omission.chars().count();
	if reserved >= max_chars {
		return Ok(take_chars(&suffix, max_chars));
	}
	let available = max_chars - reserved;
	let next = output
		.find("\nnext:\n")
		.map(|offset| output[offset..].to_string())
		.filter(|tail| tail.chars().count() <= available / 3);
	let body = match next {
		Some(tail) => {
			let head_chars = available.saturating_sub(tail.chars().count());
			format!(
				"{}{}{}{}",
				take_chars(&output, head_chars),
				omission,
				tail,
				suffix
			)
		}
		None => format!("{}{}{}", take_chars(&output, available), omission, suffix),
	};
	Ok(body)
}

pub(in crate::mcp) fn validate_output_budget(arguments: &Value) -> anyhow::Result<()> {
	output_budget_chars(arguments).map(|_| ())
}

fn output_budget_chars(arguments: &Value) -> anyhow::Result<usize> {
	if let Some(value) = arguments.get("max_chars") {
		let Some(value) = value.as_u64() else {
			anyhow::bail!("`max_chars` must be an integer");
		};
		let value = value as usize;
		if !(MIN_OUTPUT_CHARS..=MAX_OUTPUT_CHARS).contains(&value) {
			anyhow::bail!("`max_chars` must be between {MIN_OUTPUT_CHARS} and {MAX_OUTPUT_CHARS}");
		}
		return Ok(value);
	}
	match arguments.get("budget") {
		None => Ok(SMALL_OUTPUT_CHARS),
		Some(Value::String(value)) if value == "small" => Ok(SMALL_OUTPUT_CHARS),
		Some(Value::String(value)) if value == "medium" => Ok(MEDIUM_OUTPUT_CHARS),
		Some(Value::String(value)) if value == "full" => Ok(FULL_OUTPUT_CHARS),
		Some(Value::String(value)) => anyhow::bail!("unknown output budget `{value}`"),
		Some(_) => anyhow::bail!("`budget` must be a string"),
	}
}

fn take_chars(value: &str, count: usize) -> String {
	value.chars().take(count).collect()
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

pub(in crate::mcp) fn symbol_line_suffix(symbol: &SymbolRecord) -> String {
	line_range_suffix(symbol.line_range)
}

pub(in crate::mcp) fn sorted_count_rows<K>(counts: &BTreeMap<K, usize>) -> Vec<(String, usize)>
where
	K: ToString,
{
	let mut rows = counts
		.iter()
		.map(|(name, count)| (name.to_string(), *count))
		.collect::<Vec<_>>();
	rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
	rows
}

pub(in crate::mcp) fn compact_argument(arguments: &Value) -> anyhow::Result<bool> {
	match arguments.get("compact") {
		None => Ok(true),
		Some(Value::Bool(value)) => Ok(*value),
		Some(_) => anyhow::bail!("`compact` must be a boolean"),
	}
}

pub(in crate::mcp) fn compact_response_monikers<'a>(
	output: String,
	compact: bool,
	scheme: &str,
	candidates: impl IntoIterator<Item = &'a str>,
) -> String {
	if !compact {
		return output;
	}
	let mut candidates = candidates
		.into_iter()
		.filter_map(|uri| compact_uri(uri).map(|compact| (uri.to_owned(), compact)))
		.chain(std::iter::once((
			format!("{scheme}workspace"),
			"workspace".to_string(),
		)))
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();
	candidates.sort_by(|left, right| {
		right
			.0
			.len()
			.cmp(&left.0.len())
			.then_with(|| left.0.cmp(&right.0))
	});

	let (mut body, protected_lines) = protect_opaque_lines(&output, scheme);
	for (uri, compact) in candidates {
		let escaped = escape_call_fragment(&uri);
		if escaped != uri {
			body = body.replace(&escaped, &escape_call_fragment(&compact));
		}
		body = body.replace(&uri, &compact);
	}
	for (marker, line) in protected_lines {
		body = body.replace(&marker, &line);
	}
	body
}

fn compact_uri(uri: &str) -> Option<String> {
	if !uri.contains("+moniker://") || uri.contains("://workspace") {
		return None;
	}
	let scheme_end = uri.find("://")?.checked_add(3)?;
	let scheme = uri.get(..scheme_end)?;
	compact_identity(uri, scheme)
}

fn protect_opaque_lines(output: &str, scheme: &str) -> (String, Vec<(String, String)>) {
	let mut body = String::with_capacity(output.len());
	let mut protected = Vec::new();
	for (index, line) in output.split_inclusive('\n').enumerate() {
		if is_source_line(line)
			|| is_syntax_text_line(line)
			|| is_workspace_header_line(line, scheme)
		{
			let marker = format!("\u{1e}opaque:{index}\u{1e}");
			body.push_str(&marker);
			protected.push((marker, line.to_string()));
		} else {
			body.push_str(line);
		}
	}
	(body, protected)
}

fn is_source_line(line: &str) -> bool {
	let line = line.trim_start();
	let Some((number, _)) = line.split_once('|') else {
		return false;
	};
	!number.is_empty() && number.trim_end().chars().all(|ch| ch.is_ascii_digit())
}

fn is_syntax_text_line(line: &str) -> bool {
	let line = line.trim_start();
	line.starts_with("- ") && line.contains(" text=\"")
}

fn is_workspace_header_line(line: &str, scheme: &str) -> bool {
	let Some(uri) = line.strip_prefix("uri: ").map(str::trim_end) else {
		return false;
	};
	let workspace = format!("{scheme}workspace");
	uri == workspace || uri.starts_with(&format!("{workspace}/"))
}

fn escape_call_fragment(value: &str) -> String {
	let mut escaped = String::with_capacity(value.len());
	for ch in value.chars() {
		match ch {
			'\\' => escaped.push_str("\\\\"),
			'"' => escaped.push_str("\\\""),
			_ => escaped.push(ch),
		}
	}
	escaped
}

#[cfg(test)]
mod tests {
	use super::{
		apply_output_budget, compact_argument, compact_response_monikers, validate_output_budget,
	};
	use serde_json::json;

	const SCHEME: &str = "code+moniker://";

	#[test]
	fn compact_defaults_true_and_rejects_non_boolean_values() {
		assert!(compact_argument(&json!({})).unwrap());
		assert!(!compact_argument(&json!({"compact": false})).unwrap());
		assert!(compact_argument(&json!({"compact": "yes"})).is_err());
	}

	#[test]
	fn output_budget_is_hard_and_preserves_a_small_next_block() {
		let output = format!(
			"header\n{}\nnext:\n  - code_moniker_read uri=\"code+moniker://workspace\"\n",
			"row\n".repeat(3_000)
		);
		let bounded = apply_output_budget(output, &json!({"max_chars": 1200})).unwrap();
		assert!(
			bounded.chars().count() <= 1200,
			"{}",
			bounded.chars().count()
		);
		assert!(bounded.contains("truncated_by: max_chars"), "{bounded}");
		assert!(bounded.contains("code_moniker_read"), "{bounded}");
	}

	#[test]
	fn output_budget_defaults_small_and_leaves_short_output_untouched() {
		let output = "short response\n".to_string();
		assert_eq!(
			apply_output_budget(output.clone(), &json!({})).unwrap(),
			output
		);
	}

	#[test]
	fn output_budget_rejects_non_string_values() {
		assert!(validate_output_budget(&json!({"budget": 42})).is_err());
		assert!(validate_output_budget(&json!({"budget": false})).is_err());
		assert!(validate_output_budget(&json!({"budget": null})).is_err());
	}

	#[test]
	fn compact_response_monikers_shorten_each_body_uri_and_generated_call() {
		let parent = "code+moniker://./lang:rs/module:mcp/struct:Server";
		let child = "code+moniker://./lang:rs/module:mcp/struct:Server/method:run()";
		let unique = "code+moniker://./lang:rs/module:mcp/fn:unique()";
		let output = format!(
			"uri: {parent}\ncontext: {parent}\nchild: {child}\nnext:\n  - code_moniker_read uri=\"{parent}\"\nunique: {unique}\n"
		);
		let compacted =
			compact_response_monikers(output.clone(), true, SCHEME, [parent, child, unique]);
		assert_eq!(compacted.matches("rs:mcp.struct:Server").count(), 4);
		assert!(compacted.contains("child: rs:mcp.struct:Server/method:run()"));
		assert!(compacted.contains("unique: rs:mcp.fn:unique()"));
		assert!(compacted.contains("code_moniker_read uri=\"rs:mcp.struct:Server\""));
		assert_eq!(
			compact_response_monikers(output.clone(), false, SCHEME, [parent]),
			output
		);
	}

	#[test]
	fn compact_response_monikers_preserve_uri_type_text_in_compact_generated_calls() {
		let uri = concat!(
			"code+moniker://./lang:rs/module:mcp/fn:render(",
			"cursor:&code_moniker_query::QueryCursor)"
		);
		let output = format!("uri: {uri}\nnext:\n  - code_moniker_read uri=\"{uri}\"\n");
		let compacted = compact_response_monikers(output, true, SCHEME, [uri]);

		assert!(
			compacted.contains("uri: rs:mcp.fn:render(cursor:&code_moniker_query::QueryCursor)"),
			"{compacted}"
		);
		assert!(compacted.contains(concat!(
			"code_moniker_read uri=\"",
			"rs:mcp.fn:render(cursor:&code_moniker_query::QueryCursor)\""
		)));
	}

	#[test]
	fn compact_response_monikers_use_workspace_alias_in_generated_calls() {
		let output = concat!(
			"uri: code+moniker://workspace\n",
			"next:\n",
			"  - code_moniker_read uri=\"code+moniker://workspace\" depth=3\n",
			"  - code_moniker_read uri=\"code+moniker://workspace/views\"\n"
		)
		.to_string();
		let compacted = compact_response_monikers(output, true, SCHEME, std::iter::empty());

		assert!(compacted.contains("uri: code+moniker://workspace\n"));
		assert!(compacted.contains("code_moniker_read uri=\"workspace\" depth=3"));
		assert!(compacted.contains("code_moniker_read uri=\"workspace/views\""));
		assert!(
			compacted
				.lines()
				.filter(|line| line.trim_start().starts_with("- code_moniker_"))
				.all(|line| !line.contains("+moniker://")),
			"{compacted}"
		);
	}

	#[test]
	fn compact_response_monikers_derive_workspace_alias_from_the_active_scheme() {
		let scheme = "custom+moniker://";
		let output = concat!(
			"uri: custom+moniker://workspace/views\n",
			"next:\n",
			"  - code_moniker_read uri=\"custom+moniker://workspace/views/java-app\"\n"
		)
		.to_string();
		let compacted = compact_response_monikers(output, true, scheme, std::iter::empty());

		assert!(compacted.contains("uri: custom+moniker://workspace/views\n"));
		assert!(compacted.contains("code_moniker_read uri=\"workspace/views/java-app\""));
	}

	#[test]
	fn compact_response_monikers_preserve_canonical_uri_literals_in_source_lines() {
		let uri = "code+moniker://./lang:rs/module:mcp/struct:Server";
		let output = format!("uri: {uri}\n305 | let value = \"{uri}\";\n");
		let compacted = compact_response_monikers(output, true, SCHEME, [uri]);

		assert!(
			compacted.contains("uri: rs:mcp.struct:Server"),
			"{compacted}"
		);
		assert!(
			compacted.contains(&format!("305 | let value = \"{uri}\";")),
			"{compacted}"
		);
	}

	#[test]
	fn compact_response_monikers_preserve_canonical_uri_literals_in_syntax_leaf_text() {
		let uri = "code+moniker://./lang:rs/module:mcp/struct:Server";
		let output = format!("uri: {uri}\ntree:\n  - string_literal 1:0-1:42 text=\"{uri}\"\n");
		let compacted = compact_response_monikers(output, true, SCHEME, [uri]);

		assert!(
			compacted.contains("uri: rs:mcp.struct:Server"),
			"{compacted}"
		);
		assert!(
			compacted.contains(&format!("text=\"{uri}\"")),
			"{compacted}"
		);
	}
}
