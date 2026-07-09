use std::path::Path;

use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
use code_moniker_core::core::uri::{UriConfig, to_uri};

use crate::snapshot::{ReferenceId, SourceId, SymbolId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalIdentityResolver {
	scheme: String,
}

impl LocalIdentityResolver {
	pub fn new(scheme: impl Into<String>) -> Self {
		Self {
			scheme: scheme.into(),
		}
	}

	pub fn scheme(&self) -> &str {
		&self.scheme
	}

	pub fn source_id(&self, file_idx: usize, rel_path: &Path) -> SourceId {
		SourceId::new(format!("source:{file_idx}:{}", rel_path.display()))
	}

	pub fn source_index(&self, id: &SourceId) -> Option<usize> {
		let mut parts = id.as_str().split(':');
		match (parts.next(), parts.next()) {
			(Some("source"), Some(file)) => file.parse().ok(),
			_ => None,
		}
	}

	pub fn source_uri(&self, rel_path: &Path) -> String {
		let moniker = MonikerBuilder::new()
			.project(b".")
			.segment(b"file", rel_path.display().to_string().as_bytes())
			.build();
		self.moniker_uri(&moniker)
	}

	pub fn symbol_id(&self, file_idx: usize, def_idx: usize) -> SymbolId {
		SymbolId::at(file_idx, def_idx)
	}

	pub fn symbol_location(&self, id: &SymbolId) -> Option<(usize, usize)> {
		Some((id.file(), id.def()))
	}

	pub fn reference_id(&self, file_idx: usize, ref_idx: usize) -> ReferenceId {
		ReferenceId::at(file_idx, ref_idx)
	}

	pub fn reference_location(&self, id: &ReferenceId) -> Option<(usize, usize)> {
		Some((id.file(), id.reference()))
	}

	pub fn moniker_uri(&self, moniker: &Moniker) -> String {
		to_uri(
			moniker,
			&UriConfig {
				scheme: &self.scheme,
			},
		)
	}
}

impl Default for LocalIdentityResolver {
	fn default() -> Self {
		Self::new(crate::DEFAULT_IDENTITY_SCHEME)
	}
}
