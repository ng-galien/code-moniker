use std::collections::BTreeSet;
use std::sync::{Arc, OnceLock};

use roaring::RoaringBitmap;

mod generated {
	include!("sdk_catalog_generated.rs");
}

const DEFAULT_LIBRARIES: &[&str] = &[
	"es2022",
	"dom",
	"dom.iterable",
	"dom.asynciterable",
	"scripthost",
];

#[derive(Clone, Debug)]
pub struct TsSdkProfile {
	active: Arc<RoaringBitmap>,
	libraries: Arc<[String]>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TsSdkMember {
	pub owner: &'static str,
	pub result: Option<&'static str>,
}

impl Default for TsSdkProfile {
	fn default() -> Self {
		static DEFAULT: OnceLock<TsSdkProfile> = OnceLock::new();
		DEFAULT
			.get_or_init(|| Self::from_libraries(DEFAULT_LIBRARIES.iter().copied()))
			.clone()
	}
}

impl TsSdkProfile {
	pub fn from_libraries(libraries: impl IntoIterator<Item = impl AsRef<str>>) -> Self {
		build_profile(libraries)
	}

	pub fn libraries(&self) -> &[String] {
		&self.libraries
	}

	pub fn catalog_typescript_version() -> &'static str {
		generated::CATALOG_TYPESCRIPT_VERSION
	}

	pub fn catalog_digest() -> &'static str {
		generated::CATALOG_DIGEST
	}

	pub fn is_global_type(&self, name: &[u8]) -> bool {
		find_type(name).is_some_and(|(_, ordinal)| self.active.contains(*ordinal))
			|| self.is_global_value(name)
	}

	pub fn is_global_value(&self, name: &[u8]) -> bool {
		value_range(name).any(|(_, _, ordinal)| self.active.contains(*ordinal))
	}

	pub fn global_value_owner(&self, name: &[u8]) -> Option<&'static str> {
		global_value_owner(self, name)
	}

	pub fn method(&self, owner: &str, name: &str) -> Option<TsSdkMember> {
		find_member(self, owner, name, true, 0, &mut Vec::new())
	}

	pub fn property(&self, owner: &str, name: &str) -> Option<TsSdkMember> {
		find_member(self, owner, name, false, 0, &mut Vec::new())
	}
}

fn build_profile(libraries: impl IntoIterator<Item = impl AsRef<str>>) -> TsSdkProfile {
	let mut requested = BTreeSet::new();
	for library in libraries {
		let normalized = normalize_library(library.as_ref());
		if !normalized.is_empty() {
			requested.insert(normalized);
		}
	}
	let mut expanded = BTreeSet::new();
	for library in requested {
		expand_library(&library, &mut expanded);
	}
	let mut active = RoaringBitmap::new();
	for library in &expanded {
		if let Ok(index) =
			generated::LIBRARIES.binary_search_by(|entry| entry.0.cmp(library.as_str()))
		{
			active.extend(generated::LIBRARIES[index].1.iter().copied());
		}
	}
	TsSdkProfile {
		active: Arc::new(active),
		libraries: expanded.into_iter().collect::<Vec<_>>().into(),
	}
}

fn global_value_owner(profile: &TsSdkProfile, name: &[u8]) -> Option<&'static str> {
	let mut fallback = None;
	for (value_name, owner, ordinal) in value_range(name) {
		if !profile.active.contains(*ordinal) {
			continue;
		}
		if !owner.is_empty() {
			return Some(owner);
		}
		fallback = Some(*value_name);
	}
	fallback.filter(|owner| profile.is_global_type(owner.as_bytes()))
}

fn find_member(
	profile: &TsSdkProfile,
	owner: &str,
	name: &str,
	method: bool,
	depth: usize,
	visited: &mut Vec<&'static str>,
) -> Option<TsSdkMember> {
	if depth > 16 || visited.contains(&owner) {
		return None;
	}
	let owner = active_type_name(profile, owner)?;
	visited.push(owner);
	let direct = direct_member(profile, owner, name, method);
	if direct.is_some() {
		visited.pop();
		return direct;
	}
	for (_, parent, ordinal) in parent_range(owner) {
		if !profile.active.contains(*ordinal) {
			continue;
		}
		if let Some(member) = find_member(profile, parent, name, method, depth + 1, visited) {
			visited.pop();
			return Some(member);
		}
	}
	visited.pop();
	None
}

fn direct_member(
	profile: &TsSdkProfile,
	owner: &'static str,
	name: &str,
	method: bool,
) -> Option<TsSdkMember> {
	let mut fallback = None;
	for (entry_owner, _, entry_method, result, ordinal) in member_range(owner, name) {
		if *entry_method != method || !profile.active.contains(*ordinal) {
			continue;
		}
		let candidate = TsSdkMember {
			owner: entry_owner,
			result: (!result.is_empty()).then_some(*result),
		};
		if candidate.result.is_some() {
			return Some(candidate);
		}
		fallback = Some(candidate);
	}
	fallback
}

fn normalize_library(library: &str) -> String {
	let mut normalized = library.trim().to_ascii_lowercase();
	if let Some(stripped) = normalized.strip_prefix("lib.") {
		normalized = stripped.to_owned();
	}
	if let Some(stripped) = normalized.strip_suffix(".d.ts") {
		normalized = stripped.to_owned();
	}
	normalized
}

fn expand_library(library: &str, expanded: &mut BTreeSet<String>) {
	if !expanded.insert(library.to_owned()) {
		return;
	}
	let Ok(index) = generated::LIBRARIES.binary_search_by(|entry| entry.0.cmp(library)) else {
		return;
	};
	for dependency in generated::LIBRARIES[index].2 {
		expand_library(dependency, expanded);
	}
}

fn find_type(name: &[u8]) -> Option<&'static (&'static str, u32)> {
	generated::TYPES
		.binary_search_by(|entry| entry.0.as_bytes().cmp(name))
		.ok()
		.map(|index| &generated::TYPES[index])
}

fn active_type_name(profile: &TsSdkProfile, name: &str) -> Option<&'static str> {
	find_type(name.as_bytes())
		.and_then(|(known, ordinal)| profile.active.contains(*ordinal).then_some(*known))
}

fn value_range(name: &[u8]) -> impl Iterator<Item = &'static (&'static str, &'static str, u32)> {
	let start = generated::VALUES.partition_point(|entry| entry.0.as_bytes() < name);
	let end = generated::VALUES.partition_point(|entry| entry.0.as_bytes() <= name);
	generated::VALUES[start..end].iter()
}

fn member_range(
	owner: &str,
	name: &str,
) -> impl Iterator<Item = &'static (&'static str, &'static str, bool, &'static str, u32)> {
	let key = (owner, name);
	let start = generated::MEMBERS.partition_point(|entry| (entry.0, entry.1) < key);
	let end = generated::MEMBERS.partition_point(|entry| (entry.0, entry.1) <= key);
	generated::MEMBERS[start..end].iter()
}

fn parent_range(owner: &str) -> impl Iterator<Item = &'static (&'static str, &'static str, u32)> {
	let start = generated::PARENTS.partition_point(|entry| entry.0 < owner);
	let end = generated::PARENTS.partition_point(|entry| entry.0 <= owner);
	generated::PARENTS[start..end].iter()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn dom_profile_owns_dom_globals_types_and_inherited_members() {
		let profile = TsSdkProfile::from_libraries(["ES2022", "DOM"]);

		assert!(profile.is_global_value(b"document"));
		assert!(profile.is_global_type(b"HTMLButtonElement"));
		assert_eq!(profile.global_value_owner(b"document"), Some("Document"));
		assert_eq!(
			profile.method("HTMLButtonElement", "replaceChildren"),
			Some(TsSdkMember {
				owner: "ParentNode",
				result: None,
			})
		);
		assert_eq!(
			profile.property("HTMLButtonElement", "classList"),
			Some(TsSdkMember {
				owner: "Element",
				result: Some("DOMTokenList"),
			})
		);
		assert_eq!(
			profile.method("DOMTokenList", "add"),
			Some(TsSdkMember {
				owner: "DOMTokenList",
				result: None,
			})
		);
	}

	#[test]
	fn node_only_es_profile_does_not_acquire_dom() {
		let profile = TsSdkProfile::from_libraries(["ES2022"]);

		assert!(profile.is_global_type(b"Promise"));
		assert!(!profile.is_global_value(b"document"));
		assert!(!profile.is_global_type(b"HTMLElement"));
		assert!(
			profile
				.method("HTMLButtonElement", "replaceChildren")
				.is_none()
		);
	}

	#[test]
	fn webworker_profile_is_distinct_from_dom() {
		let profile = TsSdkProfile::from_libraries(["ES2022", "WebWorker"]);

		assert!(profile.is_global_value(b"self"));
		assert!(profile.is_global_type(b"WorkerGlobalScope"));
		assert!(!profile.is_global_value(b"document"));
		assert!(!profile.is_global_type(b"HTMLButtonElement"));
	}

	#[test]
	fn namespace_declarations_expose_qualified_sdk_members() {
		let profile = TsSdkProfile::from_libraries(["ES2022"]);

		assert!(profile.is_global_value(b"Intl"));
		assert_eq!(
			profile.method("Intl", "DateTimeFormat"),
			Some(TsSdkMember {
				owner: "Intl",
				result: Some("Intl.DateTimeFormat"),
			}),
		);
		assert!(profile.is_global_type(b"Intl.DateTimeFormat"));
	}
}
