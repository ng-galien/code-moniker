use std::borrow::Cow;
use std::path::Path;

use regex::Regex;

pub(crate) fn compile_glob(pattern: &str) -> Regex {
	Regex::new(&glob_to_regex(pattern)).expect("glob compiler emits valid regex")
}

fn glob_to_regex(pattern: &str) -> String {
	let mut out = String::from("^");
	let mut chars = pattern.chars().peekable();
	while let Some(ch) = chars.next() {
		match ch {
			'*' if chars.peek() == Some(&'*') => {
				chars.next();
				if chars.peek() == Some(&'/') {
					chars.next();
					out.push_str("(?:.*/)?");
				} else {
					out.push_str(".*");
				}
			}
			'*' => out.push_str("[^/]*"),
			'?' => out.push_str("[^/]"),
			other => out.push_str(&regex::escape(&other.to_string())),
		}
	}
	out.push('$');
	out
}

#[derive(Clone, Debug, Default)]
pub(crate) struct FilePathFilter {
	patterns: Vec<Regex>,
}

impl FilePathFilter {
	pub(crate) fn compile(patterns: &[String]) -> anyhow::Result<Self> {
		patterns
			.iter()
			.map(|pattern| Ok(compile_glob(&normalize_path_pattern(pattern)?)))
			.collect::<anyhow::Result<Vec<_>>>()
			.map(|patterns| Self { patterns })
	}

	pub(crate) fn matches(&self, rel_path: impl AsRef<Path>) -> bool {
		if self.patterns.is_empty() {
			return true;
		}
		let lossy = rel_path.as_ref().to_string_lossy();
		let rel = if lossy.contains('\\') {
			Cow::Owned(lossy.replace('\\', "/"))
		} else {
			lossy
		};
		self.patterns.iter().any(|pattern| pattern.is_match(&rel))
	}
}

fn normalize_path_pattern(pattern: &str) -> anyhow::Result<String> {
	let trimmed = pattern.trim();
	if trimmed.is_empty() {
		anyhow::bail!("path glob must not be empty");
	}
	let normalized = trimmed.replace('\\', "/");
	Ok(normalized
		.trim_start_matches("./")
		.trim_start_matches('/')
		.to_string())
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn empty_filter_matches_everything() {
		let filter = FilePathFilter::compile(&[]).unwrap();
		assert!(filter.matches("any/path.rs"));
	}

	#[test]
	fn relative_glob_scopes_to_subtree() {
		let filter = FilePathFilter::compile(&["pkg/src/**".to_string()]).unwrap();
		assert!(filter.matches("pkg/src/a.ts"));
		assert!(filter.matches("pkg/src/nested/a.ts"));
		assert!(!filter.matches("pkg/test/a.ts"));
	}

	#[test]
	fn single_star_stays_within_one_segment() {
		let filter = FilePathFilter::compile(&["**/fixtures/*.rs".to_string()]).unwrap();
		assert!(filter.matches("crates/core/tests/fixtures/accounts.rs"));
		assert!(!filter.matches("crates/core/tests/fixtures/rs/accounts.rs"));
	}

	#[test]
	fn double_star_slash_allows_empty_prefix() {
		assert_eq!(glob_to_regex("**/x"), "^(?:.*/)?x$");
	}

	#[test]
	fn metacharacters_are_escaped() {
		let filter = FilePathFilter::compile(&["a.txt".to_string()]).unwrap();
		assert!(filter.matches("a.txt"));
		assert!(!filter.matches("axtxt"));
	}

	#[test]
	fn matches_accepts_both_path_and_str() {
		let filter = FilePathFilter::compile(&["pkg/**".to_string()]).unwrap();
		assert!(filter.matches(Path::new("pkg/a.rs")));
		assert!(filter.matches("pkg/a.rs"));
	}

	#[test]
	fn leading_dot_slash_is_normalized() {
		let filter = FilePathFilter::compile(&["./pkg/**".to_string()]).unwrap();
		assert!(filter.matches("pkg/a.rs"));
	}

	#[test]
	fn backslash_separators_normalize_before_prefix_strip() {
		let filter = FilePathFilter::compile(&["\\pkg\\src\\**".to_string()]).unwrap();
		assert!(filter.matches("pkg/src/a.rs"));
		assert!(filter.matches("pkg/src/nested/a.rs"));
		assert!(!filter.matches("pkg/test/a.rs"));
	}

	#[test]
	fn leading_dot_backslash_is_normalized() {
		let filter = FilePathFilter::compile(&[".\\pkg\\**".to_string()]).unwrap();
		assert!(filter.matches("pkg/a.rs"));
	}
}
