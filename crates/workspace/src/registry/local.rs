use std::path::PathBuf;

use crate::changes::LocalChangeOverlay;
use crate::code::{LocalCodeIndex, LocalCodeIndexOptions};
use crate::extract::JavaExtractionPipeline;
use crate::linkage::LocalLinkage;
use crate::source::{
	LocalIdentityResolver, LocalResourceCache, LocalSourceCatalog, LocalSourceCatalogOptions,
};

use super::{WorkspacePorts, WorkspaceRegistry};

pub type LocalWorkspaceRegistry =
	WorkspaceRegistry<LocalSourceCatalog, LocalCodeIndex, LocalLinkage, LocalChangeOverlay>;

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

impl LocalWorkspaceRegistry {
	pub fn local(options: LocalWorkspaceOptions) -> Self {
		Self::local_with_cache(options, LocalResourceCache::default())
	}

	pub fn local_with_cache(options: LocalWorkspaceOptions, cache: LocalResourceCache) -> Self {
		Self::new(local_workspace_ports(options, cache))
	}
}

pub(crate) fn local_workspace_ports(
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
