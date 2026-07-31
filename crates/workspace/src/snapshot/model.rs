use std::mem::size_of;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
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

#[derive(Clone, Debug)]
pub struct WorkspaceRequest {
	pub label: String,
	pub catalog: CatalogRequest,
	cancellation: WorkspaceCancellation,
}

impl PartialEq for WorkspaceRequest {
	fn eq(&self, other: &Self) -> bool {
		self.label == other.label && self.catalog == other.catalog
	}
}

impl Eq for WorkspaceRequest {}

#[derive(Clone, Debug, Default)]
pub struct WorkspaceCancellation(Arc<AtomicBool>);

impl WorkspaceCancellation {
	pub fn cancel(&self) {
		self.0.store(true, Ordering::Release);
	}

	pub fn is_cancelled(&self) -> bool {
		self.0.load(Ordering::Acquire)
	}

	pub fn check(&self, resource: WorkspaceResource) -> WorkspaceResult<()> {
		if self.is_cancelled() {
			return Err(WorkspaceFailure::new(resource, "workspace build cancelled"));
		}
		Ok(())
	}
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
			cancellation: WorkspaceCancellation::default(),
		}
	}

	pub fn with_cancellation(mut self, cancellation: WorkspaceCancellation) -> Self {
		self.cancellation = cancellation;
		self
	}

	pub fn cancellation(&self) -> &WorkspaceCancellation {
		&self.cancellation
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
	pub call_name: Option<String>,
	pub call_arity: Option<usize>,
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
			call_name: None,
			call_arity: None,
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
	pub inventory: Arc<super::SymbolInventoryIndex>,
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
		let sources = Vec::new();
		let symbols = RecordTable::from_records(symbols);
		let inventory = Arc::new(super::SymbolInventoryIndex::build(
			generation, &sources, &symbols,
		));
		Self {
			generation,
			catalog_generation,
			identity_scheme: crate::DEFAULT_IDENTITY_SCHEME.to_string(),
			sources,
			symbols,
			references: RecordTable::from_records(references),
			inventory,
			timings: CodeIndexTimings::default(),
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkageEdge {
	pub reference: ReferenceId,
	pub target: SymbolId,
	pub evidence: ResolutionEvidence,
}

impl LinkageEdge {
	pub fn new(reference: ReferenceId, target: SymbolId) -> Self {
		Self::with_evidence(reference, target, ResolutionEvidence::ExactBinding)
	}

	pub fn with_evidence(
		reference: ReferenceId,
		target: SymbolId,
		evidence: ResolutionEvidence,
	) -> Self {
		Self {
			reference,
			target,
			evidence,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResolutionEvidence {
	ExactBinding,
	LocalBinding,
	GlobalBinding,
	TypeConstraint,
	Mro,
	Injected,
	NameMatch,
}

impl ResolutionEvidence {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::ExactBinding => "exact_binding",
			Self::LocalBinding => "local_binding",
			Self::GlobalBinding => "global_binding",
			Self::TypeConstraint => "type_constraint",
			Self::Mro => "mro",
			Self::Injected => "injected",
			Self::NameMatch => "name_match",
		}
	}

	pub fn rank(self) -> u8 {
		match self {
			Self::ExactBinding => 100,
			Self::LocalBinding | Self::TypeConstraint => 90,
			Self::Mro => 85,
			Self::GlobalBinding | Self::Injected => 80,
			Self::NameMatch => 10,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateReason {
	WeakNameMatch,
	MultipleTargets,
	AmbiguousLookup,
}

impl CandidateReason {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::WeakNameMatch => "weak_name_match",
			Self::MultipleTargets => "multiple_targets",
			Self::AmbiguousLookup => "ambiguous_lookup",
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CandidateScope {
	Local,
	Global,
	Builtin,
	Injected,
	Unknown,
}

impl CandidateScope {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::Local => "local",
			Self::Global => "global",
			Self::Builtin => "builtin",
			Self::Injected => "injected",
			Self::Unknown => "unknown",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CandidateReference {
	pub reference: ReferenceId,
	pub targets: Vec<SymbolId>,
	pub reason: CandidateReason,
	pub scope: CandidateScope,
	pub evidence: ResolutionEvidence,
}

impl CandidateReference {
	pub fn new(
		reference: ReferenceId,
		targets: Vec<SymbolId>,
		reason: CandidateReason,
		scope: CandidateScope,
		evidence: ResolutionEvidence,
	) -> Self {
		Self {
			reference,
			targets,
			reason,
			scope,
			evidence,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DynamicReason {
	DynamicAttribute,
	DescriptorOrFrameworkInjected,
	DuckTypedCandidateSet,
	MixinContract,
	ExternalDependencyUnindexed,
	RuntimeImport,
	RuntimeMutation,
	PreprocessorExpansion,
	InsufficientLocalFacts,
}

impl DynamicReason {
	pub fn as_str(self) -> &'static str {
		match self {
			Self::DynamicAttribute => "dynamic_attribute",
			Self::DescriptorOrFrameworkInjected => "descriptor_or_framework_injected",
			Self::DuckTypedCandidateSet => "duck_typed_candidate_set",
			Self::MixinContract => "mixin_contract",
			Self::ExternalDependencyUnindexed => "external_dependency_unindexed",
			Self::RuntimeImport => "runtime_import",
			Self::RuntimeMutation => "runtime_mutation",
			Self::PreprocessorExpansion => "preprocessor_expansion",
			Self::InsufficientLocalFacts => "insufficient_local_facts",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DynamicReference {
	pub reference: ReferenceId,
	pub target_identity: Arc<str>,
	pub reason: DynamicReason,
	pub candidates: Vec<SymbolId>,
}

impl DynamicReference {
	pub fn new(
		reference: ReferenceId,
		target_identity: impl Into<Arc<str>>,
		reason: DynamicReason,
		candidates: Vec<SymbolId>,
	) -> Self {
		Self {
			reference,
			target_identity: target_identity.into(),
			reason,
			candidates,
		}
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExternalReferenceOrigin {
	Sdk,
	Dependency,
	Injected,
	UnknownExternal,
}

impl ExternalReferenceOrigin {
	pub fn label(self) -> &'static str {
		match self {
			Self::Sdk => "sdk",
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

/// Why linkage could not bind a reference. The taxonomy is part of the
/// resolution contract: consumers can split external-by-design from real
/// resolution gaps instead of reading one opaque count.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub enum UnresolvedReason {
	ManifestBlocked,
	Visibility,
	LanguageBoundary,
	MissingQuery,
	NoCandidate,
	Ambiguous,
	UnsupportedLanguageRule,
	IncompleteExtractorMetadata,
}

impl UnresolvedReason {
	pub fn as_str(&self) -> &'static str {
		match self {
			Self::ManifestBlocked => "manifest_blocked",
			Self::Visibility => "visibility",
			Self::LanguageBoundary => "language_boundary",
			Self::MissingQuery => "missing_query",
			Self::NoCandidate => "no_candidate",
			Self::Ambiguous => "ambiguous",
			Self::UnsupportedLanguageRule => "unsupported_language_rule",
			Self::IncompleteExtractorMetadata => "incomplete_extractor_metadata",
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UnresolvedReference {
	pub reference: ReferenceId,
	pub target_identity: Arc<str>,
	pub reason: UnresolvedReason,
}

impl UnresolvedReference {
	pub fn new(
		reference: ReferenceId,
		target_identity: impl Into<Arc<str>>,
		reason: UnresolvedReason,
	) -> Self {
		Self {
			reference,
			target_identity: target_identity.into(),
			reason,
		}
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LinkageSnapshot {
	pub generation: ResourceGeneration,
	pub index_generation: ResourceGeneration,
	pub resolved_refs: usize,
	pub candidate_refs: usize,
	pub external_refs: usize,
	pub dynamic_refs: usize,
	pub blocked_refs: usize,
	/// Compatibility counter for manifest-policy blocks.
	pub manifest_blocked_refs: usize,
	pub unresolved_refs: usize,
	/// Compatibility counter for existing consumers. Candidate references are
	/// now stored separately and never projected as graph edges.
	pub ambiguous_refs: usize,
	pub resolved: Vec<LinkageEdge>,
	pub candidates: Vec<CandidateReference>,
	pub external: Vec<ExternalReference>,
	pub dynamic: Vec<DynamicReference>,
	pub blocked: Vec<UnresolvedReference>,
	/// Compatibility view containing only `ManifestBlocked` entries.
	pub manifest_blocked: Vec<UnresolvedReference>,
	pub unresolved: Vec<UnresolvedReference>,
	pub read_index: LinkageReadIndexHandle,
}

#[derive(Debug)]
pub struct LinkageReadIndex {
	pub(crate) incoming: rustc_hash::FxHashMap<SymbolId, Vec<ReferenceId>>,
	pub(crate) targets: rustc_hash::FxHashMap<ReferenceId, SymbolId>,
	pub(crate) ordinals: rustc_hash::FxHashMap<SymbolId, u32>,
	pub(crate) symbols: rustc_hash::FxHashMap<u32, SymbolId>,
	pub(crate) outgoing: OutgoingPathIndex,
}

type OutgoingPathIndex =
	rustc_hash::FxHashMap<u32, rustc_hash::FxHashMap<Arc<str>, Vec<ReferenceId>>>;

impl LinkageReadIndex {
	pub(crate) fn estimated_heap_bytes(&self) -> usize {
		let incoming = self.incoming.capacity()
			* (size_of::<SymbolId>() + size_of::<Vec<ReferenceId>>())
			+ self
				.incoming
				.values()
				.map(|references| references.capacity() * size_of::<ReferenceId>())
				.sum::<usize>();
		let targets = self.targets.capacity() * (size_of::<ReferenceId>() + size_of::<SymbolId>());
		let ordinals = self.ordinals.capacity() * (size_of::<SymbolId>() + size_of::<u32>());
		let symbols = self.symbols.capacity() * (size_of::<u32>() + size_of::<SymbolId>());
		let mut relation_strings = std::collections::HashSet::<(usize, usize)>::new();
		let outgoing = self.outgoing.capacity()
			* (size_of::<u32>() + size_of::<rustc_hash::FxHashMap<Arc<str>, Vec<ReferenceId>>>())
			+ self
				.outgoing
				.values()
				.map(|relations| {
					relations.capacity() * (size_of::<Arc<str>>() + size_of::<Vec<ReferenceId>>())
						+ relations
							.iter()
							.map(|(relation, references)| {
								let string_bytes = if relation_strings
									.insert((relation.as_ptr() as usize, relation.len()))
								{
									relation.len()
								} else {
									0
								};
								string_bytes + references.capacity() * size_of::<ReferenceId>()
							})
							.sum::<usize>()
				})
				.sum::<usize>();
		incoming + targets + ordinals + symbols + outgoing
	}

	pub fn from_edges(edges: &[LinkageEdge]) -> Self {
		let mut incoming = rustc_hash::FxHashMap::<SymbolId, Vec<ReferenceId>>::default();
		let mut targets = rustc_hash::FxHashMap::<ReferenceId, SymbolId>::default();
		for edge in edges {
			let LinkageEdge {
				reference, target, ..
			} = edge.clone();
			targets.entry(reference).or_insert(target);
			incoming.entry(target).or_default().push(reference);
		}
		Self {
			incoming,
			targets,
			ordinals: rustc_hash::FxHashMap::default(),
			symbols: rustc_hash::FxHashMap::default(),
			outgoing: rustc_hash::FxHashMap::default(),
		}
	}

	pub(crate) fn from_edges_with_ordinals(
		edges: &[LinkageEdge],
		references: &RecordTable<ReferenceRecord>,
		ordinals: impl IntoIterator<Item = (u32, SymbolId)>,
	) -> Self {
		let mut index = Self::from_edges(edges);
		for (ordinal, symbol) in ordinals {
			index.symbols.insert(ordinal, symbol);
			index.ordinals.insert(symbol, ordinal);
		}
		index.outgoing = outgoing_path_index(references, &index.ordinals);
		index
	}

	pub fn incoming(&self, symbol: &SymbolId) -> &[ReferenceId] {
		self.incoming.get(symbol).map(Vec::as_slice).unwrap_or(&[])
	}

	pub fn resolved_target(&self, reference: &ReferenceId) -> Option<&SymbolId> {
		self.targets.get(reference)
	}

	pub(crate) fn ordinal(&self, symbol: &SymbolId) -> Option<u32> {
		self.ordinals.get(symbol).copied()
	}

	pub(crate) fn symbol(&self, ordinal: u32) -> Option<SymbolId> {
		self.symbols.get(&ordinal).copied()
	}

	pub(crate) fn outgoing(&self, ordinal: u32, relation: &str) -> &[ReferenceId] {
		self.outgoing
			.get(&ordinal)
			.and_then(|by_relation| by_relation.get(relation))
			.map(Vec::as_slice)
			.unwrap_or(&[])
	}

	pub(crate) fn outgoing_relations(&self, ordinal: u32) -> impl Iterator<Item = &str> {
		self.outgoing
			.get(&ordinal)
			.into_iter()
			.flat_map(|by_relation| by_relation.keys().map(AsRef::as_ref))
	}

	#[cfg(test)]
	pub(crate) fn active_symbol_slots(&self) -> usize {
		self.symbols.len()
	}
}

fn outgoing_path_index(
	references: &RecordTable<ReferenceRecord>,
	ordinals: &rustc_hash::FxHashMap<SymbolId, u32>,
) -> OutgoingPathIndex {
	let mut index = OutgoingPathIndex::default();
	for reference in references.iter() {
		let Some(source_ordinal) = ordinals.get(&reference.source_symbol).copied() else {
			continue;
		};
		let by_relation = index.entry(source_ordinal).or_default();
		if let Some(outgoing) = by_relation.get_mut(reference.kind.as_str()) {
			outgoing.push(reference.id);
		} else {
			by_relation.insert(Arc::from(reference.kind.as_str()), vec![reference.id]);
		}
	}
	index
}

#[derive(Clone, Debug, Default)]
pub struct LinkageReadIndexHandle(Option<Arc<LinkageReadIndex>>);

impl LinkageReadIndexHandle {
	pub(crate) fn estimated_heap_bytes(&self) -> usize {
		self.0.as_deref().map_or(0, |index| {
			size_of::<LinkageReadIndex>() + index.estimated_heap_bytes()
		})
	}

	pub fn from_edges(edges: &[LinkageEdge]) -> Self {
		Self(Some(Arc::new(LinkageReadIndex::from_edges(edges))))
	}

	pub(crate) fn from_edges_with_ordinals(
		edges: &[LinkageEdge],
		references: &RecordTable<ReferenceRecord>,
		ordinals: impl IntoIterator<Item = (u32, SymbolId)>,
	) -> Self {
		Self(Some(Arc::new(LinkageReadIndex::from_edges_with_ordinals(
			edges, references, ordinals,
		))))
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
			candidate_refs: 0,
			external_refs: 0,
			dynamic_refs: 0,
			blocked_refs: 0,
			manifest_blocked_refs: 0,
			unresolved_refs,
			ambiguous_refs: 0,
			resolved: Vec::new(),
			candidates: Vec::new(),
			external: Vec::new(),
			dynamic: Vec::new(),
			blocked: Vec::new(),
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
			candidate_refs: 0,
			external_refs: 0,
			dynamic_refs: 0,
			blocked_refs: 0,
			manifest_blocked_refs: 0,
			unresolved_refs: unresolved.len(),
			ambiguous_refs: 0,
			resolved,
			candidates: Vec::new(),
			external: Vec::new(),
			dynamic: Vec::new(),
			blocked: Vec::new(),
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
