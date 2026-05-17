use std::fmt;
use std::path::Path;

use code_moniker_core::core::moniker::Moniker;

use super::compact_moniker;

macro_rules! domain_id {
	($name:ident) => {
		#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
		pub(in crate::ui) struct $name(String);

		impl $name {
			#[allow(dead_code)]
			pub(in crate::ui) fn new(value: impl Into<String>) -> Self {
				Self(value.into())
			}
		}

		impl fmt::Display for $name {
			fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
				self.0.fmt(f)
			}
		}
	};
}

domain_id!(SourceRootId);
domain_id!(FileId);
domain_id!(SymbolId);
domain_id!(RefId);
domain_id!(CoverageRunId);

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub(in crate::ui) struct NodeId(String);

impl NodeId {
	pub(in crate::ui) fn root(scope: &str) -> Self {
		Self(format!("root:{scope}"))
	}

	pub(in crate::ui) fn lang(scope: &str, lang: &str) -> Self {
		Self(format!("{scope}:lang:{lang}"))
	}

	pub(in crate::ui) fn dir(scope: &str, lang: &str, path: &str) -> Self {
		Self(format!("{scope}:dir:{lang}:{path}"))
	}

	pub(in crate::ui) fn file(anchor: &Path) -> Self {
		Self(format!("file:{}", anchor.display()))
	}

	pub(in crate::ui) fn change_file(path: &Path) -> Self {
		Self(format!("change-file:{}", path.display()))
	}

	pub(in crate::ui) fn def(moniker: &Moniker) -> Self {
		Self(format!("def:{}", compact_moniker(moniker)))
	}

	pub(in crate::ui) fn change(moniker: &Moniker) -> Self {
		Self(format!("change:{}", compact_moniker(moniker)))
	}
}

impl fmt::Display for NodeId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}
