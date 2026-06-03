use std::path::{Component, Path, PathBuf};

pub(crate) fn absolute_path(path: &Path) -> PathBuf {
	let path = if path.is_absolute() {
		path.to_path_buf()
	} else {
		std::env::current_dir()
			.map(|cwd| cwd.join(path))
			.unwrap_or_else(|_| path.to_path_buf())
	};
	path.canonicalize().unwrap_or_else(|_| lexical_path(&path))
}

pub(crate) fn normalize_path(path: &Path) -> PathBuf {
	absolute_path(path)
}

pub(crate) fn lexical_path(path: &Path) -> PathBuf {
	let mut out = PathBuf::new();
	for component in path.components() {
		match component {
			Component::CurDir => {}
			Component::ParentDir => {
				out.pop();
			}
			_ => out.push(component.as_os_str()),
		}
	}
	out
}
