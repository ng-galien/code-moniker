//! Process-wide kind registry shared by every pgrx wrapper. Each PG
//! backend has its own; canonical kinds are pre-interned in fixed order
//! so ids stay stable across processes.

use std::sync::Mutex;

use pgrx::prelude::*;

use crate::core::kind_registry::{KindId, KindRegistry, PunctClass};
use crate::core::uri::UriConfig;

pub(super) const DEFAULT_CONFIG: UriConfig<'static> = UriConfig {
	scheme: "esac://",
	path: "path",
	type_: "type",
	term: "term",
	method: "method",
};

static REGISTRY: Mutex<Option<KindRegistry>> = Mutex::new(None);

pub(super) fn with_registry<R>(f: impl FnOnce(&mut KindRegistry) -> R) -> R {
	let mut guard = REGISTRY.lock().unwrap();
	let reg = guard.get_or_insert_with(|| {
		let mut r = KindRegistry::new();
		r.intern("path", PunctClass::Path).unwrap();
		r.intern("type", PunctClass::Type).unwrap();
		r.intern("term", PunctClass::Term).unwrap();
		r.intern("method", PunctClass::Method).unwrap();
		r
	});
	f(reg)
}

pub(super) fn intern_kind(name: &str, punct: PunctClass) -> KindId {
	with_registry(|reg| reg.intern(name, punct))
		.unwrap_or_else(|| error!("kind registry exhausted (u16 ceiling)"))
}

/// Default punct class for a kind name. Drives URI ponctuation when a
/// caller passes a kind label without an explicit class. Built-in
/// vocabulary covers the common languages; unknown names default to
/// `Path`.
pub(super) fn punct_for_kind(name: &str) -> PunctClass {
	match name {
		"class" | "interface" | "enum" | "type" | "type_alias" | "struct" | "trait" => {
			PunctClass::Type
		}
		"field" | "variable" | "const" | "constant" | "property" | "term" => PunctClass::Term,
		"function" | "method" | "constructor" | "ctor" | "operator" => PunctClass::Method,
		_ => PunctClass::Path,
	}
}

pub(super) fn kind_name(id: KindId) -> String {
	with_registry(|reg| {
		reg.name(id)
			.map(str::to_string)
			.unwrap_or_else(|| format!("kind#{}", id.as_u16()))
	})
}
