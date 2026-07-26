use std::path::{Path, PathBuf};

use anyhow::Context;

#[derive(Debug)]
pub(super) struct Mutation {
	root: PathBuf,
	path: PathBuf,
	previous: Option<Vec<u8>>,
	previous_mode: Option<u32>,
	committed: Option<Vec<u8>>,
	committed_mode: Option<u32>,
	parent_existed: bool,
}

pub(super) struct Snapshot {
	root: PathBuf,
	path: PathBuf,
	contents: Option<Vec<u8>>,
	mode: Option<u32>,
	parent_existed: bool,
}

impl Snapshot {
	pub(super) fn contents(&self) -> Option<&[u8]> {
		self.contents.as_deref()
	}
}

impl Mutation {
	pub(super) fn committed_contents(&self) -> Option<&[u8]> {
		self.committed.as_deref()
	}

	pub(super) fn created_file(&self) -> bool {
		self.previous.is_none() && self.committed.is_some()
	}

	pub(super) fn created_parent(&self) -> bool {
		!self.parent_existed
	}
}

pub(super) fn read(root: &Path, path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
	Ok(snapshot(root, path)?.contents)
}

pub(super) fn snapshot(root: &Path, path: &Path) -> anyhow::Result<Snapshot> {
	let parent = path
		.parent()
		.with_context(|| format!("configuration path `{}` has no parent", path.display()))?;
	let parent_existed = parent == root || crate::fs_nofollow::directory_exists(root, parent)?;
	let contents = crate::fs_nofollow::read(root, path)?;
	let mode = crate::fs_nofollow::mode(root, path)?;
	Ok(Snapshot {
		root: root.to_path_buf(),
		path: path.to_path_buf(),
		contents,
		mode,
		parent_existed,
	})
}

pub(super) fn write(snapshot: Snapshot, contents: &[u8]) -> anyhow::Result<Mutation> {
	let root = &snapshot.root;
	let path = &snapshot.path;
	let parent = path
		.parent()
		.with_context(|| format!("configuration path `{}` has no parent", path.display()))?;
	if parent != root {
		crate::fs_nofollow::ensure_dir(root, parent)?;
	}
	run_before_write(path);
	let committed_mode = snapshot.mode.unwrap_or(0o600);
	crate::fs_nofollow::write_if_unchanged(
		root,
		path,
		snapshot.contents.as_deref(),
		snapshot.mode,
		contents,
		Some(committed_mode),
	)?;
	Ok(Mutation {
		root: snapshot.root,
		path: snapshot.path,
		previous: snapshot.contents,
		previous_mode: snapshot.mode,
		committed: Some(contents.to_vec()),
		committed_mode: Some(committed_mode),
		parent_existed: snapshot.parent_existed,
	})
}

pub(super) fn remove(snapshot: Snapshot, remove_parent: bool) -> anyhow::Result<Mutation> {
	if let Some(contents) = &snapshot.contents {
		crate::fs_nofollow::remove_if_unchanged(
			&snapshot.root,
			&snapshot.path,
			contents,
			snapshot.mode,
		)?;
	}
	if remove_parent
		&& let Some(parent) = snapshot.path.parent()
		&& parent != snapshot.root
	{
		crate::fs_nofollow::remove_dir(&snapshot.root, parent).ok();
	}
	Ok(Mutation {
		root: snapshot.root,
		path: snapshot.path,
		previous: snapshot.contents,
		previous_mode: snapshot.mode,
		committed: None,
		committed_mode: None,
		parent_existed: snapshot.parent_existed,
	})
}

pub(super) fn rollback(mutation: &Mutation) -> anyhow::Result<()> {
	match (&mutation.previous, &mutation.committed) {
		(Some(previous), Some(committed)) => crate::fs_nofollow::write_if_unchanged(
			&mutation.root,
			&mutation.path,
			Some(committed),
			mutation.committed_mode,
			previous,
			mutation.previous_mode,
		)?,
		(Some(previous), None) => {
			if let Some(parent) = mutation.path.parent()
				&& parent != mutation.root
			{
				crate::fs_nofollow::ensure_dir(&mutation.root, parent)?;
			}
			crate::fs_nofollow::write_if_unchanged(
				&mutation.root,
				&mutation.path,
				None,
				None,
				previous,
				mutation.previous_mode,
			)?;
		}
		(None, Some(committed)) => {
			crate::fs_nofollow::remove_if_unchanged(
				&mutation.root,
				&mutation.path,
				committed,
				mutation.committed_mode,
			)?;
			if !mutation.parent_existed
				&& let Some(parent) = mutation.path.parent()
			{
				crate::fs_nofollow::remove_dir(&mutation.root, parent).ok();
			}
		}
		(None, None) => {}
	}
	Ok(())
}

#[cfg(test)]
type BeforeWriteHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
	pub(super) static BEFORE_WRITE:
		std::cell::RefCell<Option<BeforeWriteHook>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
fn run_before_write(path: &Path) {
	BEFORE_WRITE.with(|hook| {
		if let Some(hook) = hook.borrow_mut().take() {
			hook(path);
		}
	});
}

#[cfg(not(test))]
fn run_before_write(_path: &Path) {}
