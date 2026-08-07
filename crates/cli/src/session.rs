use std::path::PathBuf;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SessionOptions {
	pub paths: Vec<PathBuf>,
	pub project: Option<String>,
	pub cache_dir: Option<PathBuf>,
}
