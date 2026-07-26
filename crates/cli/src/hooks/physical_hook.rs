use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

pub(super) fn validate(root: &Path, hook_path: &Path, config_path: &Path) -> anyhow::Result<()> {
	validate_existing_path(root, hook_path, "hook")?;
	validate_existing_path(root, config_path, "hook configuration")
}

pub(super) fn ensure_directories(root: &Path, hook_path: &Path) -> anyhow::Result<()> {
	let hooks_dir = hook_path
		.parent()
		.context("generated hook path has no parent directory")?;
	crate::fs_nofollow::ensure_dir(root, hooks_dir)
}

fn validate_existing_path(root: &Path, target: &Path, description: &str) -> anyhow::Result<()> {
	let relative = target.strip_prefix(root).with_context(|| {
		format!(
			"{description} path `{}` is outside project root `{}`",
			target.display(),
			root.display()
		)
	})?;
	let mut current = PathBuf::from(root);
	let component_count = relative.components().count();
	for (index, component) in relative.components().enumerate() {
		current.push(component.as_os_str());
		match fs::symlink_metadata(&current) {
			Ok(metadata) if metadata.file_type().is_symlink() => {
				bail!(
					"refusing linked {description} path component `{}`",
					current.display()
				);
			}
			Ok(metadata) if index + 1 < component_count && !metadata.is_dir() => {
				bail!(
					"{description} path component `{}` is not a directory",
					current.display()
				);
			}
			Ok(metadata) if index + 1 == component_count && !metadata.is_file() => {
				bail!(
					"{description} `{}` is not a regular file",
					current.display()
				);
			}
			Ok(_) => {}
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
			Err(error) => {
				return Err(error)
					.with_context(|| format!("cannot inspect `{}`", current.display()));
			}
		}
	}
	Ok(())
}
