use std::ffi::OsString;
use std::fs;
use std::path::Path;

use anyhow::Context;

pub(crate) fn write_atomic(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
	if fs::symlink_metadata(path)
		.map(|metadata| metadata.file_type().is_symlink())
		.unwrap_or(false)
	{
		let target = path
			.canonicalize()
			.with_context(|| format!("cannot resolve linked configuration `{}`", path.display()))?;
		return write_atomic_regular(&target, contents);
	}
	write_atomic_regular(path, contents)
}

fn write_atomic_regular(path: &Path, contents: &[u8]) -> anyhow::Result<()> {
	let parent = path
		.parent()
		.with_context(|| format!("`{}` has no parent directory", path.display()))?;
	fs::create_dir_all(parent).with_context(|| format!("cannot create `{}`", parent.display()))?;
	let permissions = fs::metadata(path)
		.ok()
		.map(|metadata| metadata.permissions());
	let mut temporary_name: OsString = path
		.file_name()
		.context("managed file has no file name")?
		.to_os_string();
	temporary_name.push(format!(".{}.tmp", std::process::id()));
	let temporary = parent.join(temporary_name);
	fs::write(&temporary, contents)
		.with_context(|| format!("cannot write `{}`", temporary.display()))?;
	if let Some(permissions) = permissions {
		fs::set_permissions(&temporary, permissions).with_context(|| {
			format!(
				"cannot preserve permissions while replacing `{}`",
				path.display()
			)
		})?;
	}
	fs::rename(&temporary, path).with_context(|| format!("cannot replace `{}`", path.display()))
}
