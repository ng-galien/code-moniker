mod catalog;
mod content;
mod identity;

pub use catalog::{LocalSourceCatalog, LocalSourceCatalogOptions, SourceCatalogPort};
pub use content::LocalResourceCache;
pub use identity::LocalIdentityResolver;

pub(crate) use content::{
	CodeIndexMaterial, IndexedSourceFile, ResolvedSourceResource, SourceCatalogMaterial,
};
