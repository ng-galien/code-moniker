mod catalog;
mod content;
mod identity;

pub use catalog::{LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort};
pub use content::LocalResourceCache;
pub use identity::LocalIdentityResolver;

pub use content::{
	CodeIndexMaterial, IndexedSourceFile, MEMORY_SOURCE_ROOT, MEMORY_SOURCE_ROOT_LABEL,
	MemorySourceDocument, MemorySourceSet, MemorySourceSetUpdate, ResolvedSourceResource,
	SourceCatalogMaterial, is_memory_source_path,
};
