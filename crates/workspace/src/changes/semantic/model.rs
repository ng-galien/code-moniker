use std::path::PathBuf;

use code_moniker_core::core::moniker::Moniker;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SemanticKind {
	Added,
	Removed,
	BodyModified,
	SignatureChanged,
	Renamed,
	Moved,
	AttributeChanged,
}

impl SemanticKind {
	pub fn label(self) -> &'static str {
		match self {
			Self::Added => "added",
			Self::Removed => "removed",
			Self::BodyModified => "body-modified",
			Self::SignatureChanged => "signature-changed",
			Self::Renamed => "renamed",
			Self::Moved => "moved",
			Self::AttributeChanged => "attribute-changed",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Confidence {
	Certain,
	Candidate,
}

impl Confidence {
	pub fn label(self) -> &'static str {
		match self {
			Self::Certain => "certain",
			Self::Candidate => "candidate",
		}
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ChangeFacets {
	pub body_changed: bool,
	pub signature_changed: bool,
	pub visibility_changed: bool,
	pub header_changed: bool,
	pub file_moved: bool,
}

impl ChangeFacets {
	pub fn any(self) -> bool {
		self.body_changed
			|| self.signature_changed
			|| self.visibility_changed
			|| self.header_changed
			|| self.file_moved
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolSide {
	pub moniker: Moniker,
	pub file_path: PathBuf,
	pub kind: String,
	pub name: String,
	pub visibility: String,
	pub signature: String,
	pub line_range: Option<(u32, u32)>,
	pub body_hash: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolChange {
	pub kind: SemanticKind,
	pub confidence: Confidence,
	pub facets: ChangeFacets,
	pub old: Option<SymbolSide>,
	pub new: Option<SymbolSide>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RefChangeKind {
	ImportRetargeted,
	CallSiteRetargeted,
	Added,
	Removed,
}

impl RefChangeKind {
	pub fn label(self) -> &'static str {
		match self {
			Self::ImportRetargeted => "import-retargeted",
			Self::CallSiteRetargeted => "call-site-retargeted",
			Self::Added => "ref-added",
			Self::Removed => "ref-removed",
		}
	}

	pub fn is_retarget(self) -> bool {
		matches!(self, Self::ImportRetargeted | Self::CallSiteRetargeted)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefChange {
	pub kind: RefChangeKind,
	pub file_path: PathBuf,
	pub ref_kind: String,
	pub old_target: Option<Moniker>,
	pub new_target: Option<Moniker>,
	pub old_line_range: Option<(u32, u32)>,
	pub new_line_range: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct HunkCoverage {
	pub old_residual: Vec<(u32, u32)>,
	pub new_residual: Vec<(u32, u32)>,
}

impl HunkCoverage {
	pub fn explained(&self) -> bool {
		self.old_residual.is_empty() && self.new_residual.is_empty()
	}
}
