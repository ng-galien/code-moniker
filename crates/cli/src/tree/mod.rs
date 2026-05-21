pub(crate) mod strategy;

#[cfg(feature = "pretty")]
mod files;
#[cfg(feature = "pretty")]
mod outline;
#[cfg(feature = "pretty")]
mod style;

#[cfg(feature = "pretty")]
pub(crate) use files::{FileEntry, write_files_tree};
#[cfg(feature = "pretty")]
pub(crate) use outline::write_tree;
