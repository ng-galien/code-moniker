use std::sync::Arc;
use std::time::Duration;

use super::records::RecordTable;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ResourceGeneration(u64);

impl ResourceGeneration {
	pub fn new(value: u64) -> Self {
		Self(value)
	}

	pub fn value(self) -> u64 {
		self.0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceRequest {
	pub label: String,
	pub catalog: CatalogRequest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CatalogRequest {
	Refresh,
	ReuseCurrent,
}

impl WorkspaceRequest {
	pub fn new(label: impl Into<String>) -> Self {
		Self {
			label: label.into(),
			catalog: CatalogRequest::Refresh,
		}
	}

	pub fn reuse_current_catalog(mut self) -> Self {
		self.catalog = CatalogRequest::ReuseCurrent;
		self
	}

	pub fn should_reuse_current_catalog(&self) -> bool {
		self.catalog == CatalogRequest::ReuseCurrent
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceId {
	file: u32,
}

impl SourceId {
	pub fn at(file: usize) -> Self {
		Self { file: file as u32 }
	}

	pub fn parse(value: &str) -> Option<Self> {
		let rest = value.strip_prefix("source:")?;
		let file = rest.split(':').next()?;
		Some(Self {
			file: file.parse().ok()?,
		})
	}

	pub fn file(self) -> usize {
		self.file as usize
	}
}

impl std::fmt::Display for SourceId {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "source:{}", self.file)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnit {
	pub id: SourceId,
	pub display_name: String,
	pub language: Option<String>,
}

impl SourceUnit {
	pub fn new(id: SourceId, display_name: impl Into<String>) -> Self {
		Self {
			id,
			display_name: display_name.into(),
			language: None,
		}
	}

	pub fn with_language(
		id: SourceId,
		display_name: impl Into<String>,
		language: impl Into<String>,
	) -> Self {
		Self {
			id,
			display_name: display_name.into(),
			language: Some(language.into()),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceCatalog {
	pub generation: ResourceGeneration,
	pub sources: Vec<SourceUnit>,
}

impl SourceCatalog {
	pub fn new(generation: ResourceGeneration, mut sources: Vec<SourceUnit>) -> Self {
		sources.shrink_to_fit();
		Self {
			generation,
			sources,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SymbolId {
	file: u32,
	def: u32,
}

impl SymbolId {
	pub fn at(file: usize, def: usize) -> Self {
		Self {
			file: file as u32,
			def: def as u32,
		}
	}

	pub fn parse(value: &str) -> Option<Self> {
		let rest = value.strip_prefix("symbol:")?;
		let (file, def) = rest.split_once(':')?;
		Some(Self {
			file: file.parse().ok()?,
			def: def.parse().ok()?,
		})
	}

	pub fn file(self) -> usize {
		self.file as usize
	}

	pub fn def(self) -> usize {
		self.def as usize
	}
}

impl std::fmt::Display for SymbolId {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "symbol:{}:{}", self.file, self.def)
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct SymbolLocation {
	pub file: usize,
	pub symbol: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolRecord {
	pub id: SymbolId,
	pub source: SourceId,
	pub identity: Arc<str>,
	pub name: String,
	pub kind: String,
	pub visibility: String,
	pub signature: String,
	pub navigable: bool,
	pub line_range: Option<(u32, u32)>,
	pub parent: Option<SymbolId>,
}

impl SymbolRecord {
	pub fn new(
		id: SymbolId,
		source: SourceId,
		name: impl Into<String>,
		kind: impl Into<String>,
	) -> Self {
		Self {
			identity: Arc::from(id.to_string()),
			id,
			source,
			name: name.into(),
			kind: kind.into(),
			visibility: String::new(),
			signature: String::new(),
			navigable: true,
			line_range: None,
			parent: None,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ReferenceId {
	file: u32,
	reference: u32,
}

impl ReferenceId {
	pub fn at(file: usize, reference: usize) -> Self {
		Self {
			file: file as u32,
			reference: reference as u32,
		}
	}

	pub fn parse(value: &str) -> Option<Self> {
		let rest = value.strip_prefix("reference:")?;
		let (file, reference) = rest.split_once(':')?;
		Some(Self {
			file: file.parse().ok()?,
			reference: reference.parse().ok()?,
		})
	}

	pub fn file(self) -> usize {
		self.file as usize
	}

	pub fn reference(self) -> usize {
		self.reference as usize
	}
}

impl std::fmt::Display for ReferenceId {
	fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(formatter, "reference:{}:{}", self.file, self.reference)
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReferenceRecord {
	pub id: ReferenceId,
	pub source: SourceId,
	pub source_symbol: SymbolId,
	pub target_identity: Arc<str>,
	pub kind: String,
	pub call_name: Option<String>,
	pub call_arity: Option<usize>,
	pub confidence: Option<String>,
	pub receiver: Option<String>,
	pub alias: Option<String>,
	pub line_range: Option<(u32, u32)>,
}

impl ReferenceRecord {
	pub fn new(
		id: ReferenceId,
		source: SourceId,
		source_symbol: SymbolId,
		target_identity: impl Into<Arc<str>>,
		kind: impl Into<String>,
		line_range: Option<(u32, u32)>,
	) -> Self {
		Self {
			id,
			source,
			source_symbol,
			target_identity: target_identity.into(),
			kind: kind.into(),
			call_name: None,
			call_arity: None,
			confidence: None,
			receiver: None,
			alias: None,
			line_range,
		}
	}

	pub fn with_call_metadata(
		mut self,
		call_name: Option<String>,
		call_arity: Option<usize>,
	) -> Self {
		self.call_name = call_name;
		self.call_arity = call_arity;
		self
	}

	pub fn with_metadata(
		mut self,
		confidence: Option<String>,
		receiver: Option<String>,
		alias: Option<String>,
	) -> Self {
		self.confidence = confidence;
		self.receiver = receiver;
		self.alias = alias;
		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceFileRecord {
	pub id: SourceId,
	pub uri: String,
	pub source_root: usize,
	pub path: String,
	pub rel_path: String,
	pub anchor: String,
	pub language: String,
	pub text: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeIndex {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub identity_scheme: String,
	pub sources: Vec<SourceFileRecord>,
	pub symbols: RecordTable<SymbolRecord>,
	pub references: RecordTable<ReferenceRecord>,
	pub timings: CodeIndexTimings,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CodeIndexTimings {
	pub extract_sources: Duration,
	pub semantic_index: Duration,
	pub total: Duration,
}

impl CodeIndex {
	pub fn new(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		symbols: Vec<SymbolRecord>,
	) -> Self {
		Self::with_references(generation, catalog_generation, symbols, Vec::new())
	}

	pub fn with_references(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		symbols: Vec<SymbolRecord>,
		references: Vec<ReferenceRecord>,
	) -> Self {
		Self {
			generation,
			catalog_generation,
			identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
			sources: Vec::new(),
			symbols: RecordTable::from_records(symbols),
			references: RecordTable::from_records(references),
			timings: CodeIndexTimings::default(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkageEdge {
	pub reference: ReferenceId,
	pub target: SymbolId,
}

impl LinkageEdge {
	pub fn new(reference: ReferenceId, target: SymbolId) -> Self {
		Self { reference, target }
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalReferenceOrigin {
	Dependency,
	Injected,
	UnknownExternal,
}

impl ExternalReferenceOrigin {
	pub fn label(self) -> &'static str {
		match self {
			Self::Dependency => "dependency",
			Self::Injected => "injected",
			Self::UnknownExternal => "unknown_external",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ExternalReference {
	pub reference: ReferenceId,
	pub target_identity: Arc<str>,
	pub origin: ExternalReferenceOrigin,
}

impl ExternalReference {
	pub fn new(
		reference: ReferenceId,
		target_identity: impl Into<Arc<str>>,
		origin: ExternalReferenceOrigin,
	) -> Self {
		Self {
			reference,
			target_identity: target_identity.into(),
			origin,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedReference {
	pub reference: ReferenceId,
	pub target_identity: Arc<str>,
}

impl UnresolvedReference {
	pub fn new(reference: ReferenceId, target_identity: impl Into<Arc<str>>) -> Self {
		Self {
			reference,
			target_identity: target_identity.into(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkageSnapshot {
	pub generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub resolved_refs: usize,
	pub external_refs: usize,
	pub manifest_blocked_refs: usize,
	pub unresolved_refs: usize,
	pub ambiguous_refs: usize,
	pub resolved: Vec<LinkageEdge>,
	pub external: Vec<ExternalReference>,
	pub manifest_blocked: Vec<UnresolvedReference>,
	pub unresolved: Vec<UnresolvedReference>,
	pub read_index: LinkageReadIndexHandle,
}

#[derive(Debug)]
pub struct LinkageReadIndex {
	pub(crate) incoming: rustc_hash::FxHashMap<SymbolId, Vec<ReferenceId>>,
	pub(crate) targets: rustc_hash::FxHashMap<ReferenceId, SymbolId>,
}

impl LinkageReadIndex {
	pub fn from_edges(edges: &[LinkageEdge]) -> Self {
		let mut incoming = rustc_hash::FxHashMap::<SymbolId, Vec<ReferenceId>>::default();
		let mut targets = rustc_hash::FxHashMap::<ReferenceId, SymbolId>::default();
		for edge in edges {
			let LinkageEdge { reference, target } = edge.clone();
			targets.entry(reference).or_insert(target);
			incoming.entry(target).or_default().push(reference);
		}
		Self { incoming, targets }
	}

	pub fn incoming(&self, symbol: &SymbolId) -> &[ReferenceId] {
		self.incoming.get(symbol).map(Vec::as_slice).unwrap_or(&[])
	}

	pub fn resolved_target(&self, reference: &ReferenceId) -> Option<&SymbolId> {
		self.targets.get(reference)
	}
}

#[derive(Clone, Debug, Default)]
pub struct LinkageReadIndexHandle(Option<Arc<LinkageReadIndex>>);

impl LinkageReadIndexHandle {
	pub fn from_edges(edges: &[LinkageEdge]) -> Self {
		Self(Some(Arc::new(LinkageReadIndex::from_edges(edges))))
	}

	pub fn get(&self) -> Option<&LinkageReadIndex> {
		self.0.as_deref()
	}
}

impl PartialEq for LinkageReadIndexHandle {
	fn eq(&self, _other: &Self) -> bool {
		true
	}
}

impl Eq for LinkageReadIndexHandle {}

impl LinkageSnapshot {
	pub fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		resolved_refs: usize,
		unresolved_refs: usize,
	) -> Self {
		Self {
			generation,
			index_generation,
			resolved_refs,
			external_refs: 0,
			manifest_blocked_refs: 0,
			unresolved_refs,
			ambiguous_refs: 0,
			resolved: Vec::new(),
			external: Vec::new(),
			manifest_blocked: Vec::new(),
			unresolved: Vec::new(),
			read_index: LinkageReadIndexHandle::default(),
		}
	}

	pub fn with_refs(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		mut resolved: Vec<LinkageEdge>,
		mut unresolved: Vec<UnresolvedReference>,
	) -> Self {
		resolved.shrink_to_fit();
		unresolved.shrink_to_fit();
		let read_index = LinkageReadIndexHandle::from_edges(&resolved);
		Self {
			generation,
			index_generation,
			resolved_refs: resolved.len(),
			external_refs: 0,
			manifest_blocked_refs: 0,
			unresolved_refs: unresolved.len(),
			ambiguous_refs: 0,
			resolved,
			external: Vec::new(),
			manifest_blocked: Vec::new(),
			unresolved,
			read_index,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeOverlay {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub scope: String,
	pub resources: Vec<ChangeResource>,
	pub diagnostics: Vec<String>,
	pub changed_symbols: Vec<SymbolId>,
	pub changes: Vec<ChangeRecord>,
	pub semantic: Option<std::sync::Arc<crate::changes::semantic::review::SemanticReview>>,
}

pub struct ChangeOverlayReport {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub scope: String,
	pub resources: Vec<ChangeResource>,
	pub diagnostics: Vec<String>,
	pub changes: Vec<ChangeRecord>,
}

impl ChangeOverlay {
	pub fn new(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		mut changed_symbols: Vec<SymbolId>,
	) -> Self {
		changed_symbols.shrink_to_fit();
		Self {
			generation,
			catalog_generation,
			index_generation,
			scope: "HEAD..worktree".to_string(),
			resources: Vec::new(),
			diagnostics: Vec::new(),
			changed_symbols,
			changes: Vec::new(),
			semantic: None,
		}
	}

	pub fn with_records(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		mut changes: Vec<ChangeRecord>,
	) -> Self {
		changes.shrink_to_fit();
		let changed_symbols = changes.iter().filter_map(|change| change.symbol).fold(
			Vec::new(),
			|mut out, symbol| {
				if !out.contains(&symbol) {
					out.push(symbol);
				}
				out
			},
		);
		let mut changed_symbols = changed_symbols;
		changed_symbols.shrink_to_fit();
		Self {
			generation,
			catalog_generation,
			index_generation,
			scope: "HEAD..worktree".to_string(),
			resources: Vec::new(),
			diagnostics: Vec::new(),
			changed_symbols,
			changes,
			semantic: None,
		}
	}

	pub fn from_report(report: ChangeOverlayReport) -> Self {
		let mut resources = report.resources;
		let mut diagnostics = report.diagnostics;
		resources.shrink_to_fit();
		diagnostics.shrink_to_fit();
		let mut overlay = Self::with_records(
			report.generation,
			report.catalog_generation,
			report.index_generation,
			report.changes,
		);
		overlay.scope = report.scope;
		overlay.resources = resources;
		overlay.diagnostics = diagnostics;
		overlay
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeResource {
	pub available: bool,
	pub label: String,
	pub message: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ChangeStatus {
	Added,
	Modified,
	Removed,
}

impl ChangeStatus {
	pub fn label(self) -> &'static str {
		match self {
			Self::Added => "added",
			Self::Modified => "modified",
			Self::Removed => "removed",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ChangeId(String);

impl ChangeId {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRecord {
	pub id: ChangeId,
	pub status: ChangeStatus,
	pub source: Option<SourceId>,
	pub source_uri: Option<String>,
	pub symbol: Option<SymbolId>,
	pub identity: String,
	pub language: String,
	pub file_path: String,
	pub name: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
	pub hunk_count: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ChangeRecordCoreFields {
	pub id: ChangeId,
	pub status: ChangeStatus,
	pub identity: String,
	pub language: String,
	pub file_path: String,
	pub name: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
	pub hunk_count: usize,
}

impl ChangeRecord {
	pub fn new(fields: ChangeRecordCoreFields) -> Self {
		Self {
			id: fields.id,
			status: fields.status,
			source: None,
			source_uri: None,
			symbol: None,
			identity: fields.identity,
			language: fields.language,
			file_path: fields.file_path,
			name: fields.name,
			kind: fields.kind,
			line_range: fields.line_range,
			hunk_count: fields.hunk_count,
		}
	}

	pub fn with_source(mut self, source: SourceId, source_uri: impl Into<String>) -> Self {
		self.source = Some(source);
		self.source_uri = Some(source_uri.into());
		self
	}

	pub fn with_symbol(mut self, symbol: SymbolId) -> Self {
		self.symbol = Some(symbol);
		self
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
	pub generation: ResourceGeneration,
	pub catalog: SourceCatalog,
	pub index: CodeIndex,
	pub linkage: LinkageSnapshot,
	pub changes: ChangeOverlay,
	pub timings: WorkspaceTimings,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct WorkspaceTimings {
	pub source_catalog: Duration,
	pub extract_sources: Duration,
	pub semantic_index: Duration,
	pub code_index: Duration,
	pub linkage: Duration,
	pub change_overlay: Duration,
	pub total: Duration,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceResource {
	SourceCatalog,
	CodeIndex,
	LinkageSnapshot,
	ChangeOverlay,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceFailure {
	pub resource: WorkspaceResource,
	pub message: String,
}

impl WorkspaceFailure {
	pub fn new(resource: WorkspaceResource, message: impl Into<String>) -> Self {
		Self {
			resource,
			message: message.into(),
		}
	}
}

pub type WorkspaceResult<T> = Result<T, WorkspaceFailure>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum WorkspaceTransition {
	Ready {
		generation: ResourceGeneration,
	},
	Failed {
		failure: WorkspaceFailure,
		preserved_generation: Option<ResourceGeneration>,
	},
}
