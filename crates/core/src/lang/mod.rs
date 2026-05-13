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

pub use extractor::LangExtractor;
#[cfg(test)]
pub use extractor::assert_conformance;

/// Single dispatch table for every supported language.
///
/// Adding a language is a one-line change here. Each row produces:
/// - a variant on `Lang`
/// - participation in `Lang::from_tag` / `Lang::tag` / `Lang::allowed_kinds`
///   / `Lang::allowed_visibilities` (all consult the trait — no per-language
///   constant ever lives outside its `LangExtractor` impl)
/// - dispatch in the conformance test that scans `docs/postgres/declare-schema.json`
///
/// Forgetting to update one of those callsites is impossible: if a row is
/// missing, the build fails. If the row is present, every dispatch sees it.
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
				mut f: impl FnMut(&'static str, &'static [&'static str], &'static [&'static str]),
			) {
				$(
					$(#[$attr])*
					f(
						<$module as LangExtractor>::LANG_TAG,
						<$module as LangExtractor>::ALLOWED_KINDS,
						<$module as LangExtractor>::ALLOWED_VISIBILITIES,
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
		for_each_language(|tag, kinds, visibilities| {
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
		for_each_language(|tag, kinds, _| {
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
