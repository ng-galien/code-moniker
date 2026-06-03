use std::sync::Arc;
use std::time::Duration;

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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceId(String);

impl SourceId {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceUnit {
	pub id: SourceId,
	pub display_name: String,
	pub language: Option<String>,
}

impl SourceUnit {
	pub fn new(id: impl Into<String>, display_name: impl Into<String>) -> Self {
		Self {
			id: SourceId::new(id),
			display_name: display_name.into(),
			language: None,
		}
	}

	pub fn with_language(
		id: impl Into<String>,
		display_name: impl Into<String>,
		language: impl Into<String>,
	) -> Self {
		Self {
			id: SourceId::new(id),
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

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SymbolId(String);

impl SymbolId {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
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
	pub identity: String,
	pub name: String,
	pub kind: String,
	pub visibility: String,
	pub signature: String,
	pub navigable: bool,
	pub line_range: Option<(u32, u32)>,
	pub parent: Option<SymbolId>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolRecordFields {
	pub id: SymbolId,
	pub source: SourceId,
	pub identity: String,
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
		id: impl Into<String>,
		source: SourceId,
		name: impl Into<String>,
		kind: impl Into<String>,
	) -> Self {
		let id = SymbolId::new(id);
		Self {
			identity: id.as_str().to_string(),
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

	pub fn from_fields(fields: SymbolRecordFields) -> Self {
		Self {
			id: fields.id,
			source: fields.source,
			identity: fields.identity,
			name: fields.name,
			kind: fields.kind,
			visibility: fields.visibility,
			signature: fields.signature,
			navigable: fields.navigable,
			line_range: fields.line_range,
			parent: fields.parent,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct ReferenceId(String);

impl ReferenceId {
	pub fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}

	pub fn as_str(&self) -> &str {
		&self.0
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
		id: impl Into<String>,
		source: SourceId,
		source_symbol: SymbolId,
		target_identity: impl Into<Arc<str>>,
		kind: impl Into<String>,
		line_range: Option<(u32, u32)>,
	) -> Self {
		Self {
			id: ReferenceId::new(id),
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

pub struct SourceFileRecordFields {
	pub id: SourceId,
	pub uri: String,
	pub source_root: usize,
	pub path: String,
	pub rel_path: String,
	pub anchor: String,
	pub language: String,
	pub text: String,
}

impl SourceFileRecord {
	pub fn from_fields(fields: SourceFileRecordFields) -> Self {
		Self {
			id: fields.id,
			uri: fields.uri,
			source_root: fields.source_root,
			path: fields.path,
			rel_path: fields.rel_path,
			anchor: fields.anchor,
			language: fields.language,
			text: fields.text,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodeIndex {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub identity_scheme: String,
	pub sources: Vec<SourceFileRecord>,
	pub symbols: Vec<SymbolRecord>,
	pub references: Vec<ReferenceRecord>,
	pub timings: CodeIndexTimings,
}

pub struct CodeIndexFields {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub identity_scheme: String,
	pub sources: Vec<SourceFileRecord>,
	pub symbols: Vec<SymbolRecord>,
	pub references: Vec<ReferenceRecord>,
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
		mut symbols: Vec<SymbolRecord>,
	) -> Self {
		symbols.shrink_to_fit();
		Self {
			generation,
			catalog_generation,
			identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
			sources: Vec::new(),
			symbols,
			references: Vec::new(),
			timings: CodeIndexTimings::default(),
		}
	}

	pub fn with_references(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		mut symbols: Vec<SymbolRecord>,
		mut references: Vec<ReferenceRecord>,
	) -> Self {
		symbols.shrink_to_fit();
		references.shrink_to_fit();
		Self {
			generation,
			catalog_generation,
			identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
			sources: Vec::new(),
			symbols,
			references,
			timings: CodeIndexTimings::default(),
		}
	}

	pub fn from_fields(mut fields: CodeIndexFields) -> Self {
		fields.sources.shrink_to_fit();
		fields.symbols.shrink_to_fit();
		fields.references.shrink_to_fit();
		Self {
			generation: fields.generation,
			catalog_generation: fields.catalog_generation,
			identity_scheme: fields.identity_scheme,
			sources: fields.sources,
			symbols: fields.symbols,
			references: fields.references,
			timings: fields.timings,
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
pub struct LinkageGraph {
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
}

impl LinkageGraph {
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
		}
	}

	pub fn from_report(mut report: LinkageGraphReport) -> Self {
		report.resolved.shrink_to_fit();
		report.external.shrink_to_fit();
		report.manifest_blocked.shrink_to_fit();
		report.unresolved.shrink_to_fit();
		Self {
			generation: report.generation,
			index_generation: report.index_generation,
			resolved_refs: report.resolved_refs,
			external_refs: report.external_refs,
			manifest_blocked_refs: report.manifest_blocked_refs,
			unresolved_refs: report.unresolved_refs,
			ambiguous_refs: report.ambiguous_refs,
			resolved: report.resolved,
			external: report.external,
			manifest_blocked: report.manifest_blocked,
			unresolved: report.unresolved,
		}
	}
}

pub struct LinkageGraphReport {
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
		}
	}

	pub fn with_records(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		mut changes: Vec<ChangeRecord>,
	) -> Self {
		changes.shrink_to_fit();
		let changed_symbols = changes
			.iter()
			.filter_map(|change| change.symbol.clone())
			.fold(Vec::new(), |mut out, symbol| {
				if !out.contains(&symbol) {
					out.push(symbol);
				}
				out
			});
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
pub struct ChangeRecordFields {
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

	pub fn from_fields(fields: ChangeRecordFields) -> Self {
		Self {
			id: fields.id,
			status: fields.status,
			source: fields.source,
			source_uri: fields.source_uri,
			symbol: fields.symbol,
			identity: fields.identity,
			language: fields.language,
			file_path: fields.file_path,
			name: fields.name,
			kind: fields.kind,
			line_range: fields.line_range,
			hunk_count: fields.hunk_count,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkspaceSnapshot {
	pub generation: ResourceGeneration,
	pub catalog: SourceCatalog,
	pub index: CodeIndex,
	pub linkage: LinkageGraph,
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
	LinkageGraph,
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
