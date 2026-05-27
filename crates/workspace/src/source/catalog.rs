use std::path::PathBuf;

use crate::environment;
use crate::extract::RustExtractionPipeline;
use crate::snapshot::{
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
	pub files: Option<Vec<PathBuf>>,
	pub project: Option<String>,
	pub identity: LocalIdentityResolver,
	pub rust_pipeline: RustExtractionPipeline,
}

impl LocalSourceCatalogOptions {
	pub fn new(paths: Vec<PathBuf>, project: Option<String>) -> Self {
		Self {
			paths,
			files: None,
			project,
			identity: LocalIdentityResolver::default(),
			rust_pipeline: RustExtractionPipeline::default(),
		}
	}

	pub fn with_files(mut self, files: Vec<PathBuf>) -> Self {
		self.files = Some(files);
		self
	}

	pub fn with_identity(mut self, identity: LocalIdentityResolver) -> Self {
		self.identity = identity;
		self
	}

	pub fn with_rust_pipeline(mut self, rust_pipeline: RustExtractionPipeline) -> Self {
		self.rust_pipeline = rust_pipeline;
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
		let mut sources = if let Some(files) = &self.options.files {
			let [root] = self.options.paths.as_slice() else {
				return Err(WorkspaceFailure::new(
					WorkspaceResource::SourceCatalog,
					"explicit source files require exactly one source root",
				));
			};
			environment::discover_source_files(root, files, self.options.project.clone())
		} else {
			environment::discover_sources(&self.options.paths, self.options.project.clone())
		}
		.map_err(|err| WorkspaceFailure::new(WorkspaceResource::SourceCatalog, err.to_string()))?;
		for root in &mut sources.roots {
			root.ctx.rust_pipeline = self.options.rust_pipeline;
		}
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
