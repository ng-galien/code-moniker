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
}

impl WorkspaceRequest {
	pub fn new(label: impl Into<String>) -> Self {
		Self {
			label: label.into(),
		}
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
	pub fn new(generation: ResourceGeneration, sources: Vec<SourceUnit>) -> Self {
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SymbolRecord {
	pub id: SymbolId,
	pub source: SourceId,
	pub identity: String,
	pub name: String,
	pub kind: String,
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
	pub target_identity: String,
	pub kind: String,
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
		target_identity: impl Into<String>,
		kind: impl Into<String>,
		line_range: Option<(u32, u32)>,
	) -> Self {
		Self {
			id: ReferenceId::new(id),
			source,
			source_symbol,
			target_identity: target_identity.into(),
			kind: kind.into(),
			confidence: None,
			receiver: None,
			alias: None,
			line_range,
		}
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
	pub source_root: usize,
	pub path: String,
	pub rel_path: String,
	pub anchor: String,
	pub language: String,
	pub text: String,
}

pub struct SourceFileRecordFields {
	pub id: SourceId,
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
	pub sources: Vec<SourceFileRecord>,
	pub symbols: Vec<SymbolRecord>,
	pub references: Vec<ReferenceRecord>,
}

pub struct CodeIndexFields {
	pub generation: ResourceGeneration,
	pub catalog_generation: ResourceGeneration,
	pub sources: Vec<SourceFileRecord>,
	pub symbols: Vec<SymbolRecord>,
	pub references: Vec<ReferenceRecord>,
}

impl CodeIndex {
	pub fn new(
		generation: ResourceGeneration,
		catalog_generation: ResourceGeneration,
		symbols: Vec<SymbolRecord>,
	) -> Self {
		Self {
			generation,
			catalog_generation,
			sources: Vec::new(),
			symbols,
			references: Vec::new(),
		}
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
			sources: Vec::new(),
			symbols,
			references,
		}
	}

	pub fn from_fields(fields: CodeIndexFields) -> Self {
		Self {
			generation: fields.generation,
			catalog_generation: fields.catalog_generation,
			sources: fields.sources,
			symbols: fields.symbols,
			references: fields.references,
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedReference {
	pub reference: ReferenceId,
	pub target_identity: String,
}

impl UnresolvedReference {
	pub fn new(reference: ReferenceId, target_identity: impl Into<String>) -> Self {
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
	pub unresolved_refs: usize,
	pub resolved: Vec<LinkageEdge>,
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
			unresolved_refs,
			resolved: Vec::new(),
			unresolved: Vec::new(),
		}
	}

	pub fn with_refs(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		resolved: Vec<LinkageEdge>,
		unresolved: Vec<UnresolvedReference>,
	) -> Self {
		Self {
			generation,
			index_generation,
			resolved_refs: resolved.len(),
			unresolved_refs: unresolved.len(),
			resolved,
			unresolved,
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
		changed_symbols: Vec<SymbolId>,
	) -> Self {
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
		changes: Vec<ChangeRecord>,
	) -> Self {
		let changed_symbols = changes
			.iter()
			.filter_map(|change| change.symbol.clone())
			.fold(Vec::new(), |mut out, symbol| {
				if !out.contains(&symbol) {
					out.push(symbol);
				}
				out
			});
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
		let mut overlay = Self::with_records(
			report.generation,
			report.catalog_generation,
			report.index_generation,
			report.changes,
		);
		overlay.scope = report.scope;
		overlay.resources = report.resources;
		overlay.diagnostics = report.diagnostics;
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
	pub symbol: Option<SymbolId>,
	pub identity: String,
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
	pub symbol: Option<SymbolId>,
	pub identity: String,
	pub name: String,
	pub kind: String,
	pub line_range: Option<(u32, u32)>,
	pub hunk_count: usize,
}

impl ChangeRecord {
	pub fn from_fields(fields: ChangeRecordFields) -> Self {
		Self {
			id: fields.id,
			status: fields.status,
			source: fields.source,
			symbol: fields.symbol,
			identity: fields.identity,
			name: fields.name,
			kind: fields.kind,
			line_range: fields.line_range,
			hunk_count: fields.hunk_count,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleDiagnostics {
	pub generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub errors: usize,
	pub warnings: usize,
	pub diagnostics: Vec<RuleDiagnostic>,
}

impl RuleDiagnostics {
	pub fn new(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		errors: usize,
		warnings: usize,
	) -> Self {
		Self {
			generation,
			index_generation,
			errors,
			warnings,
			diagnostics: Vec::new(),
		}
	}

	pub fn with_diagnostics(
		generation: ResourceGeneration,
		index_generation: ResourceGeneration,
		diagnostics: Vec<RuleDiagnostic>,
	) -> Self {
		let errors = diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.severity == RuleDiagnosticSeverity::Error)
			.count();
		let warnings = diagnostics
			.iter()
			.filter(|diagnostic| diagnostic.severity == RuleDiagnosticSeverity::Warn)
			.count();
		Self {
			generation,
			index_generation,
			errors,
			warnings,
			diagnostics,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RuleDiagnosticSeverity {
	Error,
	Warn,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuleDiagnostic {
	pub rule_id: String,
	pub severity: RuleDiagnosticSeverity,
	pub symbol: Option<SymbolId>,
	pub message: String,
}

impl RuleDiagnostic {
	pub fn new(
		rule_id: impl Into<String>,
		severity: RuleDiagnosticSeverity,
		symbol: Option<SymbolId>,
		message: impl Into<String>,
	) -> Self {
		Self {
			rule_id: rule_id.into(),
			severity,
			symbol,
			message: message.into(),
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
	pub diagnostics: RuleDiagnostics,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WorkspaceResource {
	SourceCatalog,
	CodeIndex,
	LinkageGraph,
	ChangeOverlay,
	RuleDiagnostics,
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
