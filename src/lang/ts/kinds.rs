//! TypeScript-specific kind interning.

use crate::core::kind_registry::{KindId, KindRegistry, PunctClass};

#[derive(Copy, Clone)]
pub(super) struct TsKinds {
	// Canonical structural kinds — used in moniker bytes so URI
	// roundtrip and `=` equality stay punct-class-stable.
	pub(super) path: KindId,
	pub(super) type_canon: KindId,
	pub(super) method_canon: KindId,

	// Semantic labels — used as `DefRecord.kind` / `RefRecord.kind`
	// metadata and surfaced as text in the SQL API.
	pub(super) class_label: KindId,
	pub(super) function_label: KindId,
	pub(super) import_label: KindId,
}

impl TsKinds {
	pub(super) fn new(reg: &mut KindRegistry) -> Self {
		Self {
			path: reg.intern("path", PunctClass::Path).unwrap(),
			type_canon: reg.intern("type", PunctClass::Type).unwrap(),
			method_canon: reg.intern("method", PunctClass::Method).unwrap(),
			class_label: reg.intern("class", PunctClass::Type).unwrap(),
			function_label: reg.intern("function", PunctClass::Method).unwrap(),
			import_label: reg.intern("import", PunctClass::Path).unwrap(),
		}
	}
}
