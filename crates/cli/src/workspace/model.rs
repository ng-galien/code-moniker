use std::path::PathBuf;

use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use super::git::ChangeStatus;
use super::index::{DefLocation, RefLocation};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct FileSummary {
	pub(crate) index: usize,
	pub(crate) lang: Lang,
	pub(crate) rel_path: PathBuf,
	pub(crate) anchor: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SymbolSummary {
	pub(crate) id: DefLocation,
	pub(crate) lang: Lang,
	pub(crate) kind: String,
	pub(crate) name: String,
	pub(crate) file_path: PathBuf,
	pub(crate) compact_moniker: String,
	pub(crate) line_range: Option<(u32, u32)>,
	pub(crate) child_count: usize,
	pub(crate) change: Option<ChangeBadge>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SymbolDetail {
	pub(crate) symbol: SymbolSummary,
	pub(crate) children: Vec<SymbolSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeBadge {
	pub(crate) status: ChangeStatus,
	pub(crate) usage_count: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub(crate) struct ChangeId(usize);

impl ChangeId {
	pub(in crate::workspace) fn new(index: usize) -> Self {
		Self(index)
	}

	pub(in crate::workspace) fn index(self) -> usize {
		self.0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeSummary {
	pub(crate) id: ChangeId,
	pub(crate) status: ChangeStatus,
	pub(crate) lang: Lang,
	pub(crate) kind: String,
	pub(crate) name: String,
	pub(crate) file_path: PathBuf,
	pub(crate) compact_moniker: String,
	pub(crate) line_range: Option<(u32, u32)>,
	pub(crate) hunk_count: usize,
	pub(crate) usage_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeDetail {
	pub(crate) summary: ChangeSummary,
	pub(crate) blast_radius: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ChangeOverview {
	pub(crate) scope: String,
	pub(crate) change_count: usize,
	pub(crate) file_count: usize,
	pub(crate) resources: Vec<GitResourceSummary>,
	pub(crate) diagnostics: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct GitResourceSummary {
	pub(crate) available: bool,
	pub(crate) label: String,
	pub(crate) message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ReferenceDirection {
	Incoming,
	Outgoing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceSet {
	pub(crate) summary: ReferenceSetSummary,
	pub(crate) groups: Vec<ReferenceGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceSetSummary {
	pub(crate) refs: usize,
	pub(crate) files: usize,
	pub(crate) contexts: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ReferenceGroup {
	pub(crate) kinds: Vec<String>,
	pub(crate) actor: String,
	pub(crate) location: String,
	pub(crate) endpoint_label: &'static str,
	pub(crate) endpoint: String,
	pub(crate) confidence: String,
	pub(crate) receiver: Option<String>,
	pub(crate) alias: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SymbolReferences {
	pub(crate) symbol: SymbolSummary,
	pub(crate) incoming: ReferenceSet,
	pub(crate) outgoing: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct UsageFocus {
	pub(crate) target: Moniker,
	pub(crate) label: String,
	pub(crate) compact_moniker: String,
	pub(crate) refs: Vec<RefLocation>,
	pub(crate) contexts: Vec<DefLocation>,
	pub(crate) references: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SearchHit {
	pub(crate) loc: DefLocation,
	pub(crate) score: u32,
	pub(crate) reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceLine {
	pub(crate) number: u32,
	pub(crate) text: String,
	pub(crate) active: bool,
}
