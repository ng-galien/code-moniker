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

pub(crate) fn portable_path(path: &Path) -> String {
	path.to_string_lossy()
		.replace(std::path::MAIN_SEPARATOR, "/")
}

pub(crate) fn portable_path_buf(path: &Path) -> PathBuf {
	PathBuf::from(portable_path(path))
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

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn portable_paths_use_forward_slashes() {
		let path = Path::new("src").join("engine").join("mod.rs");
		assert_eq!(portable_path(&path), "src/engine/mod.rs");
	}
}
