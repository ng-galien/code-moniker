use super::model::SearchHit;
use crate::snapshot::model::{SymbolRecord, WorkspaceSnapshot};

pub struct SearchView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> SearchView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn search_symbols(&self, query: &str, limit: usize) -> Vec<SearchHit> {
		self.search_symbols_matching(query, limit, |_| true)
	}

	pub fn search_symbols_matching(
		&self,
		query: &str,
		limit: usize,
		mut matches: impl FnMut(&SymbolRecord) -> bool,
	) -> Vec<SearchHit> {
		let trimmed = query.trim();
		let raw = trimmed.to_ascii_lowercase();
		let terms = search_terms(trimmed);
		if raw.is_empty() || terms.is_empty() || limit == 0 {
			return Vec::new();
		}
		let mut hits = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.filter(|symbol| matches(symbol))
			.filter_map(|symbol| {
				let (score, reason) = score_symbol(symbol, &raw, &terms)?;
				Some((
					SearchHit {
						symbol: symbol.id.clone(),
						score,
						reason,
					},
					symbol.identity.as_str(),
				))
			})
			.collect::<Vec<_>>();
		hits.sort_by(|(left, left_identity), (right, right_identity)| {
			right
				.score
				.cmp(&left.score)
				.then_with(|| left_identity.cmp(right_identity))
		});
		hits.truncate(limit);
		hits.into_iter().map(|(hit, _)| hit).collect()
	}
}

fn search_terms(query: &str) -> Vec<String> {
	query
		.split(|c: char| !c.is_alphanumeric())
		.flat_map(split_camel_case)
		.filter(|term| !term.is_empty())
		.map(|term| term.to_ascii_lowercase())
		.collect()
}

fn split_camel_case(word: &str) -> Vec<&str> {
	let mut parts = Vec::new();
	let mut start = 0;
	let bytes = word.as_bytes();
	for idx in 1..bytes.len() {
		let boundary = bytes[idx].is_ascii_uppercase()
			&& (bytes[idx - 1].is_ascii_lowercase()
				|| bytes
					.get(idx + 1)
					.is_some_and(|next| next.is_ascii_lowercase()));
		if boundary {
			parts.push(&word[start..idx]);
			start = idx;
		}
	}
	parts.push(&word[start..]);
	parts
}

fn score_symbol(symbol: &SymbolRecord, phrase: &str, terms: &[String]) -> Option<(u32, String)> {
	let name = symbol.name.to_ascii_lowercase();
	let kind = symbol.kind.to_ascii_lowercase();
	let identity = symbol.identity.to_ascii_lowercase();
	let fields = [
		("name", name.as_str(), 120, 50),
		("kind", kind.as_str(), 35, 20),
		("identity", identity.as_str(), 20, 10),
	];
	let mut score = 0;
	let mut reason = None;
	for (label, value, exact_score, _) in fields {
		if let Some(field_score) = phrase_match_score(value, phrase, exact_score) {
			score += field_score;
			reason.get_or_insert(label);
		}
	}
	for term in terms {
		let mut matched = false;
		for (label, value, _, term_score) in fields {
			if value.contains(term) {
				score += term_score;
				matched = true;
				reason.get_or_insert(label);
			}
		}
		if !matched {
			return None;
		}
	}
	(score > 0).then(|| (score, reason.unwrap_or("match").to_string()))
}

fn phrase_match_score(value: &str, phrase: &str, exact_score: u32) -> Option<u32> {
	if value == phrase {
		return Some(exact_score * 2);
	}
	let start = value.find(phrase)?;
	let extra_len = value.chars().count().saturating_sub(phrase.chars().count()) as u32;
	let proximity_bonus = 30u32.saturating_sub(extra_len.min(30));
	let prefix_bonus = if start == 0 { 30 } else { 0 };
	Some(exact_score + prefix_bonus + proximity_bonus)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::snapshot::model::{
		ChangeOverlay, CodeIndex, LinkageSnapshot, ResourceGeneration, SourceCatalog, SourceId,
		SymbolId, WorkspaceTimings,
	};

	fn symbol(id: &str, name: &str, kind: &str, identity: &str) -> SymbolRecord {
		SymbolRecord {
			id: SymbolId::new(id),
			source: SourceId::new("source:test"),
			identity: identity.to_string(),
			name: name.to_string(),
			kind: kind.to_string(),
			visibility: String::new(),
			signature: String::new(),
			navigable: true,
			line_range: None,
			parent: None,
		}
	}

	fn score(symbol: &SymbolRecord, query: &str) -> u32 {
		let raw = query.to_ascii_lowercase();
		let terms = search_terms(&raw);
		score_symbol(symbol, &raw, &terms)
			.expect("symbol should match")
			.0
	}

	fn snapshot(symbols: Vec<SymbolRecord>) -> WorkspaceSnapshot {
		WorkspaceSnapshot {
			generation: ResourceGeneration::new(1),
			catalog: SourceCatalog::new(ResourceGeneration::new(1), Vec::new()),
			index: CodeIndex::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				symbols,
			),
			linkage: LinkageSnapshot::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				0,
				0,
			),
			changes: ChangeOverlay::new(
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				ResourceGeneration::new(1),
				Vec::new(),
			),
			timings: WorkspaceTimings::default(),
		}
	}

	#[test]
	fn exact_name_match_still_beats_partial_match() {
		let exact = symbol("exact", "parse", "fn", "code+moniker://./fn:parse");
		let partial = symbol(
			"partial",
			"parse_fragment",
			"fn",
			"code+moniker://./fn:parse_fragment",
		);

		assert!(score(&exact, "parse") > score(&partial, "parse"));
	}

	#[test]
	fn shorter_prefix_match_beats_longer_prefix_match() {
		let short = symbol("short", "parse_ast", "fn", "code+moniker://./fn:parse_ast");
		let long = symbol(
			"long",
			"parse_fragment_from_source",
			"fn",
			"code+moniker://./fn:parse_fragment_from_source",
		);

		assert!(score(&short, "parse") > score(&long, "parse"));
	}

	#[test]
	fn filtered_search_ranks_after_filtering() {
		let unfiltered_best = symbol("best", "parse", "fn", "code+moniker://./fn:parse");
		let filtered_hit = symbol(
			"target",
			"parse_fragment",
			"fn",
			"code+moniker://./module:target/fn:parse_fragment",
		);
		let snapshot = snapshot(vec![unfiltered_best, filtered_hit]);

		let hits = SearchView::new(&snapshot).search_symbols_matching("parse", 1, |symbol| {
			symbol.identity.contains("module:target")
		});

		assert_eq!(hits.len(), 1);
		assert_eq!(hits[0].symbol.as_str(), "target");
	}
}
