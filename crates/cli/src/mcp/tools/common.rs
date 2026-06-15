use std::collections::BTreeMap;

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
