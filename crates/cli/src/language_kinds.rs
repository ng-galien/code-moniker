use std::collections::BTreeSet;

use code_moniker_core::core::kinds::{
	KIND_COMMENT, KIND_LOCAL, KIND_MODULE, KIND_PARAM, REF_ANNOTATES, REF_CALLS, REF_DI_REGISTER,
	REF_DI_REQUIRE, REF_EXTENDS, REF_IMPLEMENTS, REF_IMPORTS_MODULE, REF_IMPORTS_SYMBOL,
	REF_INSTANTIATES, REF_METHOD_CALL, REF_READS, REF_REEXPORTS, REF_USES_TYPE,
};
use code_moniker_core::lang::Lang;

pub const CROSS_LANG_KINDS: &[&[u8]] = &[
	KIND_MODULE,
	KIND_COMMENT,
	KIND_LOCAL,
	KIND_PARAM,
	REF_IMPORTS_SYMBOL,
	REF_IMPORTS_MODULE,
	REF_REEXPORTS,
	REF_DI_REGISTER,
	REF_DI_REQUIRE,
	REF_CALLS,
	REF_METHOD_CALL,
	REF_READS,
	REF_USES_TYPE,
	REF_INSTANTIATES,
	REF_EXTENDS,
	REF_IMPLEMENTS,
	REF_ANNOTATES,
];

pub fn known_kinds<'a>(langs: impl IntoIterator<Item = &'a Lang>) -> BTreeSet<&'static str> {
	let mut out: BTreeSet<&'static str> = BTreeSet::new();
	for k in CROSS_LANG_KINDS {
		out.insert(
			std::str::from_utf8(k)
				.unwrap_or_else(|err| panic!("kind constants must be ASCII: {err}")),
		);
	}
	for lang in langs {
		for k in lang.allowed_kinds() {
			out.insert(*k);
		}
	}
	out
}

pub fn unknown_kinds(kinds: &[String], known: &BTreeSet<&'static str>) -> Vec<String> {
	kinds
		.iter()
		.filter(|k| !known.contains(k.as_str()))
		.cloned()
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn known_kinds_for_ts_includes_class_function_and_ref_kinds() {
		let k = known_kinds(std::iter::once(&Lang::Ts));
		assert!(k.contains("class"));
		assert!(k.contains("function"));
		assert!(k.contains("method"));
		assert!(k.contains("calls"));
		assert!(k.contains("imports_module"));
		assert!(k.contains("module"));
		assert!(!k.contains("fn"), "fn is Rust-specific, not in ts vocab");
	}

	#[test]
	fn known_kinds_union_picks_up_per_lang_specifics() {
		let langs = [Lang::Ts, Lang::Rs];
		let k = known_kinds(langs.iter());
		assert!(k.contains("function"), "TS contributes `function`");
		assert!(k.contains("fn"), "Rust contributes `fn`");
	}

	#[test]
	fn unknown_kinds_flags_typos_and_lang_mismatches() {
		let langs = [Lang::Ts];
		let k = known_kinds(langs.iter());
		let unknown = unknown_kinds(
			&[
				"function".to_string(),
				"fn".to_string(),
				"clazz".to_string(),
			],
			&k,
		);
		assert_eq!(unknown, vec!["fn".to_string(), "clazz".to_string()]);
	}
}
