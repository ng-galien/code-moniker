use std::collections::{BTreeMap, BTreeSet};

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

pub(in crate::mcp) fn apply_response_aliases<'a>(
	output: String,
	compact: bool,
	candidates: impl IntoIterator<Item = &'a str>,
) -> String {
	if !compact {
		return output;
	}
	let mut candidates = candidates
		.into_iter()
		.filter(|uri| uri.contains("+moniker://") && !uri.contains("://workspace"))
		.map(ToOwned::to_owned)
		.collect::<BTreeSet<_>>()
		.into_iter()
		.collect::<Vec<_>>();
	candidates.sort_by(|left, right| right.len().cmp(&left.len()).then_with(|| left.cmp(right)));
	let all_candidates = candidates.clone();

	let (aliasable_output, protected_calls) = protect_generated_calls(&output);
	let mut shadow = aliasable_output.clone();
	let mut repeated = Vec::new();
	for uri in candidates {
		let escaped = escape_call_fragment(&uri);
		let mut occurrences = shadow.matches(&uri).count();
		if escaped != uri {
			occurrences += shadow.matches(&escaped).count();
			shadow = shadow.replace(&escaped, "");
		}
		shadow = shadow.replace(&uri, "");
		if occurrences >= 2 && uri.len() >= 32 {
			repeated.push(uri);
		}
	}
	if repeated.is_empty() {
		return output;
	}

	repeated.sort_by(|left, right| {
		aliasable_output
			.find(left)
			.unwrap_or(usize::MAX)
			.cmp(&aliasable_output.find(right).unwrap_or(usize::MAX))
			.then_with(|| right.len().cmp(&left.len()))
	});
	let aliases = repeated
		.iter()
		.enumerate()
		.map(|(index, uri)| (uri.clone(), format!("@{}", index + 1)))
		.collect::<BTreeMap<_, _>>();

	let mut body = aliasable_output;
	let mut protected = Vec::new();
	for (index, uri) in all_candidates.into_iter().enumerate() {
		let escaped = escape_call_fragment(&uri);
		if let Some(alias) = aliases.get(&uri) {
			if escaped != uri {
				body = body.replace(&escaped, alias);
			}
			body = body.replace(&uri, alias);
			continue;
		}
		if escaped != uri {
			let marker = format!("\u{1f}escaped:{index}\u{1f}");
			body = body.replace(&escaped, &marker);
			protected.push((marker, escaped));
		}
		let marker = format!("\u{1f}raw:{index}\u{1f}");
		body = body.replace(&uri, &marker);
		protected.push((marker, uri));
	}
	for (marker, value) in protected {
		body = body.replace(&marker, &value);
	}
	for (marker, call) in protected_calls {
		body = body.replace(&marker, &call);
	}

	let mut compacted = String::from("aliases:\n");
	for uri in repeated {
		compacted.push_str(&format!("  {}: {uri}\n", aliases[&uri]));
	}
	compacted.push('\n');
	compacted.push_str(&body);
	compacted
}

fn protect_generated_calls(output: &str) -> (String, Vec<(String, String)>) {
	let mut body = String::with_capacity(output.len());
	let mut protected = Vec::new();
	for (index, line) in output.split_inclusive('\n').enumerate() {
		if line.contains("code_moniker_") {
			let marker = format!("\u{1e}call:{index}\u{1e}");
			body.push_str(&marker);
			protected.push((marker, line.to_string()));
		} else {
			body.push_str(line);
		}
	}
	(body, protected)
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
		apply_output_budget, apply_response_aliases, compact_argument, validate_output_budget,
	};
	use serde_json::json;

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
	fn response_aliases_replace_repeated_monikers_without_state() {
		let parent = "code+moniker://./lang:rs/module:mcp/struct:Server";
		let child = "code+moniker://./lang:rs/module:mcp/struct:Server/method:run()";
		let unique = "code+moniker://./lang:rs/module:mcp/fn:unique()";
		let output = format!(
			"uri: {parent}\ncontext: {parent}\nchild: {child}\nnext:\n  - code_moniker_read uri=\"{parent}\"\nunique: {unique}\n"
		);
		let compacted = apply_response_aliases(output.clone(), true, [parent, child, unique]);
		assert!(compacted.starts_with("aliases:\n  @1: "));
		assert!(
			compacted.contains(&format!("  @1: {parent}\n")),
			"{compacted}"
		);
		assert_eq!(compacted.matches("@1").count(), 3, "{compacted}");
		assert!(
			compacted.contains(&format!("code_moniker_read uri=\"{parent}\"")),
			"{compacted}"
		);
		assert!(
			compacted.contains(&format!("child: {child}\n")),
			"{compacted}"
		);
		assert_eq!(compacted.matches(unique).count(), 1, "{compacted}");
		assert_eq!(
			apply_response_aliases(output.clone(), false, [parent]),
			output
		);
	}
}
