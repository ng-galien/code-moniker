#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Dep {
	pub name: String,
	pub version: Option<String>,
	pub dep_kind: String,
	pub import_root: String,
}

#[derive(Debug)]
pub enum GoModError {
	Schema(String),
}

impl std::fmt::Display for GoModError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::Schema(s) => write!(f, "go.mod schema error: {s}"),
		}
	}
}

impl std::error::Error for GoModError {}

pub fn parse(content: &str) -> Result<Vec<Dep>, GoModError> {
	let mut out = Vec::new();
	let mut block: Option<Block> = None;

	for raw_line in content.lines() {
		let (clean, comment) = split_comment(raw_line);
		let trimmed = clean.trim();
		if trimmed.is_empty() {
			continue;
		}

		if let Some(b) = block {
			if trimmed == ")" {
				block = None;
				continue;
			}
			match b {
				Block::Require => {
					if let Some(dep) = parse_require_entry(trimmed, comment) {
						out.push(dep);
					}
				}
				Block::Other => {}
			}
			block = Some(b);
			continue;
		}

		if let Some(rest) = trimmed.strip_prefix("module") {
			let path = rest.trim().trim_matches('"');
			if !path.is_empty() {
				out.push(Dep {
					name: path.into(),
					version: None,
					dep_kind: "package".into(),
					import_root: path.into(),
				});
			}
			continue;
		}

		if let Some(rest) = trimmed.strip_prefix("require") {
			let rest = rest.trim();
			if rest == "(" {
				block = Some(Block::Require);
				continue;
			}
			if let Some(dep) = parse_require_entry(rest, comment) {
				out.push(dep);
			}
			continue;
		}

		if trimmed.starts_with("replace")
			|| trimmed.starts_with("exclude")
			|| trimmed.starts_with("retract")
		{
			let rest = trimmed.split_whitespace().nth(1).unwrap_or("");
			if rest == "(" {
				block = Some(Block::Other);
			}
			continue;
		}

		if trimmed.starts_with("go ") || trimmed.starts_with("toolchain ") {
			continue;
		}
	}

	Ok(out)
}

#[derive(Clone, Copy)]
enum Block {
	Require,
	Other,
}

fn parse_require_entry(line: &str, comment: &str) -> Option<Dep> {
	let mut parts = line.split_whitespace();
	let path = parts.next()?.trim_matches('"');
	let version = parts.next()?.trim_matches('"');
	if path.is_empty() || version.is_empty() {
		return None;
	}
	let dep_kind = if comment.trim() == "indirect" {
		"indirect"
	} else {
		"normal"
	};
	Some(Dep {
		name: path.into(),
		version: Some(version.into()),
		dep_kind: dep_kind.into(),
		import_root: path.into(),
	})
}

pub fn package_moniker(project: &[u8], import_root: &str) -> crate::core::moniker::Moniker {
	let mut b = crate::core::moniker::MonikerBuilder::new();
	b.project(project);
	let mut pieces = import_root.split('/').filter(|s| !s.is_empty());
	if let Some(head) = pieces.next() {
		b.segment(crate::lang::kinds::EXTERNAL_PKG, head.as_bytes());
		for piece in pieces {
			b.segment(crate::lang::kinds::PATH, piece.as_bytes());
		}
	}
	b.build()
}

fn split_comment(line: &str) -> (&str, &str) {
	if let Some(idx) = line.find("//") {
		(&line[..idx], &line[idx + 2..])
	} else {
		(line, "")
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn parse_empty_returns_empty_vec() {
		assert!(parse("").unwrap().is_empty());
	}

	#[test]
	fn parse_module_only_emits_package_dep() {
		let src = "module github.com/foo/bar\n\ngo 1.21\n";
		let deps = parse(src).unwrap();
		assert_eq!(
			deps,
			vec![Dep {
				name: "github.com/foo/bar".into(),
				version: None,
				dep_kind: "package".into(),
				import_root: "github.com/foo/bar".into(),
			}]
		);
	}

	#[test]
	fn parse_single_line_require() {
		let src = "module foo\n\nrequire gopkg.in/x v1.0.0\n";
		let deps = parse(src).unwrap();
		let req = deps.iter().find(|d| d.name == "gopkg.in/x").unwrap();
		assert_eq!(req.version.as_deref(), Some("v1.0.0"));
		assert_eq!(req.dep_kind, "normal");
		assert_eq!(req.import_root, "gopkg.in/x");
	}

	#[test]
	fn parse_block_require_multiple_entries() {
		let src = "module foo\n\nrequire (\n\tgithub.com/x/y v1.2.3\n\tgithub.com/a/b v0.5.0\n)\n";
		let deps = parse(src).unwrap();
		assert!(
			deps.iter()
				.any(|d| d.name == "github.com/x/y" && d.version.as_deref() == Some("v1.2.3"))
		);
		assert!(
			deps.iter()
				.any(|d| d.name == "github.com/a/b" && d.version.as_deref() == Some("v0.5.0"))
		);
	}

	#[test]
	fn parse_indirect_marker_sets_dep_kind() {
		let src = "module foo\n\nrequire (\n\tgithub.com/x/y v1.2.3 // indirect\n)\n";
		let deps = parse(src).unwrap();
		let req = deps.iter().find(|d| d.name == "github.com/x/y").unwrap();
		assert_eq!(req.dep_kind, "indirect");
	}

	#[test]
	fn parse_inline_indirect_on_single_line_require() {
		let src = "module foo\n\nrequire github.com/x/y v1.0.0 // indirect\n";
		let deps = parse(src).unwrap();
		let req = deps.iter().find(|d| d.name == "github.com/x/y").unwrap();
		assert_eq!(req.dep_kind, "indirect");
	}

	#[test]
	fn parse_skips_replace_block() {
		let src = "module foo\n\nrequire github.com/x v1.0.0\n\nreplace (\n\tgithub.com/old => github.com/new v2.0.0\n)\n\nrequire github.com/z v3.0.0\n";
		let deps = parse(src).unwrap();
		let names: Vec<&str> = deps.iter().map(|d| d.name.as_str()).collect();
		assert!(names.contains(&"github.com/x"));
		assert!(names.contains(&"github.com/z"));
		assert!(!names.contains(&"github.com/old"));
		assert!(!names.contains(&"github.com/new"));
	}

	#[test]
	fn parse_skips_replace_single_line() {
		let src = "module foo\n\nreplace github.com/old => github.com/new v2.0.0\n\nrequire github.com/x v1.0.0\n";
		let deps = parse(src).unwrap();
		assert!(deps.iter().any(|d| d.name == "github.com/x"));
		assert!(!deps.iter().any(|d| d.name == "github.com/old"));
	}

	#[test]
	fn parse_skips_go_and_toolchain_directives() {
		let src = "module foo\n\ngo 1.21\ntoolchain go1.22.0\n";
		let deps = parse(src).unwrap();
		assert_eq!(deps.len(), 1);
		assert_eq!(deps[0].name, "foo");
	}

	#[test]
	fn parse_strips_inline_comments_outside_indirect_marker() {
		let src = "module foo // some comment\n\nrequire github.com/x v1.0.0 // some other text\n";
		let deps = parse(src).unwrap();
		let req = deps.iter().find(|d| d.name == "github.com/x").unwrap();
		assert_eq!(req.dep_kind, "normal");
	}
}
