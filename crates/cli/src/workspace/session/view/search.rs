use super::model::SearchHit;
use crate::workspace::session::model::{SymbolRecord, WorkspaceSnapshot};

pub struct SearchView<'a> {
	snapshot: &'a WorkspaceSnapshot,
}

impl<'a> SearchView<'a> {
	pub(super) fn new(snapshot: &'a WorkspaceSnapshot) -> Self {
		Self { snapshot }
	}

	pub fn search_symbols(&self, query: &str, limit: usize) -> Vec<SearchHit> {
		let raw = query.trim().to_ascii_lowercase();
		let terms = search_terms(&raw);
		if raw.is_empty() || terms.is_empty() || limit == 0 {
			return Vec::new();
		}
		let mut hits = self
			.snapshot
			.index
			.symbols
			.iter()
			.filter(|symbol| symbol.navigable)
			.filter_map(|symbol| {
				let (score, reason) = score_symbol(symbol, &raw, &terms)?;
				Some(SearchHit {
					symbol: symbol.id.clone(),
					score,
					reason,
				})
			})
			.collect::<Vec<_>>();
		hits.sort_by(|left, right| {
			right.score.cmp(&left.score).then_with(|| {
				let left_symbol = self.symbol_identity(left);
				let right_symbol = self.symbol_identity(right);
				left_symbol.cmp(&right_symbol)
			})
		});
		hits.truncate(limit);
		hits
	}

	fn symbol_identity(&self, hit: &SearchHit) -> String {
		self.snapshot
			.index
			.symbols
			.iter()
			.find(|symbol| symbol.id == hit.symbol)
			.map(|symbol| symbol.identity.clone())
			.unwrap_or_else(|| hit.symbol.as_str().to_string())
	}
}

fn search_terms(query: &str) -> Vec<String> {
	query
		.split(|c: char| !c.is_alphanumeric())
		.filter(|term| !term.is_empty())
		.map(ToOwned::to_owned)
		.collect()
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
		if value == phrase {
			score += exact_score * 2;
			reason.get_or_insert(label);
		} else if value.contains(phrase) {
			score += exact_score;
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
