//! Per-language extractor contract.
//!
//! Every supported language exposes a zero-sized `Lang` type implementing
//! `LangExtractor`. The trait carries no dispatch overhead — it is the
//! formal contract every extractor must satisfy.
//!
//! `assert_conformance::<Lang>(graph, anchor)` validates that a graph
//! produced by an extractor respects the contract. Each extractor's
//! `#[cfg(test)] extract_default` helper invokes it on every fixture.
//!
//! The trait also exposes `declare` / `to_spec` default impls that delegate
//! to `declare::*` after validating the spec/graph carries this
//! language's `LANG_TAG`. The dynamic-dispatch SQL entry points
//! (`code_graph_declare`, `code_graph_to_spec`) keep using the free
//! functions in `declare`; the typed methods on the trait give Rust
//! callers a compile-time-typed handle to the same lifecycle.

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;
use crate::declare::{DeclareError, SerializeError, declare_from_json_value, graph_to_spec};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct KindSpec {
	pub id: &'static str,
	pub shape: Shape,
	pub order: u16,
	pub label: &'static str,
}

impl KindSpec {
	pub const fn new(id: &'static str, shape: Shape, order: u16, label: &'static str) -> Self {
		Self {
			id,
			shape,
			order,
			label,
		}
	}
}

pub trait LangExtractor {
	type Presets: Default;

	const LANG_TAG: &'static str;

	const ALLOWED_KINDS: &'static [&'static str];

	const KIND_SPECS: &'static [KindSpec];

	const ALLOWED_VISIBILITIES: &'static [&'static str];

	fn extract(
		uri: &str,
		source: &str,
		anchor: &Moniker,
		deep: bool,
		presets: &Self::Presets,
	) -> CodeGraph;

	fn declare(spec: &serde_json::Value) -> Result<CodeGraph, DeclareError> {
		check_spec_lang::<Self>(spec)?;
		declare_from_json_value(spec)
	}

	fn to_spec(graph: &CodeGraph) -> Result<serde_json::Value, SerializeError> {
		check_graph_lang::<Self>(graph)?;
		graph_to_spec(graph)
	}
}

fn check_spec_lang<E: LangExtractor + ?Sized>(
	spec: &serde_json::Value,
) -> Result<(), DeclareError> {
	let actual = spec
		.get("lang")
		.and_then(|v| v.as_str())
		.ok_or(DeclareError::MissingField {
			path: "$".to_string(),
			field: "lang",
		})?;
	if actual != E::LANG_TAG {
		return Err(DeclareError::LangMismatch {
			expected: E::LANG_TAG,
			actual: actual.to_string(),
		});
	}
	Ok(())
}

fn check_graph_lang<E: LangExtractor + ?Sized>(graph: &CodeGraph) -> Result<(), SerializeError> {
	let root = graph.root();
	let view = root.as_view();
	let lang_bytes = view
		.lang_segment()
		.ok_or_else(|| SerializeError::RootHasNoLangSegment {
			root: format!("{root:?}"),
		})?;
	let lang_str = std::str::from_utf8(lang_bytes).map_err(|_| SerializeError::Utf8 {
		what: "lang segment",
	})?;
	if lang_str != E::LANG_TAG {
		return Err(SerializeError::LangMismatch {
			expected: E::LANG_TAG,
			actual: lang_str.to_string(),
		});
	}
	Ok(())
}

mod conformance {
	use super::LangExtractor;
	use crate::core::code_graph::{CodeGraph, assert_local_refs_closed};
	use crate::core::kinds::{
		BIND_IMPORT, BIND_INJECT, BIND_LOCAL, BIND_NONE, KIND_COMMENT, KIND_LOCAL, KIND_MODULE,
		KIND_PARAM, ORIGIN_EXTRACTED, REF_ANNOTATES, REF_CALLS, REF_DI_REGISTER, REF_DI_REQUIRE,
		REF_EXTENDS, REF_IMPLEMENTS, REF_IMPORTS_MODULE, REF_IMPORTS_SYMBOL, REF_INSTANTIATES,
		REF_METHOD_CALL, REF_READS, REF_REEXPORTS, REF_USES_TYPE, VIS_NONE,
	};
	use crate::core::moniker::Moniker;

	const INTERNAL_KINDS: &[&[u8]] = &[KIND_MODULE, KIND_LOCAL, KIND_PARAM, KIND_COMMENT];

	pub fn assert_conformance<E: LangExtractor>(graph: &CodeGraph, anchor: &Moniker) {
		assert_root_under_anchor::<E>(graph, anchor);
		for d in graph.defs() {
			assert_kind_in_profile::<E>(d.moniker.as_bytes(), &d.kind);
			assert_visibility_in_profile::<E>(d.moniker.as_bytes(), &d.visibility);
			assert_kind_matches_moniker_last_segment(&d.moniker, &d.kind);
			assert_origin_extracted(&d.moniker, &d.origin);
		}
		for r in graph.refs() {
			assert_ref_binding_consistent(&r.kind, &r.binding);
		}
		assert_local_refs_closed(graph);
	}

	fn assert_root_under_anchor<E: LangExtractor>(graph: &CodeGraph, anchor: &Moniker) {
		let root = graph.root();
		let root_view = root.as_view();
		assert!(
			anchor.as_view().is_ancestor_of(&root_view) || root.as_bytes() == anchor.as_bytes(),
			"contract violation: root {root:?} is not anchored under {anchor:?}"
		);
		let lang = root_view.lang_segment().unwrap_or_else(|| {
			panic!(
				"contract violation: root {:?} has no `lang:` segment (lang={:?} expected)",
				root,
				E::LANG_TAG
			)
		});
		assert_eq!(
			lang,
			E::LANG_TAG.as_bytes(),
			"contract violation: root carries lang:{} but extractor LANG_TAG={}",
			String::from_utf8_lossy(lang),
			E::LANG_TAG
		);
	}

	fn assert_kind_in_profile<E: LangExtractor>(moniker_bytes: &[u8], kind: &[u8]) {
		if INTERNAL_KINDS.contains(&kind) {
			return;
		}
		let kind_str = std::str::from_utf8(kind).unwrap_or_else(|_| {
			panic!("contract violation: def kind is not UTF-8 ({kind:?})");
		});
		assert!(
			E::ALLOWED_KINDS.contains(&kind_str),
			"contract violation: def kind `{}` is not in {} profile (moniker bytes: {:?})",
			kind_str,
			E::LANG_TAG,
			moniker_bytes
		);
	}

	fn assert_visibility_in_profile<E: LangExtractor>(moniker_bytes: &[u8], vis: &[u8]) {
		if vis == VIS_NONE {
			return;
		}
		let vis_str = std::str::from_utf8(vis).unwrap_or_else(|_| {
			panic!("contract violation: def visibility is not UTF-8 ({vis:?})");
		});
		assert!(
			E::ALLOWED_VISIBILITIES.contains(&vis_str),
			"contract violation: def visibility `{}` is not in {} profile (moniker bytes: {:?})",
			vis_str,
			E::LANG_TAG,
			moniker_bytes
		);
	}

	fn assert_kind_matches_moniker_last_segment(moniker: &Moniker, kind: &[u8]) {
		if INTERNAL_KINDS.contains(&kind) {
			return;
		}
		let last_kind = moniker.last_kind().unwrap_or_else(|| {
			panic!("contract violation: def has no segments (kind={kind:?})");
		});
		assert_eq!(
			last_kind.as_slice(),
			kind,
			"contract violation: def.kind {kind:?} does not match moniker last segment kind {last_kind:?}"
		);
	}

	fn assert_origin_extracted(moniker: &Moniker, origin: &[u8]) {
		assert_eq!(
			origin, ORIGIN_EXTRACTED,
			"contract violation: extractor produced def with origin={origin:?} (must be `extracted`); moniker={moniker:?}"
		);
	}

	fn assert_ref_binding_consistent(kind: &[u8], binding: &[u8]) {
		let expected: &[u8] =
			if kind == REF_IMPORTS_SYMBOL || kind == REF_IMPORTS_MODULE || kind == REF_REEXPORTS {
				BIND_IMPORT
			} else if kind == REF_DI_REGISTER || kind == REF_DI_REQUIRE {
				BIND_INJECT
			} else if kind == REF_CALLS
				|| kind == REF_METHOD_CALL
				|| kind == REF_READS
				|| kind == REF_USES_TYPE
				|| kind == REF_INSTANTIATES
				|| kind == REF_EXTENDS
				|| kind == REF_IMPLEMENTS
				|| kind == REF_ANNOTATES
			{
				BIND_LOCAL
			} else {
				BIND_NONE
			};
		assert_eq!(
			binding,
			expected,
			"contract violation: ref kind={:?} got binding={:?} (expected {:?})",
			std::str::from_utf8(kind).unwrap_or("<non-utf8>"),
			std::str::from_utf8(binding).unwrap_or("<non-utf8>"),
			std::str::from_utf8(expected).unwrap_or("<non-utf8>"),
		);
	}
}

#[doc(hidden)]
pub use conformance::assert_conformance;

#[cfg(test)]
mod typed_lifecycle_tests {
	use super::*;
	use crate::declare::{DeclareError, declare_from_json_value};
	use serde_json::json;

	#[test]
	fn typed_declare_rejects_lang_mismatch() {
		let spec = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:foo",
			"lang": "rs",
			"symbols": []
		});
		let err = <crate::lang::ts::Lang as LangExtractor>::declare(&spec).unwrap_err();
		assert!(matches!(
			err,
			DeclareError::LangMismatch { expected: "ts", .. }
		));
	}

	#[test]
	fn typed_declare_accepts_matching_lang() {
		let spec = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:foo",
			"lang": "rs",
			"symbols": []
		});
		assert!(<crate::lang::rs::Lang as LangExtractor>::declare(&spec).is_ok());
	}

	#[test]
	fn typed_to_spec_rejects_lang_mismatch() {
		let spec = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:foo",
			"lang": "rs",
			"symbols": []
		});
		let g = declare_from_json_value(&spec).unwrap();
		let err = <crate::lang::ts::Lang as LangExtractor>::to_spec(&g).unwrap_err();
		assert!(matches!(
			err,
			SerializeError::LangMismatch { expected: "ts", .. }
		));
	}

	#[test]
	fn typed_to_spec_accepts_matching_lang() {
		let spec = json!({
			"root": "code+moniker://app/srcset:main/lang:rs/module:foo",
			"lang": "rs",
			"symbols": []
		});
		let g = declare_from_json_value(&spec).unwrap();
		assert!(<crate::lang::rs::Lang as LangExtractor>::to_spec(&g).is_ok());
	}
}
