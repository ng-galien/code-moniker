use std::collections::{BTreeMap, BTreeSet};

use serde_json::Value;

use code_moniker_workspace::snapshot::SymbolRecord;

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
	use super::{apply_response_aliases, compact_argument};
	use serde_json::json;

	#[test]
	fn compact_defaults_true_and_rejects_non_boolean_values() {
		assert!(compact_argument(&json!({})).unwrap());
		assert!(!compact_argument(&json!({"compact": false})).unwrap());
		assert!(compact_argument(&json!({"compact": "yes"})).is_err());
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
