use std::path::PathBuf;

use crate::sources;
use crate::workspace::snapshot::{
	SourceCatalog, SourceUnit, WorkspaceFailure, WorkspaceRequest, WorkspaceResource,
	WorkspaceResult,
};

use super::content::{LocalResourceCache, SourceCatalogMaterial};
use super::identity::LocalIdentityResolver;

pub trait SourceCatalogPort {
	fn load_catalog(&mut self, request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog>;
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSourceCatalogOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub identity: LocalIdentityResolver,
}

impl LocalSourceCatalogOptions {
	pub fn new(paths: Vec<PathBuf>, project: Option<String>) -> Self {
		Self {
			paths,
			project,
			identity: LocalIdentityResolver::default(),
		}
	}

	pub fn with_identity(mut self, identity: LocalIdentityResolver) -> Self {
		self.identity = identity;
		self
	}
}

pub struct LocalSourceCatalog {
	options: LocalSourceCatalogOptions,
	cache: LocalResourceCache,
}

impl LocalSourceCatalog {
	pub fn new(options: LocalSourceCatalogOptions, cache: LocalResourceCache) -> Self {
		Self { options, cache }
	}
}

impl SourceCatalogPort for LocalSourceCatalog {
	fn load_catalog(&mut self, _request: &WorkspaceRequest) -> WorkspaceResult<SourceCatalog> {
		let sources = sources::discover(&self.options.paths, self.options.project.clone())
			.map_err(|err| {
				WorkspaceFailure::new(WorkspaceResource::SourceCatalog, err.to_string())
			})?;
		let generation = self.cache.next_generation();
		let units = sources
			.files
			.iter()
			.enumerate()
			.map(|(file_idx, file)| {
				SourceUnit::with_language(
					self.options
						.identity
						.source_id(file_idx, &file.rel_path)
						.as_str(),
					file.rel_path.display().to_string(),
					file.lang.tag(),
				)
			})
			.collect::<Vec<_>>();
		self.cache.insert_sources(
			generation,
			SourceCatalogMaterial {
				sources,
				identity: self.options.identity.clone(),
			},
		);
		Ok(SourceCatalog::new(generation, units))
	}
}
