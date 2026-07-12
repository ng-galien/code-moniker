use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::kinds;
use rustc_hash::FxHashSet;

use crate::linkage::catalog::LinkageQuery;
use crate::source::CodeIndexMaterial;

// Manifest-free external classification: a target whose package chain has no
// workspace definition cannot be a workspace symbol, whatever the build
// system. Exact chains only — real corpora define types inside third-party
// packages (test mocks in org.apache.bookkeeper.client), a prefix rule lies.
pub(in crate::linkage) struct WorkspacePackageIndex {
	packages: FxHashSet<Vec<u8>>,
}

impl WorkspacePackageIndex {
	pub(in crate::linkage) fn build(material: &CodeIndexMaterial) -> Self {
		let mut packages = FxHashSet::default();
		for file in material.files.iter() {
			for def_idx in 0..file.graph.def_count() {
				if let Some(key) = package_key(&file.graph.def_at(def_idx).moniker) {
					packages.insert(key);
				}
			}
		}
		Self { packages }
	}

	pub(in crate::linkage) fn is_foreign(&self, query: &LinkageQuery<'_>) -> bool {
		self.is_foreign_moniker(query.target)
	}

	pub(in crate::linkage) fn is_foreign_moniker(&self, target: &Moniker) -> bool {
		match package_key(target) {
			Some(key) => !self.packages.contains(&key),
			None => false,
		}
	}
}

fn package_key(moniker: &Moniker) -> Option<Vec<u8>> {
	let mut key = Vec::new();
	let mut lang: &[u8] = b"";
	let mut has_package = false;
	for segment in moniker.as_view().segments() {
		if segment.kind == kinds::LANG {
			lang = segment.name;
		} else if segment.kind == kinds::PACKAGE {
			key.push(0);
			key.extend_from_slice(segment.name);
			has_package = true;
		} else if segment.kind == kinds::EXTERNAL_PKG {
			return None;
		}
	}
	if !has_package {
		return None;
	}
	let mut full = lang.to_vec();
	full.extend_from_slice(&key);
	Some(full)
}

#[cfg(test)]
mod tests {
	use code_moniker_core::core::moniker::MonikerBuilder;

	use super::*;

	fn moniker(segments: &[(&[u8], &[u8])]) -> Moniker {
		let mut builder = MonikerBuilder::new();
		builder.project(b".");
		for (kind, name) in segments {
			builder.segment(kind, name);
		}
		builder.build()
	}

	#[test]
	fn package_key_ignores_srcset_and_trailing_shape() {
		let main = moniker(&[
			(b"srcset", b"main"),
			(kinds::LANG, b"java"),
			(kinds::PACKAGE, b"com"),
			(kinds::PACKAGE, b"acme"),
			(b"module", b"Widget"),
			(b"class", b"Widget"),
		]);
		let test = moniker(&[
			(b"srcset", b"test"),
			(kinds::LANG, b"java"),
			(kinds::PACKAGE, b"com"),
			(kinds::PACKAGE, b"acme"),
			(b"module", b"Other"),
			(b"method", b"run()"),
		]);
		assert_eq!(package_key(&main), package_key(&test));
	}

	#[test]
	fn package_key_distinguishes_langs_and_chains() {
		let java = moniker(&[(kinds::LANG, b"java"), (kinds::PACKAGE, b"acme")]);
		let python = moniker(&[(kinds::LANG, b"python"), (kinds::PACKAGE, b"acme")]);
		let nested = moniker(&[
			(kinds::LANG, b"java"),
			(kinds::PACKAGE, b"acme"),
			(kinds::PACKAGE, b"util"),
		]);
		assert_ne!(package_key(&java), package_key(&python));
		assert_ne!(package_key(&java), package_key(&nested));
	}

	#[test]
	fn package_key_absent_without_packages_or_on_external() {
		let bare = moniker(&[(kinds::LANG, b"java"), (b"module", b"Widget")]);
		let external = moniker(&[
			(kinds::EXTERNAL_PKG, b"zustand"),
			(kinds::PACKAGE, b"store"),
		]);
		assert_eq!(package_key(&bare), None);
		assert_eq!(package_key(&external), None);
	}
}
