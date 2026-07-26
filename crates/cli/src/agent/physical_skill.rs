use std::fs;
use std::path::Path;

use anyhow::{Context, bail};

use super::SKILL_FILES;

pub(super) struct Mutation {
	path: std::path::PathBuf,
	previous: Snapshot,
	committed: Snapshot,
}

#[derive(Clone)]
struct Snapshot {
	directory_exists: bool,
	assets: Vec<AssetSnapshot>,
}

#[derive(Clone)]
struct AssetSnapshot {
	relative: &'static str,
	contents: Option<Vec<u8>>,
	mode: Option<u32>,
	observed: bool,
}

pub(super) fn matches(home: &Path, path: &Path) -> bool {
	SKILL_FILES.iter().all(|(relative, contents)| {
		crate::fs_nofollow::read(home, &path.join(relative))
			.ok()
			.flatten()
			.is_some_and(|actual| actual == contents.as_bytes())
	})
}

pub(super) fn exists(home: &Path, path: &Path) -> anyhow::Result<bool> {
	crate::fs_nofollow::directory_exists(home, path)
}

pub(super) fn ensure_parent(home: &Path, path: &Path) -> anyhow::Result<()> {
	let parent = path
		.parent()
		.with_context(|| format!("skill path `{}` has no parent", path.display()))?;
	fs::create_dir_all(home)
		.with_context(|| format!("cannot create integration home `{}`", home.display()))?;
	crate::fs_nofollow::ensure_dir(home, parent)
}

pub(super) fn ensure_directory(home: &Path, path: &Path) -> anyhow::Result<()> {
	crate::fs_nofollow::ensure_dir(home, path)
}

pub(super) fn write_assets(home: &Path, path: &Path) -> anyhow::Result<Mutation> {
	ensure_assets(path)?;
	let previous = snapshot(home, path)?;
	let mut changed = Vec::new();
	let result = write_assets_inner(home, path, &previous, &mut changed);
	let committed = committed_after_write(&previous, &changed);
	if let Err(error) = result {
		let mutation = Mutation {
			path: path.to_path_buf(),
			previous,
			committed,
		};
		if let Err(rollback) = rollback(home, &mutation) {
			bail!("{error:#}; additionally failed to roll back the skill write: {rollback:#}");
		}
		return Err(error);
	}
	Ok(Mutation {
		path: path.to_path_buf(),
		previous,
		committed,
	})
}

fn write_assets_inner(
	home: &Path,
	path: &Path,
	previous: &Snapshot,
	changed: &mut Vec<&'static str>,
) -> anyhow::Result<()> {
	for (relative, _) in SKILL_FILES {
		let asset = path.join(relative);
		let parent = asset
			.parent()
			.with_context(|| format!("skill asset `{}` has no parent", asset.display()))?;
		crate::fs_nofollow::ensure_dir(home, parent)?;
	}
	ensure_assets(path)?;
	for ((relative, contents), asset) in SKILL_FILES.iter().zip(&previous.assets) {
		run_before_asset_mutation(&path.join(relative));
		crate::fs_nofollow::write_if_unchanged(
			home,
			&path.join(relative),
			asset.contents.as_deref(),
			asset.mode,
			contents.as_bytes(),
			Some(asset.mode.unwrap_or(0o600)),
		)?;
		changed.push(relative);
	}
	Ok(())
}

pub(super) fn remove(home: &Path, path: &Path) -> anyhow::Result<Mutation> {
	let previous = snapshot(home, path)?;
	let mut removed = Vec::new();
	let result = (|| -> anyhow::Result<()> {
		for asset in previous.assets.iter().rev() {
			if let Some(contents) = &asset.contents {
				run_before_asset_mutation(&path.join(asset.relative));
				crate::fs_nofollow::remove_if_unchanged(
					home,
					&path.join(asset.relative),
					contents,
					asset.mode,
				)?;
				removed.push(asset.relative);
			}
		}
		Ok(())
	})();
	if let Err(error) = result {
		let committed = committed_after_remove(&previous, &removed, previous.directory_exists);
		let mutation = Mutation {
			path: path.to_path_buf(),
			previous,
			committed,
		};
		if let Err(rollback) = rollback(home, &mutation) {
			bail!("{error:#}; additionally failed to roll back the skill removal: {rollback:#}");
		}
		return Err(error);
	}
	let mut directories = SKILL_FILES
		.iter()
		.filter_map(|(relative, _)| Path::new(relative).parent())
		.filter(|parent| !parent.as_os_str().is_empty())
		.map(|parent| path.join(parent))
		.collect::<Vec<_>>();
	directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
	directories.dedup();
	for directory in directories {
		crate::fs_nofollow::remove_dir(home, &directory).ok();
	}
	crate::fs_nofollow::remove_dir(home, path).ok();
	let committed = committed_after_remove(&previous, &removed, false);
	Ok(Mutation {
		path: path.to_path_buf(),
		previous,
		committed,
	})
}

pub(super) fn rollback(home: &Path, mutation: &Mutation) -> anyhow::Result<()> {
	if mutation.previous.directory_exists {
		ensure_directory(home, &mutation.path)?;
	}
	for (previous, committed) in mutation
		.previous
		.assets
		.iter()
		.zip(&mutation.committed.assets)
	{
		if !committed.observed {
			continue;
		}
		let path = mutation.path.join(previous.relative);
		match (&previous.contents, &committed.contents) {
			(Some(previous_contents), Some(committed_contents)) => {
				let parent = path
					.parent()
					.with_context(|| format!("skill asset `{}` has no parent", path.display()))?;
				crate::fs_nofollow::ensure_dir(home, parent)?;
				crate::fs_nofollow::write_if_unchanged(
					home,
					&path,
					Some(committed_contents),
					committed.mode,
					previous_contents,
					previous.mode,
				)?;
			}
			(Some(previous_contents), None) => {
				let parent = path
					.parent()
					.with_context(|| format!("skill asset `{}` has no parent", path.display()))?;
				crate::fs_nofollow::ensure_dir(home, parent)?;
				crate::fs_nofollow::write_if_unchanged(
					home,
					&path,
					None,
					None,
					previous_contents,
					previous.mode,
				)?;
			}
			(None, Some(committed_contents)) => {
				crate::fs_nofollow::remove_if_unchanged(
					home,
					&path,
					committed_contents,
					committed.mode,
				)?;
			}
			(None, None) => {}
		}
	}
	if !mutation.previous.directory_exists {
		remove_empty_skill_directories(home, &mutation.path);
	}
	Ok(())
}

fn snapshot(home: &Path, path: &Path) -> anyhow::Result<Snapshot> {
	let directory_exists = crate::fs_nofollow::directory_exists(home, path)?;
	let mut assets = Vec::with_capacity(SKILL_FILES.len());
	for (relative, _) in SKILL_FILES {
		let asset = path.join(relative);
		assets.push(AssetSnapshot {
			relative,
			contents: crate::fs_nofollow::read(home, &asset)?,
			mode: crate::fs_nofollow::mode(home, &asset)?,
			observed: true,
		});
	}
	Ok(Snapshot {
		directory_exists,
		assets,
	})
}

fn committed_after_write(previous: &Snapshot, changed: &[&str]) -> Snapshot {
	let assets = previous
		.assets
		.iter()
		.map(|asset| {
			let Some((_, contents)) = SKILL_FILES
				.iter()
				.find(|(relative, _)| *relative == asset.relative)
				.filter(|(relative, _)| changed.contains(relative))
			else {
				let mut unchanged = asset.clone();
				unchanged.observed = false;
				return unchanged;
			};
			AssetSnapshot {
				relative: asset.relative,
				contents: Some(contents.as_bytes().to_vec()),
				mode: Some(asset.mode.unwrap_or(0o600)),
				observed: true,
			}
		})
		.collect();
	Snapshot {
		directory_exists: true,
		assets,
	}
}

fn committed_after_remove(
	previous: &Snapshot,
	removed: &[&str],
	directory_exists: bool,
) -> Snapshot {
	let assets = previous
		.assets
		.iter()
		.map(|asset| {
			let mut committed = asset.clone();
			committed.observed = removed.contains(&asset.relative);
			if committed.observed {
				committed.contents = None;
				committed.mode = None;
			}
			committed
		})
		.collect();
	Snapshot {
		directory_exists,
		assets,
	}
}

fn remove_empty_skill_directories(home: &Path, path: &Path) {
	let mut directories = SKILL_FILES
		.iter()
		.filter_map(|(relative, _)| Path::new(relative).parent())
		.filter(|parent| !parent.as_os_str().is_empty())
		.map(|parent| path.join(parent))
		.collect::<Vec<_>>();
	directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
	directories.dedup();
	for directory in directories {
		crate::fs_nofollow::remove_dir(home, &directory).ok();
	}
	crate::fs_nofollow::remove_dir(home, path).ok();
}

fn ensure_assets(path: &Path) -> anyhow::Result<()> {
	for (relative, _) in SKILL_FILES {
		let mut current = path.to_path_buf();
		for component in Path::new(relative).components() {
			current.push(component.as_os_str());
			match fs::symlink_metadata(&current) {
				Ok(metadata) if metadata.file_type().is_symlink() => {
					bail!(
						"refusing linked skill asset `{}`; replace it with a physical path",
						current.display()
					);
				}
				Ok(_) => {}
				Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
				Err(error) => {
					return Err(error)
						.with_context(|| format!("cannot inspect `{}`", current.display()));
				}
			}
		}
	}
	Ok(())
}

#[cfg(test)]
type AssetMutationHook = Box<dyn FnMut(&Path)>;

#[cfg(test)]
thread_local! {
	pub(super) static BEFORE_ASSET_MUTATION:
		std::cell::RefCell<Option<AssetMutationHook>> = std::cell::RefCell::new(None);
}

#[cfg(test)]
fn run_before_asset_mutation(path: &Path) {
	BEFORE_ASSET_MUTATION.with(|hook| {
		if let Some(hook) = hook.borrow_mut().as_mut() {
			hook(path);
		}
	});
}

#[cfg(not(test))]
fn run_before_asset_mutation(_path: &Path) {}
