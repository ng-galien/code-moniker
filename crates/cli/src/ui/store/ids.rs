use std::fmt;
use std::path::Path;

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

	pub(in crate::ui) fn def(compact_moniker: &str) -> Self {
		Self(format!("def:{compact_moniker}"))
	}

	pub(in crate::ui) fn change(compact_moniker: &str) -> Self {
		Self(format!("change:{compact_moniker}"))
	}
}

impl fmt::Display for NodeId {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		self.0.fmt(f)
	}
}
