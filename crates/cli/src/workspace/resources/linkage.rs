use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::lang::Lang;

use crate::workspace::resources::material::{CodeIndexMaterial, LocalResourceCache};
use crate::workspace::session::{
	CodeIndex, LinkageEdge, LinkageGraph, LinkagePort, ReferenceRecord, SymbolId,
	UnresolvedReference, WorkspaceFailure, WorkspaceResource, WorkspaceResult,
};

pub struct LocalLinkage {
	cache: LocalResourceCache,
}

impl LocalLinkage {
	pub fn new(cache: LocalResourceCache) -> Self {
		Self { cache }
	}
}

impl LinkagePort for LocalLinkage {
	fn resolve_linkage(&mut self, index: &CodeIndex) -> WorkspaceResult<LinkageGraph> {
		let material = self.cache.index_material(index.generation).ok_or_else(|| {
			WorkspaceFailure::new(
				WorkspaceResource::LinkageGraph,
				"code index material is unavailable",
			)
		})?;
		let generation = self.cache.next_generation();
		let resolver = LinkageResolver::new(&material);
		let outcome = resolver.resolve(index);
		Ok(LinkageGraph::with_refs(
			generation,
			index.generation,
			outcome.resolved,
			outcome.unresolved,
		))
	}
}

struct LinkageResolver<'a> {
	material: &'a CodeIndexMaterial,
	local: LocalScopeResolver,
	global: GlobalScopeResolver,
}

impl<'a> LinkageResolver<'a> {
	fn new(material: &'a CodeIndexMaterial) -> Self {
		Self {
			material,
			local: LocalScopeResolver,
			global: GlobalScopeResolver,
		}
	}

	fn resolve(&self, index: &CodeIndex) -> LinkageOutcome {
		let mut outcome = LinkageOutcome::default();
		let candidates = CandidateCatalog::new(self.material);
		for reference in &index.references {
			let Some(query) = LinkageQuery::new(reference, self.material) else {
				outcome.unresolved(reference);
				continue;
			};
			if let Some(target) = self.local.resolve(&query, &candidates) {
				outcome.resolved(reference, target);
			} else if let Some(target) = self.global.resolve(&query, &candidates) {
				outcome.resolved(reference, target);
			} else {
				outcome.unresolved(reference);
			}
		}
		outcome
	}
}

#[derive(Default)]
struct LinkageOutcome {
	resolved: Vec<LinkageEdge>,
	unresolved: Vec<UnresolvedReference>,
}

impl LinkageOutcome {
	fn resolved(&mut self, reference: &ReferenceRecord, target: SymbolId) {
		self.resolved
			.push(LinkageEdge::new(reference.id.clone(), target));
	}

	fn unresolved(&mut self, reference: &ReferenceRecord) {
		self.unresolved.push(UnresolvedReference::new(
			reference.id.clone(),
			reference.target_identity.clone(),
		));
	}
}

struct LinkageQuery<'a> {
	target: &'a Moniker,
	source_file: usize,
	strategy: &'static dyn LanguageLinkageStrategy,
}

impl<'a> LinkageQuery<'a> {
	fn new(reference: &'a ReferenceRecord, material: &'a CodeIndexMaterial) -> Option<Self> {
		let target = material.reference_targets.get(&reference.id)?;
		let source_file = material.identity.source_index(&reference.source)?;
		let lang = material.files.get(source_file)?.lang;
		Some(Self {
			target,
			source_file,
			strategy: language_strategy(lang),
		})
	}

	fn matches(&self, candidate: &LinkageCandidate<'_>) -> bool {
		self.strategy.matches(self, candidate)
	}
}

struct LinkageCandidate<'a> {
	symbol: &'a SymbolId,
	moniker: &'a Moniker,
	source_file: usize,
}

struct CandidateCatalog<'a> {
	material: &'a CodeIndexMaterial,
}

impl<'a> CandidateCatalog<'a> {
	fn new(material: &'a CodeIndexMaterial) -> Self {
		Self { material }
	}

	fn local_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.matches(|candidate| {
			candidate.source_file == query.source_file && query.matches(candidate)
		})
	}

	fn global_matches(&self, query: &LinkageQuery<'_>) -> Vec<LinkageCandidate<'a>> {
		self.matches(|candidate| {
			candidate.source_file != query.source_file && query.matches(candidate)
		})
	}

	fn matches(&self, accept: impl Fn(&LinkageCandidate<'_>) -> bool) -> Vec<LinkageCandidate<'a>> {
		self.material
			.symbol_monikers
			.iter()
			.filter_map(|(symbol, moniker)| self.candidate(symbol, moniker))
			.filter(accept)
			.collect()
	}

	fn candidate(
		&self,
		symbol: &'a SymbolId,
		moniker: &'a Moniker,
	) -> Option<LinkageCandidate<'a>> {
		let (source_file, _) = self.material.identity.symbol_location(symbol)?;
		Some(LinkageCandidate {
			symbol,
			moniker,
			source_file,
		})
	}
}

struct LocalScopeResolver;

impl LocalScopeResolver {
	fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Option<SymbolId> {
		first_candidate(candidates.local_matches(query))
	}
}

struct GlobalScopeResolver;

impl GlobalScopeResolver {
	fn resolve(
		&self,
		query: &LinkageQuery<'_>,
		candidates: &CandidateCatalog<'_>,
	) -> Option<SymbolId> {
		first_candidate(candidates.global_matches(query))
	}
}

trait LanguageLinkageStrategy: Sync {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool;
}

struct GenericLanguageLinkageStrategy;

impl LanguageLinkageStrategy for GenericLanguageLinkageStrategy {
	fn matches(&self, query: &LinkageQuery<'_>, candidate: &LinkageCandidate<'_>) -> bool {
		candidate.moniker.bind_match(query.target) || query.target.bind_match(candidate.moniker)
	}
}

static GENERIC_STRATEGY: GenericLanguageLinkageStrategy = GenericLanguageLinkageStrategy;

fn language_strategy(_lang: Lang) -> &'static dyn LanguageLinkageStrategy {
	&GENERIC_STRATEGY
}

fn first_candidate(candidates: Vec<LinkageCandidate<'_>>) -> Option<SymbolId> {
	candidates.first().map(|candidate| candidate.symbol.clone())
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use code_moniker_core::core::code_graph::CodeGraph;
	use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};
	use rustc_hash::FxHashMap;

	use super::*;
	use crate::sources;
	use crate::workspace::resources::identity::LocalIdentityResolver;
	use crate::workspace::resources::material::{
		CodeIndexMaterial, IndexedSourceFile, SourceCatalogMaterial,
	};
	use crate::workspace::session::{
		CodeIndexFields, ReferenceRecord, ResourceGeneration, SourceFileRecord,
		SourceFileRecordFields,
	};

	#[test]
	fn local_linkage_wins_before_global_candidates() {
		let target = sample_moniker("target");
		let mut model = LinkageModel::new(vec![
			(0, "src/local.rs", target.clone()),
			(1, "src/global.rs", target.clone()),
		]);
		let reference = model.reference_for_file(0, target.clone());

		let outcome = LinkageResolver::new(&model.material).resolve(&model.index(reference));

		assert_eq!(outcome.resolved.len(), 1);
		assert_eq!(outcome.resolved[0].target, model.symbol_id(0));
	}

	#[test]
	fn global_linkage_resolves_when_local_scope_has_no_candidate() {
		let target = sample_moniker("target");
		let other = sample_moniker("other");
		let mut model = LinkageModel::new(vec![
			(0, "src/local.rs", other),
			(1, "src/global.rs", target.clone()),
		]);
		let reference = model.reference_for_file(0, target);

		let outcome = LinkageResolver::new(&model.material).resolve(&model.index(reference));

		assert_eq!(outcome.resolved.len(), 1);
		assert_eq!(outcome.resolved[0].target, model.symbol_id(1));
	}

	struct LinkageModel {
		identity: LocalIdentityResolver,
		material: CodeIndexMaterial,
		sources: Vec<SourceFileRecord>,
	}

	impl LinkageModel {
		fn new(files: Vec<(usize, &'static str, Moniker)>) -> Self {
			let identity = LocalIdentityResolver::default();
			let indexed_files = indexed_files(&identity, &files);
			let sources = source_records(&identity, &files);
			let symbol_monikers = symbol_monikers(&identity, files);
			Self {
				identity: identity.clone(),
				material: CodeIndexMaterial {
					source_catalog: source_catalog(identity.clone()),
					files: indexed_files,
					identity,
					symbols_by_moniker: FxHashMap::default(),
					symbol_monikers,
					reference_targets: FxHashMap::default(),
				},
				sources,
			}
		}

		fn index(&self, reference: ReferenceRecord) -> CodeIndex {
			CodeIndex::from_fields(CodeIndexFields {
				generation: ResourceGeneration::new(1),
				catalog_generation: ResourceGeneration::new(0),
				identity_scheme: self.identity.scheme().to_string(),
				sources: self.sources.clone(),
				symbols: Vec::new(),
				references: vec![reference],
			})
		}

		fn reference_for_file(&mut self, file_idx: usize, target: Moniker) -> ReferenceRecord {
			self.material
				.reference_targets
				.insert(reference_id(file_idx), target.clone());
			let source = self
				.sources
				.get(file_idx)
				.map(|source| source.id.clone())
				.expect("test source exists");
			ReferenceRecord::new(
				reference_id(file_idx).as_str(),
				source,
				self.symbol_id(file_idx),
				self.identity.moniker_uri(&target),
				"call",
				None,
			)
		}

		fn symbol_id(&self, file_idx: usize) -> SymbolId {
			self.identity.symbol_id(file_idx, 0)
		}
	}

	fn indexed_files(
		identity: &LocalIdentityResolver,
		files: &[(usize, &'static str, Moniker)],
	) -> Vec<IndexedSourceFile> {
		files
			.iter()
			.map(|(file_idx, rel_path, _)| indexed_file(identity, *file_idx, rel_path))
			.collect()
	}

	fn indexed_file(
		identity: &LocalIdentityResolver,
		file_idx: usize,
		rel_path: &str,
	) -> IndexedSourceFile {
		let rel_path = PathBuf::from(rel_path);
		IndexedSourceFile {
			source_root: 0,
			source_id: identity.source_id(file_idx, &rel_path),
			source_uri: identity.source_uri(&rel_path),
			identity: identity.clone(),
			path: rel_path.clone(),
			rel_path: rel_path.clone(),
			anchor: rel_path,
			lang: Lang::Rs,
			graph: CodeGraph::new(sample_moniker("file"), b"file"),
			source: String::new(),
		}
	}

	fn source_records(
		identity: &LocalIdentityResolver,
		files: &[(usize, &'static str, Moniker)],
	) -> Vec<SourceFileRecord> {
		files
			.iter()
			.map(|(file_idx, rel_path, _)| source_record(identity, *file_idx, rel_path))
			.collect()
	}

	fn source_record(
		identity: &LocalIdentityResolver,
		file_idx: usize,
		rel_path: &str,
	) -> SourceFileRecord {
		let rel_path = PathBuf::from(rel_path);
		SourceFileRecord::from_fields(SourceFileRecordFields {
			id: identity.source_id(file_idx, &rel_path),
			uri: identity.source_uri(&rel_path),
			source_root: 0,
			path: rel_path.display().to_string(),
			rel_path: rel_path.display().to_string(),
			anchor: rel_path.display().to_string(),
			language: Lang::Rs.tag().to_string(),
			text: String::new(),
		})
	}

	fn source_catalog(identity: LocalIdentityResolver) -> SourceCatalogMaterial {
		SourceCatalogMaterial {
			sources: sources::SourceSet {
				roots: Vec::new(),
				files: Vec::new(),
				multi: false,
			},
			identity,
		}
	}

	fn symbol_monikers(
		identity: &LocalIdentityResolver,
		files: Vec<(usize, &'static str, Moniker)>,
	) -> FxHashMap<SymbolId, Moniker> {
		files
			.into_iter()
			.map(|(file_idx, _, moniker)| (identity.symbol_id(file_idx, 0), moniker))
			.collect()
	}

	fn reference_id(file_idx: usize) -> crate::workspace::session::ReferenceId {
		crate::workspace::session::ReferenceId::new(format!("reference:{file_idx}:0"))
	}

	fn sample_moniker(name: &str) -> Moniker {
		MonikerBuilder::new()
			.project(b"demo")
			.segment(b"fn", name.as_bytes())
			.build()
	}
}
