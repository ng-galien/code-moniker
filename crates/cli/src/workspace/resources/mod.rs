mod change_analyzer;
mod change_overlay;
mod code_index;
mod identity;
mod linkage;
mod material;
mod rule_diagnostics;
mod source_catalog;
mod symbol_provider;

pub use change_overlay::LocalChangeOverlay;
pub use code_index::{LocalCodeIndex, LocalCodeIndexOptions};
pub use identity::LocalIdentityResolver;
pub use linkage::LocalLinkage;
pub use material::LocalResourceCache;
pub use rule_diagnostics::{LocalCheckRunner, LocalCheckRunnerOptions};
pub use source_catalog::{LocalSourceCatalog, LocalSourceCatalogOptions};

#[cfg(test)]
mod tests;
