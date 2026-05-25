use std::collections::BTreeMap;

use super::super::model::{ChangeId, ChangeStatus, ReferenceId, SourceId, SymbolId};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceSummary {
	pub id: SourceId,
	pub display_name: String,
	pub language: Option<String>,
	pub change_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolSummary {
	pub id: SymbolId,
	pub source: SourceId,
	pub identity: String,
	pub name: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
	pub child_count: usize,
	pub change: Option<ChangeStatus>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolDetail {
	pub symbol: SymbolSummary,
	pub children: Vec<SymbolSummary>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReferenceDirection {
	Incoming,
	Outgoing,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolReferences {
	pub symbol: SymbolSummary,
	pub incoming: ReferenceSet,
	pub outgoing: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceSet {
	pub summary: ReferenceSetSummary,
	pub groups: Vec<ReferenceSummary>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceSetSummary {
	pub refs: usize,
	pub files: usize,
	pub contexts: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceSummary {
	pub reference: ReferenceId,
	pub source: Option<SourceId>,
	pub context: Option<SymbolId>,
	pub actor: String,
	pub endpoint_label: &'static str,
	pub endpoint: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SearchHit {
	pub symbol: SymbolId,
	pub score: u32,
	pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeSummary {
	pub id: ChangeId,
	pub status: ChangeStatus,
	pub source: Option<SourceId>,
	pub symbol: Option<SymbolId>,
	pub identity: String,
	pub name: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
	pub hunk_count: usize,
	pub usage_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeDetail {
	pub summary: ChangeSummary,
	pub blast_radius: ReferenceSet,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedLinkageReport {
	pub unresolved_refs: usize,
	pub sources: BTreeMap<SourceId, usize>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiagnosticsSummary {
	pub errors: usize,
	pub warnings: usize,
	pub total: usize,
}
