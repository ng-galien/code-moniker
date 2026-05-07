//! Kind registry — interns short identifiers (`"class"`, `"method"`,
//! `"package"`, …) to a stable [`KindId`] (`u16`), and pairs each
//! interned kind with a [`PunctClass`] that drives SCIP-style URI
//! serialisation.
//!
//! The registry is **process-local**. Each PostgreSQL backend gets its
//! own; ids are not portable across instances. The bytea encoding of a
//! moniker carries kind ids and is therefore process-local too — that
//! is acceptable for the MVP and is documented as a known limitation.
//! When persisted ids are needed (cross-backend, cross-restart), a
//! future revision will encode kind names directly in the bytea or
//! back the registry with a PG table.

use std::collections::HashMap;
use std::sync::Arc;

/// Punctuation class for a kind. Drives SCIP-style URI serialisation:
///
/// | Class    | URI shape                | Used for                                        |
/// |----------|--------------------------|-------------------------------------------------|
/// | `Path`   | `…/<name>`               | srcset, package, directory, file-as-module      |
/// | `Type`   | `…#<name>#`              | class, interface, enum, type alias              |
/// | `Term`   | `…#<name>.`              | field, variable, constant                       |
/// | `Method` | `…#<name>().` / `(N).`   | method, function, constructor                   |
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum PunctClass {
	Path,
	Type,
	Term,
	Method,
}

/// Interned identifier for a kind. Stable for the lifetime of the
/// owning [`KindRegistry`] instance.
///
/// `KindId(0)` is reserved as [`KindId::INVALID`]. Real interns start
/// at id 1. The [`Default`] impl returns `INVALID`.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct KindId(u16);

impl KindId {
	/// Sentinel id meaning "not interned" / "invalid".
	pub const INVALID: KindId = KindId(0);

	pub fn as_u16(self) -> u16 {
		self.0
	}

	/// Wrap a raw u16. Only meaningful when the value comes from
	/// previously-interned data (e.g. decoding a moniker buffer).
	pub fn from_raw(v: u16) -> Self {
		Self(v)
	}

	pub fn is_valid(self) -> bool {
		self.0 != 0
	}
}

#[derive(Debug)]
struct KindEntry {
	name: Arc<str>,
	punct: PunctClass,
}

/// Registry of interned kinds.
#[derive(Debug, Default)]
pub struct KindRegistry {
	by_name: HashMap<Arc<str>, KindId>,
	entries: Vec<KindEntry>,
}

impl KindRegistry {
	pub fn new() -> Self {
		Self::default()
	}

	/// Intern a kind. Same name returns the same id across calls.
	///
	/// If `name` is already interned, the existing id is returned and
	/// the supplied `punct` is **ignored** — the first registration
	/// wins. This means "what is this kind?" is decided once, at first
	/// intern, and is stable thereafter.
	///
	/// Returns `None` if the registry has reached the u16 ceiling
	/// (65 535 distinct kinds).
	pub fn intern(&mut self, name: &str, punct: PunctClass) -> Option<KindId> {
		if let Some(&id) = self.by_name.get(name) {
			return Some(id);
		}
		if self.entries.len() >= u16::MAX as usize {
			return None;
		}
		let s: Arc<str> = Arc::from(name);
		let id = KindId((self.entries.len() as u16) + 1);
		self.entries.push(KindEntry {
			name: Arc::clone(&s),
			punct,
		});
		self.by_name.insert(s, id);
		Some(id)
	}

	/// Reverse lookup: id → name.
	pub fn name(&self, id: KindId) -> Option<&str> {
		if !id.is_valid() {
			return None;
		}
		self.entries
			.get((id.0 as usize) - 1)
			.map(|e| e.name.as_ref())
	}

	/// Reverse lookup: id → punctuation class.
	pub fn punct_class(&self, id: KindId) -> Option<PunctClass> {
		if !id.is_valid() {
			return None;
		}
		self.entries.get((id.0 as usize) - 1).map(|e| e.punct)
	}

	pub fn len(&self) -> usize {
		self.entries.len()
	}

	pub fn is_empty(&self) -> bool {
		self.entries.is_empty()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn create_empty() {
		let reg = KindRegistry::new();
		assert_eq!(reg.len(), 0);
		assert!(reg.is_empty());
	}

	#[test]
	fn intern_first() {
		let mut reg = KindRegistry::new();
		let id = reg.intern("class", PunctClass::Type).unwrap();
		assert!(id.is_valid());
		assert_eq!(id.as_u16(), 1);
	}

	#[test]
	fn intern_idempotent() {
		let mut reg = KindRegistry::new();
		let a = reg.intern("method", PunctClass::Method).unwrap();
		let b = reg.intern("method", PunctClass::Method).unwrap();
		assert_eq!(a, b);
		assert_eq!(reg.len(), 1);
	}

	#[test]
	fn intern_idempotent_ignores_second_punct() {
		// Second registration with a different punct class must be a no-op:
		// first wins.
		let mut reg = KindRegistry::new();
		let id = reg.intern("class", PunctClass::Type).unwrap();
		let again = reg.intern("class", PunctClass::Term).unwrap();
		assert_eq!(id, again);
		assert_eq!(reg.punct_class(id), Some(PunctClass::Type));
	}

	#[test]
	fn intern_distinct() {
		let mut reg = KindRegistry::new();
		let a = reg.intern("class", PunctClass::Type).unwrap();
		let b = reg.intern("method", PunctClass::Method).unwrap();
		let c = reg.intern("package", PunctClass::Path).unwrap();
		assert_ne!(a, b);
		assert_ne!(b, c);
		assert_ne!(a, c);
	}

	#[test]
	fn name_and_punct_lookup() {
		let mut reg = KindRegistry::new();
		let id = reg.intern("field", PunctClass::Term).unwrap();
		assert_eq!(reg.name(id), Some("field"));
		assert_eq!(reg.punct_class(id), Some(PunctClass::Term));
	}

	#[test]
	fn lookup_invalid() {
		let mut reg = KindRegistry::new();
		reg.intern("x", PunctClass::Type).unwrap();
		assert_eq!(reg.name(KindId::INVALID), None);
		assert_eq!(reg.punct_class(KindId::INVALID), None);
		assert_eq!(reg.name(KindId::from_raw(99)), None);
		assert_eq!(reg.punct_class(KindId::from_raw(99)), None);
	}

	#[test]
	fn name_after_mutating_input() {
		let mut reg = KindRegistry::new();
		let mut buf = String::from("method");
		let id = reg.intern(&buf, PunctClass::Method).unwrap();
		buf.clear();
		buf.push_str("xxxxxxxx");
		assert_eq!(reg.name(id), Some("method"));
	}

	#[test]
	fn many_distinct_kinds() {
		let mut reg = KindRegistry::new();
		let ids: Vec<KindId> = (0..200)
			.map(|i| reg.intern(&format!("k{i}"), PunctClass::Type).unwrap())
			.collect();
		assert_eq!(reg.len(), 200);
		for (i, id) in ids.iter().enumerate() {
			let again = reg.intern(&format!("k{i}"), PunctClass::Type).unwrap();
			assert_eq!(*id, again);
			assert_eq!(reg.name(*id), Some(format!("k{i}").as_str()));
		}
	}

	#[test]
	fn ids_are_sequential() {
		let mut reg = KindRegistry::new();
		for i in 0..50 {
			let id = reg.intern(&format!("k{i}"), PunctClass::Type).unwrap();
			assert_eq!(id.as_u16(), (i as u16) + 1);
		}
	}
}
