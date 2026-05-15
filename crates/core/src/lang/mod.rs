pub mod build_manifest;
pub mod callable;
pub mod canonical_walker;
pub mod cs;
pub mod extractor;
pub mod go;
pub mod java;
pub mod kinds;
pub mod python;
pub mod rs;
pub mod sql;
pub mod strategy;
pub mod tree_util;
pub mod ts;

#[doc(hidden)]
pub use extractor::assert_conformance;
pub use extractor::{KindSpec, LangExtractor};

/// Adding a row registers the language for `Lang::from_tag` / `tag` /
/// `allowed_kinds` / `allowed_visibilities` and the schema-sync test.
macro_rules! define_languages {
	($($(#[$attr:meta])* $variant:ident => $module:ty),* $(,)?) => {
		#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
		pub enum Lang {
			$(
				$(#[$attr])*
				$variant,
			)*
		}

		impl Lang {
			pub const ALL: &'static [Lang] = &[
				$(
					$(#[$attr])*
					Self::$variant,
				)*
			];

			pub fn from_tag(s: &str) -> Option<Self> {
				$(
					$(#[$attr])*
					if s == <$module as $crate::lang::LangExtractor>::LANG_TAG {
						return Some(Self::$variant);
					}
				)*
				None
			}

			pub fn tag(self) -> &'static str {
				match self {
					$(
						$(#[$attr])*
						Self::$variant => <$module as $crate::lang::LangExtractor>::LANG_TAG,
					)*
				}
			}

			pub fn allowed_kinds(self) -> &'static [&'static str] {
				match self {
					$(
						$(#[$attr])*
						Self::$variant => <$module as $crate::lang::LangExtractor>::ALLOWED_KINDS,
					)*
				}
			}

			pub fn kind_specs(self) -> &'static [$crate::lang::KindSpec] {
				match self {
					$(
						$(#[$attr])*
						Self::$variant => <$module as $crate::lang::LangExtractor>::KIND_SPECS,
					)*
				}
			}

			pub fn kind_spec(self, id: &str) -> Option<&'static $crate::lang::KindSpec> {
				self.kind_specs().iter().find(|spec| spec.id == id)
			}

			pub fn allowed_visibilities(self) -> &'static [&'static str] {
				match self {
					$(
						$(#[$attr])*
						Self::$variant => <$module as $crate::lang::LangExtractor>::ALLOWED_VISIBILITIES,
					)*
				}
			}

			pub fn ignores_visibility(self) -> bool {
				self.allowed_visibilities().is_empty()
			}
		}

		#[cfg(test)]
		mod _conformance_dispatch {
			use $crate::lang::LangExtractor;

			/// Dispatches a closure that takes `(lang_tag, allowed_kinds, allowed_visibilities)`
			/// over every registered language. Used by the JSON Schema sync test.
			pub(crate) fn for_each_language(
				mut f: impl FnMut(
					&'static str,
					&'static [&'static str],
					&'static [&'static str],
					&'static [$crate::lang::KindSpec],
				),
			) {
				$(
					$(#[$attr])*
					f(
						<$module as LangExtractor>::LANG_TAG,
						<$module as LangExtractor>::ALLOWED_KINDS,
						<$module as LangExtractor>::ALLOWED_VISIBILITIES,
						<$module as LangExtractor>::KIND_SPECS,
					);
				)*
			}
		}
	};
}

define_languages! {
	Ts     => crate::lang::ts::Lang,
	Rs     => crate::lang::rs::Lang,
	Java   => crate::lang::java::Lang,
	Python => crate::lang::python::Lang,
	Go     => crate::lang::go::Lang,
	Cs     => crate::lang::cs::Lang,
	Sql    => crate::lang::sql::Lang,
}

#[cfg(test)]
pub(crate) use _conformance_dispatch::for_each_language;

#[cfg(test)]
mod schema_sync_tests {
	use super::for_each_language;
	use serde_json::Value;

	const SCHEMA_JSON: &str = include_str!("../../../../docs/postgres/declare-schema.json");

	fn profile_name_for(tag: &str) -> String {
		let mut chars = tag.chars();
		let first = chars.next().unwrap().to_uppercase().collect::<String>();
		format!("{first}{}Profile", chars.as_str())
	}

	fn enum_at<'a>(schema: &'a Value, profile: &str, field: &str) -> Vec<&'a str> {
		schema
			.get("$defs")
			.and_then(|d| d.get(profile))
			.and_then(|p| p.get("properties"))
			.and_then(|p| p.get("symbols"))
			.and_then(|s| s.get("items"))
			.and_then(|i| i.get("properties"))
			.and_then(|p| p.get(field))
			.and_then(|f| f.get("enum"))
			.and_then(|e| e.as_array())
			.map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
			.unwrap_or_default()
	}

	#[test]
	fn declare_schema_matches_trait_constants() {
		let schema: Value = serde_json::from_str(SCHEMA_JSON)
			.expect("docs/postgres/declare-schema.json must be valid JSON");

		let mut visited = 0usize;
		for_each_language(|tag, kinds, visibilities, _| {
			visited += 1;
			let profile = profile_name_for(tag);

			let schema_kinds = enum_at(&schema, &profile, "kind");
			let trait_kinds: Vec<&str> = kinds.to_vec();
			assert_eq!(
				sort(&schema_kinds),
				sort(&trait_kinds),
				"declare-schema.json {profile}.kind enum drifted from `{tag}` trait ALLOWED_KINDS"
			);

			if visibilities.is_empty() {
				let schema_vis = enum_at(&schema, &profile, "visibility");
				assert!(
					schema_vis.is_empty(),
					"declare-schema.json {profile} declares visibilities but extractor profile is empty"
				);
			} else {
				let schema_vis = enum_at(&schema, &profile, "visibility");
				let trait_vis: Vec<&str> = visibilities.to_vec();
				assert_eq!(
					sort(&schema_vis),
					sort(&trait_vis),
					"declare-schema.json {profile}.visibility enum drifted from `{tag}` trait ALLOWED_VISIBILITIES"
				);
			}
		});

		assert_eq!(
			visited,
			super::Lang::ALL.len(),
			"for_each_language visited {visited} languages but Lang::ALL contains {}; the cfg gates of the dispatch table and the macro variants are out of sync",
			super::Lang::ALL.len()
		);
	}

	fn sort<'a>(xs: &[&'a str]) -> Vec<&'a str> {
		let mut v: Vec<&str> = xs.to_vec();
		v.sort_unstable();
		v
	}
}

#[cfg(test)]
mod shape_coverage_tests {
	use super::for_each_language;
	use crate::core::shape::shape_of;

	#[test]
	fn every_allowed_kind_has_a_shape() {
		let mut missing: Vec<(String, String)> = Vec::new();
		for_each_language(|tag, kinds, _, _| {
			for k in kinds {
				if shape_of(k.as_bytes()).is_none() {
					missing.push((tag.to_string(), (*k).to_string()));
				}
			}
		});
		assert!(
			missing.is_empty(),
			"kinds in ALLOWED_KINDS without an entry in core::shape::SHAPE_TABLE: {missing:?}"
		);
	}

	#[test]
	fn internal_kinds_have_a_shape() {
		for k in [b"module".as_slice(), b"comment", b"local", b"param"] {
			assert!(
				shape_of(k).is_some(),
				"internal kind {:?} must have a shape entry",
				std::str::from_utf8(k).unwrap()
			);
		}
	}
}

#[cfg(test)]
mod kind_contract_tests {
	use super::for_each_language;
	use crate::core::shape::shape_of;

	#[test]
	fn every_language_kind_spec_matches_allowed_kinds() {
		for_each_language(|tag, kinds, _, specs| {
			let spec_ids: Vec<_> = specs.iter().map(|spec| spec.id).collect();
			assert_eq!(
				sort(&spec_ids),
				sort(kinds),
				"{tag} KIND_SPECS must describe exactly ALLOWED_KINDS"
			);
		});
	}

	#[test]
	fn every_language_kind_spec_has_stable_semantics() {
		for_each_language(|tag, _, _, specs| {
			assert!(!specs.is_empty(), "{tag} must declare kind specs");
			let mut seen_ids = std::collections::HashSet::new();
			for spec in specs {
				assert!(
					seen_ids.insert(spec.id),
					"{tag} duplicates kind spec `{}`",
					spec.id
				);
				assert!(
					!spec.label.is_empty(),
					"{tag} kind `{}` has no label",
					spec.id
				);
				assert_ne!(spec.order, 0, "{tag} kind `{}` has no order", spec.id);
				assert_eq!(
					shape_of(spec.id.as_bytes()),
					Some(spec.shape),
					"{tag} kind `{}` shape must stay aligned with core shape taxonomy",
					spec.id
				);
			}
		});
	}

	fn sort<'a>(xs: &[&'a str]) -> Vec<&'a str> {
		let mut v: Vec<&str> = xs.to_vec();
		v.sort_unstable();
		v
	}
}

#[cfg(test)]
mod comment_collapse_tests {
	use crate::core::moniker::MonikerBuilder;

	struct Case {
		tag: &'static str,
		uri: &'static str,
		run: fn(&'static str) -> crate::core::code_graph::CodeGraph,
	}

	fn anchor() -> crate::core::moniker::Moniker {
		MonikerBuilder::new().project(b"app").build()
	}

	fn cases() -> Vec<Case> {
		vec![
			Case {
				tag: "rs",
				uri: "test.rs",
				run: |src| {
					super::rs::extract(
						"test.rs",
						src,
						&anchor(),
						false,
						&super::rs::Presets::default(),
					)
				},
			},
			Case {
				tag: "ts",
				uri: "test.ts",
				run: |src| {
					super::ts::extract(
						"test.ts",
						src,
						&anchor(),
						false,
						&super::ts::Presets::default(),
					)
				},
			},
			Case {
				tag: "python",
				uri: "test.py",
				run: |src| {
					super::python::extract(
						"test.py",
						src,
						&anchor(),
						false,
						&super::python::Presets::default(),
					)
				},
			},
			Case {
				tag: "go",
				uri: "test.go",
				run: |src| {
					super::go::extract(
						"test.go",
						src,
						&anchor(),
						false,
						&super::go::Presets::default(),
					)
				},
			},
			Case {
				tag: "java",
				uri: "test.java",
				run: |src| {
					super::java::extract(
						"test.java",
						src,
						&anchor(),
						false,
						&super::java::Presets::default(),
					)
				},
			},
			Case {
				tag: "cs",
				uri: "test.cs",
				run: |src| {
					super::cs::extract(
						"test.cs",
						src,
						&anchor(),
						false,
						&super::cs::Presets::default(),
					)
				},
			},
			Case {
				tag: "sql",
				uri: "test.sql",
				run: |src| {
					super::sql::extract(
						"test.sql",
						src,
						&anchor(),
						false,
						&super::sql::Presets::default(),
					)
				},
			},
		]
	}

	const ADJACENT: &[(&str, &str)] = &[
		("rs", "// a\n// b\n// c\nstruct Foo;\n"),
		("ts", "// a\n// b\n// c\nclass Foo {}"),
		("python", "# a\n# b\n# c\nclass Foo: pass\n"),
		("go", "package x\n// a\n// b\n// c\nfunc Foo() {}\n"),
		("java", "// a\n// b\n// c\nclass Foo {}\n"),
		("cs", "// a\n// b\n// c\nclass Foo {}\n"),
		(
			"sql",
			"-- a\n-- b\n-- c\nCREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;\n",
		),
	];

	const SPLIT_BY_BLANK: &[(&str, &str)] = &[
		("rs", "// a\n// b\n\n// c\nstruct Foo;\n"),
		("ts", "// a\n// b\n\n// c\nclass Foo {}"),
		("python", "# a\n# b\n\n# c\nclass Foo: pass\n"),
		("go", "package x\n// a\n// b\n\n// c\nfunc Foo() {}\n"),
		("java", "// a\n// b\n\n// c\nclass Foo {}\n"),
		("cs", "// a\n// b\n\n// c\nclass Foo {}\n"),
		(
			"sql",
			"-- a\n-- b\n\n-- c\nCREATE FUNCTION f() RETURNS int LANGUAGE sql AS $$ SELECT 1 $$;\n",
		),
	];

	fn count_comments(g: &crate::core::code_graph::CodeGraph) -> usize {
		g.defs().filter(|d| d.kind == b"comment").count()
	}

	#[test]
	fn each_language_collapses_three_adjacent_line_comments_into_one_def() {
		for case in cases() {
			let src = ADJACENT
				.iter()
				.find(|(tag, _)| *tag == case.tag)
				.expect("adjacent fixture")
				.1;
			let g = (case.run)(src);
			assert_eq!(
				count_comments(&g),
				1,
				"lang={} ({}): three adjacent line comments must collapse to one def",
				case.tag,
				case.uri
			);
		}
	}

	#[test]
	fn each_language_splits_runs_on_blank_line() {
		for case in cases() {
			let src = SPLIT_BY_BLANK
				.iter()
				.find(|(tag, _)| *tag == case.tag)
				.expect("blank-line fixture")
				.1;
			let g = (case.run)(src);
			assert_eq!(
				count_comments(&g),
				2,
				"lang={} ({}): blank line must break the run into two defs",
				case.tag,
				case.uri
			);
		}
	}
}
