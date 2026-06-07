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
use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;
use crate::core::shape::Shape;

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
}

mod conformance {
	use super::LangExtractor;
	use crate::core::code_graph::{CodeGraph, assert_local_refs_closed};
	use crate::core::kinds::{
		BIND_IMPORT, BIND_INJECT, BIND_LOCAL, BIND_NONE, KIND_COMMENT, KIND_LOCAL, KIND_MODULE,
		KIND_PARAM, ORIGIN_EXTRACTED, REF_ANNOTATES, REF_CALLS, REF_DI_REGISTER, REF_DI_REQUIRE,
		REF_EXTENDS, REF_IMPLEMENTS, REF_IMPORTS_MODULE, REF_IMPORTS_SYMBOL, REF_INSTANTIATES,
		REF_METHOD_CALL, REF_READS, REF_REEXPORTS, REF_RETURNS_TYPE, REF_USES_TYPE, VIS_NONE,
	};
	use crate::core::moniker::Moniker;

	const INTERNAL_KINDS: &[&[u8]] = &[KIND_MODULE, KIND_LOCAL, KIND_PARAM, KIND_COMMENT];

	pub fn assert_conformance<E: LangExtractor>(graph: &CodeGraph, anchor: &Moniker) {
		assert_root_under_anchor::<E>(graph, anchor);
		for d in graph.defs() {
			assert_kind_in_profile::<E>(d.moniker.as_encoded(), &d.kind);
			assert_visibility_in_profile::<E>(d.moniker.as_encoded(), &d.visibility);
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
			anchor.as_view().is_ancestor_of(&root_view) || root.as_encoded() == anchor.as_encoded(),
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
				|| kind == REF_RETURNS_TYPE
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
