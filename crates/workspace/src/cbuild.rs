use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

use code_moniker_core::lang::c::Presets;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct HeaderUsage {
	c: bool,
	cpp: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum TranslationUnitLanguage {
	C,
	Cpp,
}

/// Build facts that affect the meaning of C-family source files. A `.h` suffix
/// is not a language declaration: until C++ extraction exists, headers reached
/// exclusively from C++ translation units must not be parsed as C.
#[derive(Clone, Debug, Default)]
pub struct CBuildContext {
	root: PathBuf,
	header_usage: HashMap<PathBuf, HeaderUsage>,
	include_paths: Vec<PathBuf>,
	workspace_files: Arc<BTreeSet<String>>,
	external_include_package: Option<String>,
	has_c_translation_unit: bool,
	has_cpp_translation_unit: bool,
}

impl CBuildContext {
	pub fn load(root: &Path) -> Self {
		let root = absolute_normalized(root);
		let entries = ignore::WalkBuilder::new(&root)
			.build()
			.filter_map(Result::ok)
			.filter(|entry| entry.file_type().is_some_and(|kind| kind.is_file()))
			.map(|entry| entry.into_path())
			.collect::<Vec<_>>();
		let workspace_files = entries
			.iter()
			.filter_map(|path| project_relative_path(&root, path))
			.collect::<BTreeSet<_>>();
		let makefile = load_makefile_hints(&root);
		let mut context = Self {
			include_paths: makefile.include_paths,
			root: root.clone(),
			header_usage: HashMap::new(),
			workspace_files: Arc::new(workspace_files),
			external_include_package: makefile.external_include_package,
			has_c_translation_unit: false,
			has_cpp_translation_unit: false,
		};
		if !context.include_paths.contains(&root) {
			context.include_paths.push(root.clone());
		}
		let mut visited_c = HashSet::new();
		let mut visited_cpp = HashSet::new();
		for path in entries {
			let Some(language) = translation_unit_language(&path) else {
				continue;
			};
			match language {
				TranslationUnitLanguage::C => context.has_c_translation_unit = true,
				TranslationUnitLanguage::Cpp => context.has_cpp_translation_unit = true,
			}
			let visited = match language {
				TranslationUnitLanguage::C => &mut visited_c,
				TranslationUnitLanguage::Cpp => &mut visited_cpp,
			};
			context.record_translation_unit(&path, language, visited);
		}
		context
	}

	pub fn extraction_presets(&self) -> Presets {
		Presets {
			include_paths: self
				.include_paths
				.iter()
				.filter_map(|path| project_relative_path(&self.root, path))
				.collect(),
			workspace_files: Arc::clone(&self.workspace_files),
			external_include_package: self.external_include_package.clone(),
		}
	}

	pub fn should_index_as_c(&self, path: &Path) -> bool {
		if !has_extension(path, "h") {
			return true;
		}
		if self.has_cpp_translation_unit && !self.has_c_translation_unit {
			return false;
		}
		let path = absolute_normalized(path);
		!self
			.header_usage
			.get(&path)
			.is_some_and(|usage| usage.cpp && !usage.c)
	}

	fn record_translation_unit(
		&mut self,
		path: &Path,
		language: TranslationUnitLanguage,
		visited: &mut HashSet<PathBuf>,
	) {
		self.record_includes(path, language, visited);
	}

	fn record_includes(
		&mut self,
		path: &Path,
		language: TranslationUnitLanguage,
		visited: &mut HashSet<PathBuf>,
	) {
		let path = absolute_normalized(path);
		if !visited.insert(path.clone()) {
			return;
		}
		let Ok(source) = std::fs::read(&path) else {
			return;
		};
		let source = String::from_utf8_lossy(&source);
		for include in includes(&source) {
			let Some(header) = self.resolve_include(&path, include) else {
				continue;
			};
			let usage = self.header_usage.entry(header.clone()).or_default();
			match language {
				TranslationUnitLanguage::C => usage.c = true,
				TranslationUnitLanguage::Cpp => usage.cpp = true,
			}
			self.record_includes(&header, language, visited);
		}
	}

	fn resolve_include(&self, source: &Path, include: IncludeDirective<'_>) -> Option<PathBuf> {
		if include.quoted {
			let relative = source.parent().unwrap_or(&self.root).join(include.path);
			if relative.is_file() {
				return Some(absolute_normalized(&relative));
			}
		}
		self.include_paths
			.iter()
			.map(|base| base.join(include.path))
			.find(|candidate| candidate.is_file())
			.map(|candidate| absolute_normalized(&candidate))
	}
}

fn translation_unit_language(path: &Path) -> Option<TranslationUnitLanguage> {
	let extension = path.extension()?.to_str()?.to_ascii_lowercase();
	match extension.as_str() {
		"c" => Some(TranslationUnitLanguage::C),
		"cc" | "cpp" | "cxx" | "c++" => Some(TranslationUnitLanguage::Cpp),
		_ => None,
	}
}

fn has_extension(path: &Path, expected: &str) -> bool {
	path.extension()
		.and_then(|extension| extension.to_str())
		.is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

#[derive(Clone, Copy)]
struct IncludeDirective<'a> {
	path: &'a str,
	quoted: bool,
}

fn includes(source: &str) -> impl Iterator<Item = IncludeDirective<'_>> {
	source.lines().filter_map(|line| {
		let directive = line.trim_start().strip_prefix('#')?.trim_start();
		let rest = directive.strip_prefix("include")?;
		if rest
			.chars()
			.next()
			.is_some_and(|character| character.is_alphanumeric() || character == '_')
		{
			return None;
		}
		let rest = rest.trim_start();
		if let Some(quoted) = rest.strip_prefix('"') {
			let end = quoted.find('"')?;
			return Some(IncludeDirective {
				path: &quoted[..end],
				quoted: true,
			});
		}
		let system = rest.strip_prefix('<')?;
		let end = system.find('>')?;
		Some(IncludeDirective {
			path: &system[..end],
			quoted: false,
		})
	})
}

#[derive(Default)]
struct MakefileHints {
	include_paths: Vec<PathBuf>,
	external_include_package: Option<String>,
}

fn load_makefile_hints(root: &Path) -> MakefileHints {
	let Ok(bytes) = std::fs::read(root.join("Makefile")) else {
		return MakefileHints::default();
	};
	let text = String::from_utf8_lossy(&bytes).replace("\\\n", " ");
	let mut paths = Vec::new();
	for line in text.lines() {
		let Some((_, value)) = line.split_once('=') else {
			continue;
		};
		let mut tokens = value.split_whitespace().peekable();
		while let Some(token) = tokens.next() {
			let raw = if token == "-I" {
				tokens.next()
			} else {
				token.strip_prefix("-I")
			};
			let Some(raw) = raw else {
				continue;
			};
			let raw = raw.trim_matches(['\'', '"']);
			if raw.is_empty() || raw.contains('$') || raw.contains('`') || raw.starts_with('-') {
				continue;
			}
			let path = absolute_normalized(&root.join(raw));
			if path.is_dir() && !paths.contains(&path) {
				paths.push(path);
			}
		}
	}
	let external_include_package = (text.contains("PGXS")
		&& (text.contains("PG_CONFIG") || text.contains("pg_config")))
	.then(|| "postgresql".to_string());
	MakefileHints {
		include_paths: paths,
		external_include_package,
	}
}

fn absolute_normalized(path: &Path) -> PathBuf {
	let absolute = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.unwrap_or_else(|_| PathBuf::from("."))
			.join(path)
	};
	let mut normalized = PathBuf::new();
	for component in absolute.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				normalized.pop();
			}
			other => normalized.push(other.as_os_str()),
		}
	}
	normalized
}

fn project_relative_path(root: &Path, path: &Path) -> Option<String> {
	let relative = absolute_normalized(path)
		.strip_prefix(root)
		.ok()?
		.to_path_buf();
	Some(
		relative
			.components()
			.filter_map(|component| component.as_os_str().to_str())
			.collect::<Vec<_>>()
			.join("/"),
	)
}

#[cfg(test)]
mod tests {
	use super::*;
	use std::fs;

	fn write(root: &Path, relative: &str, body: &str) {
		let path = root.join(relative);
		if let Some(parent) = path.parent() {
			fs::create_dir_all(parent).unwrap();
		}
		fs::write(path, body).unwrap();
	}

	#[test]
	fn cpp_only_header_is_not_indexed_as_c() {
		let temp = tempfile::tempdir().unwrap();
		write(
			temp.path(),
			"generated/model.pb.cc",
			"#include \"model.pb.h\"\n",
		);
		write(
			temp.path(),
			"generated/model.pb.h",
			"namespace generated {}\n",
		);

		let context = CBuildContext::load(temp.path());

		assert!(!context.should_index_as_c(&temp.path().join("generated/model.pb.h")));
	}

	#[test]
	fn header_shared_with_c_translation_unit_remains_c_indexable() {
		let temp = tempfile::tempdir().unwrap();
		write(temp.path(), "main.c", "#include \"shared.h\"\n");
		write(temp.path(), "main.cpp", "#include \"shared.h\"\n");
		write(temp.path(), "shared.h", "int shared(void);\n");

		let context = CBuildContext::load(temp.path());

		assert!(context.should_index_as_c(&temp.path().join("shared.h")));
	}

	#[test]
	fn cpp_only_project_does_not_treat_orphan_headers_as_c() {
		let temp = tempfile::tempdir().unwrap();
		write(temp.path(), "main.cpp", "int main() { return 0; }\n");
		write(temp.path(), "orphan.h", "class Orphan {};\n");

		let context = CBuildContext::load(temp.path());

		assert!(!context.should_index_as_c(&temp.path().join("orphan.h")));
	}

	#[test]
	fn transitive_cpp_headers_are_not_indexed_as_c() {
		let temp = tempfile::tempdir().unwrap();
		write(temp.path(), "main.cpp", "#include \"first.hpp\"\n");
		write(temp.path(), "first.hpp", "#include \"second.h\"\n");
		write(temp.path(), "second.h", "class Second {};\n");

		let context = CBuildContext::load(temp.path());

		assert!(!context.should_index_as_c(&temp.path().join("second.h")));
	}

	#[test]
	fn makefile_include_path_resolves_cpp_header_provenance() {
		let temp = tempfile::tempdir().unwrap();
		write(temp.path(), "Makefile", "CXXFLAGS += -I./include\n");
		write(
			temp.path(),
			"src/model.cpp",
			"#include \"generated/model.h\"\n",
		);
		write(
			temp.path(),
			"include/generated/model.h",
			"class Model {};\n",
		);

		let context = CBuildContext::load(temp.path());

		assert!(!context.should_index_as_c(&temp.path().join("include/generated/model.h")));
	}

	#[test]
	fn angle_include_through_makefile_path_marks_header_as_c() {
		let temp = tempfile::tempdir().unwrap();
		write(temp.path(), "Makefile", "CPPFLAGS += -I./include\n");
		write(temp.path(), "main.cpp", "#include \"shared.h\"\n");
		write(temp.path(), "main.c", "#include <shared.h>\n");
		write(temp.path(), "include/shared.h", "int shared(void);\n");

		let context = CBuildContext::load(temp.path());

		assert!(context.should_index_as_c(&temp.path().join("include/shared.h")));
	}

	#[test]
	fn pgxs_makefile_declares_postgresql_header_provenance() {
		let temp = tempfile::tempdir().unwrap();
		write(
			temp.path(),
			"Makefile",
			"USE_PGXS = 1\nPG_CONFIG = pg_config\nPGXS := $(shell $(PG_CONFIG) --pgxs)\ninclude $(PGXS)\n",
		);

		let context = CBuildContext::load(temp.path());

		assert_eq!(
			context
				.extraction_presets()
				.external_include_package
				.as_deref(),
			Some("postgresql")
		);
	}
}
