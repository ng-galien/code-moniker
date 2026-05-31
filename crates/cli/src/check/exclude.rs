use std::path::Path;

use regex::Regex;

use code_moniker_workspace::extract;

#[derive(Debug, Clone)]
pub(crate) struct UriExclusionMatcher {
	patterns: Vec<CompiledUriPattern>,
}

#[derive(Debug, Clone)]
struct CompiledUriPattern {
	regex: Regex,
}

impl UriExclusionMatcher {
	pub fn new(patterns: &[String]) -> Self {
		let patterns = patterns
			.iter()
			.map(|raw| CompiledUriPattern {
				regex: crate::glob::compile_glob(&normalize_uri(raw)),
			})
			.collect();
		Self { patterns }
	}

	pub fn matches_path(&self, path: &Path) -> bool {
		if self.patterns.is_empty() {
			return false;
		}
		let candidates = path_candidates(path);
		self.patterns.iter().any(|pattern| {
			candidates
				.iter()
				.any(|candidate| pattern.regex.is_match(candidate))
		})
	}
}

fn path_candidates(path: &Path) -> Vec<String> {
	let mut candidates = Vec::new();
	push_unique(&mut candidates, normalize_uri(&extract::file_uri(path)));
	push_unique(&mut candidates, normalize_path(path));
	if let Ok(abs) = path.canonicalize() {
		push_unique(&mut candidates, normalize_path(&abs));
		push_unique(&mut candidates, normalize_uri(&extract::file_uri(&abs)));
	}
	candidates
}

fn push_unique(values: &mut Vec<String>, value: String) {
	if !values.iter().any(|existing| existing == &value) {
		values.push(value);
	}
}

fn normalize_path(path: &Path) -> String {
	path.to_string_lossy().replace('\\', "/")
}

fn normalize_uri(value: &str) -> String {
	value.replace('\\', "/")
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn double_star_slash_can_match_no_prefix() {
		let matcher = UriExclusionMatcher::new(&["**/crates/core/tests/fixtures/**".to_string()]);

		assert!(matcher.matches_path(Path::new(
			"crates/core/tests/fixtures/extractors/rs/accounts.rs"
		)));
	}

	#[test]
	fn uri_pattern_matches_absolute_file_uri_candidate() {
		let matcher = UriExclusionMatcher::new(&["**/crates/core/tests/fixtures/**".to_string()]);
		let path =
			Path::new("/tmp/project/crates/core/tests/fixtures/extractors/java/UserService.java");

		assert!(matcher.matches_path(path));
	}

	#[test]
	fn single_star_stays_within_one_path_segment() {
		let matcher = UriExclusionMatcher::new(&["**/fixtures/*.rs".to_string()]);

		assert!(matcher.matches_path(Path::new("crates/core/tests/fixtures/accounts.rs")));
		assert!(!matcher.matches_path(Path::new(
			"crates/core/tests/fixtures/extractors/rs/accounts.rs"
		)));
	}
}
