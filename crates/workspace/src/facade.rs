use std::path::PathBuf;

use crate::changes::{ChangeOverlayPort, LocalChangeOverlay};
use crate::code::{CodeIndexPort, LocalCodeIndex, LocalCodeIndexOptions};
use crate::extract::JavaExtractionPipeline;
use crate::linkage::{LinkagePort, LocalLinkage};
use crate::snapshot::{
	WorkspaceFailure, WorkspaceRequest, WorkspaceSnapshot, WorkspaceSnapshotRefresh,
	WorkspaceTransition, WorkspaceView,
};
use crate::source::{
	LocalIdentityResolver, LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions,
	SourceCatalogPort,
};

pub struct WorkspacePorts<Sources, Index, Linkage, Changes> {
	pub source_catalog: Sources,
	pub code_index: Index,
	pub linkage: Linkage,
	pub change_overlay: Changes,
}

impl<Sources, Index, Linkage, Changes> WorkspacePorts<Sources, Index, Linkage, Changes> {
	pub fn new(
		source_catalog: Sources,
		code_index: Index,
		linkage: Linkage,
		change_overlay: Changes,
	) -> Self {
		Self {
			source_catalog,
			code_index,
			linkage,
			change_overlay,
		}
	}
}

pub struct WorkspaceFacade<Sources, Index, Linkage, Changes> {
	refresh: WorkspaceSnapshotRefresh<Sources, Index, Linkage, Changes>,
}

impl<Sources, Index, Linkage, Changes> WorkspaceFacade<Sources, Index, Linkage, Changes>
where
	Sources: SourceCatalogPort,
	Index: CodeIndexPort,
	Linkage: LinkagePort,
	Changes: ChangeOverlayPort,
{
	pub fn new(ports: WorkspacePorts<Sources, Index, Linkage, Changes>) -> Self {
		Self {
			refresh: WorkspaceSnapshotRefresh::new(
				ports.source_catalog,
				ports.code_index,
				ports.linkage,
				ports.change_overlay,
			),
		}
	}

	pub fn refresh(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.refresh.refresh(request)
	}

	pub fn load_catalog(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.refresh.load_catalog(request)
	}

	pub fn load_index(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.refresh.load_index(request)
	}

	pub fn resolve_linkage(&mut self, request: WorkspaceRequest) -> WorkspaceTransition {
		self.refresh.resolve_linkage(request)
	}

	pub fn replace_snapshot(&mut self, snapshot: WorkspaceSnapshot) {
		self.refresh.replace_snapshot(snapshot);
	}

	pub fn snapshot(&self) -> Option<&WorkspaceSnapshot> {
		self.refresh.snapshot()
	}

	pub fn view(&self) -> Option<WorkspaceView<'_>> {
		self.snapshot().map(WorkspaceView::new)
	}

	pub fn last_failure(&self) -> Option<&WorkspaceFailure> {
		self.refresh.last_failure()
	}
}

pub type LocalWorkspaceFacade =
	WorkspaceFacade<LocalSourceCatalog, LocalCodeIndex, LocalLinkage, LocalChangeOverlay>;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWorkspaceOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
	pub files: Option<Vec<PathBuf>>,
	pub identity: LocalIdentityResolver,
	pub java_pipeline: JavaExtractionPipeline,
}

impl LocalWorkspaceOptions {
	pub fn new(paths: Vec<PathBuf>, project: Option<String>) -> Self {
		Self {
			paths,
			project,
			cache_dir: None,
			files: None,
			identity: LocalIdentityResolver::default(),
			java_pipeline: JavaExtractionPipeline::default(),
		}
	}

	pub fn with_cache_dir(mut self, cache_dir: Option<PathBuf>) -> Self {
		self.cache_dir = cache_dir;
		self
	}

	pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
		self.files = Some(files);
		self
	}

	pub fn with_identity(mut self, identity: LocalIdentityResolver) -> Self {
		self.identity = identity;
		self
	}

	pub fn with_java_pipeline(mut self, java_pipeline: JavaExtractionPipeline) -> Self {
		self.java_pipeline = java_pipeline;
		self
	}
}

impl LocalWorkspaceFacade {
	pub fn local(options: LocalWorkspaceOptions) -> Self {
		let cache = LocalResourceCache::default();
		Self::new(local_workspace_ports(options, cache))
	}
}

pub fn local_workspace_ports(
	options: LocalWorkspaceOptions,
	cache: LocalResourceCache,
) -> WorkspacePorts<LocalSourceCatalog, LocalCodeIndex, LocalLinkage, LocalChangeOverlay> {
	let mut source_options = LocalSourceCatalogOptions::new(options.paths, options.project)
		.with_identity(options.identity)
		.with_java_pipeline(options.java_pipeline);
	if let Some(files) = options.files {
		source_options = source_options.with_files(files);
	}
	WorkspacePorts::new(
		LocalSourceCatalog::new(source_options, cache.clone()),
		LocalCodeIndex::new(LocalCodeIndexOptions::new(options.cache_dir), cache.clone()),
		LocalLinkage::new(cache.clone()),
		LocalChangeOverlay::new(cache),
	)
}
