use std::path::Path;

#[cfg(unix)]
mod imp {
	use std::ffi::{CStr, CString, OsStr};
	use std::fs::File;
	use std::io::{Read, Write};
	use std::os::fd::{AsRawFd, FromRawFd};
	use std::os::unix::ffi::OsStrExt;
	use std::path::{Component, Path};
	use std::sync::atomic::{AtomicU64, Ordering};

	use anyhow::{Context, bail};

	static TEMPORARY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

	#[derive(Clone, Copy, Eq, PartialEq)]
	struct FileIdentity {
		device: libc::dev_t,
		inode: libc::ino_t,
	}

	struct RestoreFallback<'a> {
		contents: &'a [u8],
		mode: Option<u32>,
	}

	struct RestoreCandidate {
		identity: FileIdentity,
		_file: File,
	}

	struct PhysicalEntry<'a> {
		parent: &'a File,
		name: &'a CStr,
		path: &'a Path,
	}

	pub(super) struct ExclusiveLock {
		_file: File,
	}

	impl FileIdentity {
		fn from_file(file: &File, path: &Path) -> anyhow::Result<Self> {
			let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
			let result = unsafe { libc::fstat(file.as_raw_fd(), stat.as_mut_ptr()) };
			if result == -1 {
				return Err(std::io::Error::last_os_error())
					.with_context(|| format!("cannot inspect physical file `{}`", path.display()));
			}
			let stat = unsafe { stat.assume_init() };
			if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
				bail!("`{}` is not a regular physical file", path.display());
			}
			Ok(Self {
				device: stat.st_dev,
				inode: stat.st_ino,
			})
		}

		fn regular_at(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<Option<Self>> {
			let Some(stat) = stat_at(parent, name, path)? else {
				return Ok(None);
			};
			if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
				return Ok(None);
			}
			Ok(Some(Self {
				device: stat.st_dev,
				inode: stat.st_ino,
			}))
		}
	}

	impl RestoreCandidate {
		fn open(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<Self> {
			let fd = unsafe {
				libc::openat(
					parent.as_raw_fd(),
					name.as_ptr(),
					libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
				)
			};
			if fd == -1 {
				return Err(std::io::Error::last_os_error())
					.with_context(|| format!("cannot open physical file `{}`", path.display()));
			}
			Self::from_file(unsafe { File::from_raw_fd(fd) }, path)
		}

		fn from_file(file: File, path: &Path) -> anyhow::Result<Self> {
			Ok(Self {
				identity: FileIdentity::from_file(&file, path)?,
				_file: file,
			})
		}

		fn mode(&self, path: &Path) -> anyhow::Result<u32> {
			use std::os::unix::fs::PermissionsExt;

			let metadata = self
				._file
				.metadata()
				.with_context(|| format!("cannot inspect physical file `{}`", path.display()))?;
			if !metadata.is_file() {
				bail!("`{}` is not a regular physical file", path.display());
			}
			Ok(metadata.permissions().mode() & 0o7777)
		}

		fn read_contents(&mut self, path: &Path) -> anyhow::Result<Vec<u8>> {
			let mut contents = Vec::new();
			self._file.read_to_end(&mut contents).with_context(|| {
				format!("cannot read physical candidate for `{}`", path.display())
			})?;
			Ok(contents)
		}
	}

	impl<'a> PhysicalEntry<'a> {
		fn new(parent: &'a File, name: &'a CStr, path: &'a Path) -> Self {
			Self { parent, name, path }
		}

		fn commit(
			&self,
			temporary_name: &CStr,
			temporary_identity: FileIdentity,
			exchange: bool,
		) -> anyhow::Result<()> {
			exchange_rollback::run_before_mutation_hook();
			if identity_at_optional(self.parent, temporary_name, self.path)?
				!= Some(temporary_identity)
			{
				bail!(
					"temporary file for `{}` changed concurrently; preserving the replacement",
					self.path.display()
				);
			}
			let commit = if exchange {
				atomic_rename::exchange(self.parent, temporary_name, self.name)
			} else {
				atomic_rename::noreplace(self.parent, temporary_name, self.name)
			};
			if let Err(error) = commit {
				PhysicalEntry::new(self.parent, temporary_name, self.path)
					.remove_identity(temporary_identity)
					.ok();
				if matches!(
					error.raw_os_error(),
					Some(code) if code == libc::EEXIST || code == libc::ENOENT
				) {
					bail!(
						"managed file `{}` changed concurrently; refusing to overwrite it",
						self.path.display()
					);
				}
				return Err(error).with_context(|| {
					format!("cannot conditionally replace `{}`", self.path.display())
				});
			}
			Ok(())
		}

		fn remove_identity(&self, expected: FileIdentity) -> anyhow::Result<()> {
			for _ in 0..32 {
				let quarantine = vacant_temporary_name(self.parent, self.name, self.path)?;
				match atomic_rename::noreplace(self.parent, self.name, &quarantine) {
					Ok(()) => {}
					Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(()),
					Err(error) => {
						return Err(error).with_context(|| {
							format!(
								"cannot quarantine concurrent entry for `{}`",
								self.path.display()
							)
						});
					}
				}
				let moved = identity_at(self.parent, &quarantine, self.path)?;
				if moved == expected {
					if identity_at_optional(self.parent, &quarantine, self.path)? == Some(expected)
					{
						unlink_at(self.parent, &quarantine)?;
						return Ok(());
					}
					continue;
				}
				match atomic_rename::noreplace(self.parent, &quarantine, self.name) {
					Ok(()) => {}
					Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
						bail!(
							"managed file `{}` changed repeatedly during quarantine",
							self.path.display()
						);
					}
					Err(error) => {
						return Err(error).with_context(|| {
							format!(
								"cannot restore concurrent entry for `{}`",
								self.path.display()
							)
						});
					}
				}
			}
			bail!(
				"cannot stabilize `{}` after repeated concurrent replacements",
				self.path.display()
			)
		}
	}

	pub(super) fn ensure_dir(root: &Path, directory: &Path) -> anyhow::Result<()> {
		let relative = relative_components(root, directory)?;
		let mut current = open_root(root)?;
		for component in relative {
			let name = component_name(component)?;
			current = match open_directory_at(&current, &name) {
				Ok(directory) => directory,
				Err(error) if error.raw_os_error() == Some(libc::ENOENT) => {
					mkdir_at(&current, &name)?;
					open_directory_at(&current, &name).with_context(|| {
						format!(
							"cannot open newly created physical directory below `{}`",
							root.display()
						)
					})?
				}
				Err(error) => {
					return Err(error).with_context(|| {
						format!(
							"refusing non-physical directory component below `{}`",
							root.display()
						)
					});
				}
			};
		}
		Ok(())
	}

	pub(super) fn lock_exclusive(root: &Path, path: &Path) -> anyhow::Result<ExclusiveLock> {
		let parent_path = path
			.parent()
			.with_context(|| format!("lock path `{}` has no parent", path.display()))?;
		ensure_dir(root, parent_path)?;
		let (parent, name) = open_parent(root, path)?;
		let fd = unsafe {
			libc::openat(
				parent.as_raw_fd(),
				name.as_ptr(),
				libc::O_RDWR
					| libc::O_CREAT | libc::O_CLOEXEC
					| libc::O_NOFOLLOW
					| libc::O_NONBLOCK,
				0o600,
			)
		};
		if fd == -1 {
			return Err(std::io::Error::last_os_error())
				.with_context(|| format!("cannot open physical lock `{}`", path.display()));
		}
		let file = unsafe { File::from_raw_fd(fd) };
		FileIdentity::from_file(&file, path)?;
		if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == -1 {
			return Err(std::io::Error::last_os_error())
				.with_context(|| format!("cannot lock `{}`", path.display()));
		}
		Ok(ExclusiveLock { _file: file })
	}

	pub(super) fn read(root: &Path, path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			return Ok(None);
		};
		read_at(&parent, &name, path)
	}

	fn read_at(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
		let fd = unsafe {
			libc::openat(
				parent.as_raw_fd(),
				name.as_ptr(),
				libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
			)
		};
		if fd == -1 {
			let error = std::io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ENOENT) {
				return Ok(None);
			}
			return Err(error)
				.with_context(|| format!("cannot read physical file `{}`", path.display()));
		}
		let mut file = unsafe { File::from_raw_fd(fd) };
		let metadata = file
			.metadata()
			.with_context(|| format!("cannot inspect physical file `{}`", path.display()))?;
		if !metadata.is_file() {
			bail!("`{}` is not a regular physical file", path.display());
		}
		let mut contents = Vec::new();
		file.read_to_end(&mut contents)
			.with_context(|| format!("cannot read physical file `{}`", path.display()))?;
		Ok(Some(contents))
	}

	pub(super) fn exists(root: &Path, path: &Path) -> anyhow::Result<bool> {
		Ok(mode(root, path)?.is_some())
	}

	pub(super) fn directory_exists(root: &Path, path: &Path) -> anyhow::Result<bool> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			return Ok(false);
		};
		let Some(stat) = stat_at(&parent, &name, path)? else {
			return Ok(false);
		};
		if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
			bail!("`{}` is not a physical directory", path.display());
		}
		Ok(true)
	}

	pub(super) fn mode(root: &Path, path: &Path) -> anyhow::Result<Option<u32>> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			return Ok(None);
		};
		let Some(stat) = stat_at(&parent, &name, path)? else {
			return Ok(None);
		};
		if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
			bail!("`{}` is not a regular physical file", path.display());
		}
		Ok(Some(permission_bits(stat.st_mode)))
	}

	#[cfg(test)]
	pub(super) fn write(
		root: &Path,
		path: &Path,
		contents: &[u8],
		exact_mode: Option<u32>,
	) -> anyhow::Result<()> {
		let previous = read(root, path)?;
		let previous_mode = mode(root, path)?;
		write_if_unchanged(
			root,
			path,
			previous.as_deref(),
			previous_mode,
			contents,
			exact_mode,
		)
	}

	pub(super) fn write_if_unchanged(
		root: &Path,
		path: &Path,
		expected: Option<&[u8]>,
		expected_mode: Option<u32>,
		contents: &[u8],
		exact_mode: Option<u32>,
	) -> anyhow::Result<()> {
		let (parent, name) = open_parent(root, path)?;
		let existing_mode = stat_at(&parent, &name, path)?
			.map(|stat| {
				if stat.st_mode & libc::S_IFMT != libc::S_IFREG {
					bail!("`{}` is not a regular physical file", path.display());
				}
				Ok(permission_bits(stat.st_mode))
			})
			.transpose()?;
		let (temporary, temporary_name) = create_temporary(&parent, &name, path)?;
		let temporary =
			match write_temporary(temporary, contents, exact_mode.or(existing_mode), path) {
				Ok(temporary) => temporary,
				Err(error) => {
					unlink_at(&parent, &temporary_name).ok();
					return Err(error);
				}
			};
		let inserted_identity = FileIdentity::from_file(&temporary, path)?;

		let managed = PhysicalEntry::new(&parent, &name, path);
		managed.commit(&temporary_name, inserted_identity, expected.is_some())?;

		let installed_matches =
			FileIdentity::regular_at(&parent, &name, path)? == Some(inserted_identity);
		let Some(expected) = expected else {
			if installed_matches {
				return Ok(());
			}
			bail!(
				"managed file `{}` changed concurrently after creation; preserving the replacement",
				path.display()
			);
		};
		if !installed_matches {
			bail!(
				"managed file `{}` changed concurrently after replacement; backup retained",
				path.display()
			);
		}
		let observed = (|| -> anyhow::Result<(RestoreCandidate, Vec<u8>, u32)> {
			let mut candidate = RestoreCandidate::open(&parent, &temporary_name, path)?;
			let mode = candidate.mode(path)?;
			let contents = candidate.read_contents(path)?;
			if identity_at_optional(&parent, &temporary_name, path)? != Some(candidate.identity) {
				bail!(
					"replacement backup for `{}` changed concurrently",
					path.display()
				);
			}
			Ok((candidate, contents, mode))
		})();
		match observed {
			Ok((candidate, displaced, displaced_mode))
				if displaced == expected
					&& expected_mode.is_none_or(|mode| displaced_mode == mode) =>
			{
				PhysicalEntry::new(&parent, &temporary_name, path)
					.remove_identity(candidate.identity)?;
				Ok(())
			}
			Ok((_candidate, displaced, displaced_mode)) => {
				exchange_rollback::restore(
					&parent,
					&name,
					&temporary_name,
					inserted_identity,
					RestoreFallback {
						contents: &displaced,
						mode: Some(displaced_mode),
					},
					path,
				)?;
				bail!(
					"managed file `{}` changed concurrently; refusing to overwrite it",
					path.display()
				);
			}
			Err(error) => {
				exchange_rollback::restore(
					&parent,
					&name,
					&temporary_name,
					inserted_identity,
					RestoreFallback {
						contents: expected,
						mode: existing_mode,
					},
					path,
				)?;
				Err(error).context("concurrent managed file was not a regular physical file")
			}
		}
	}

	#[allow(clippy::useless_conversion)]
	fn permission_bits(mode: libc::mode_t) -> u32 {
		// mode_t is u16 on macOS and u32 on Linux.
		u32::from(mode & 0o7777)
	}

	pub(super) fn remove_if_unchanged(
		root: &Path,
		path: &Path,
		expected: &[u8],
		expected_mode: Option<u32>,
	) -> anyhow::Result<bool> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			bail!(
				"managed file `{}` changed concurrently: expected a regular file, found it missing",
				path.display()
			);
		};
		let temporary_name = vacant_temporary_name(&parent, &name, path)?;
		if let Err(error) = atomic_rename::noreplace(&parent, &name, &temporary_name) {
			if matches!(
				error.raw_os_error(),
				Some(code) if code == libc::EEXIST || code == libc::ENOENT
			) {
				bail!(
					"managed file `{}` changed concurrently; refusing to remove it",
					path.display()
				);
			}
			return Err(error)
				.with_context(|| format!("cannot conditionally remove `{}`", path.display()));
		}
		exchange_rollback::run_before_mutation_hook();
		let observed = (|| -> anyhow::Result<(RestoreCandidate, Vec<u8>, u32)> {
			let mut candidate = RestoreCandidate::open(&parent, &temporary_name, path)?;
			let mode = candidate.mode(path)?;
			let contents = candidate.read_contents(path)?;
			if identity_at_optional(&parent, &temporary_name, path)? != Some(candidate.identity) {
				bail!(
					"conditional remove candidate for `{}` changed concurrently",
					path.display()
				);
			}
			Ok((candidate, contents, mode))
		})();
		match observed {
			Ok((candidate, displaced, displaced_mode))
				if displaced == expected
					&& expected_mode.is_none_or(|mode| displaced_mode == mode) =>
			{
				PhysicalEntry::new(&parent, &temporary_name, path)
					.remove_identity(candidate.identity)?;
				Ok(true)
			}
			Ok((_candidate, displaced, displaced_mode)) => {
				exchange_rollback::restore_missing(
					&parent,
					&name,
					&temporary_name,
					RestoreFallback {
						contents: &displaced,
						mode: Some(displaced_mode),
					},
					path,
				)?;
				bail!(
					"managed file `{}` changed concurrently; refusing to remove it",
					path.display()
				);
			}
			Err(error) => {
				exchange_rollback::restore_missing(
					&parent,
					&name,
					&temporary_name,
					RestoreFallback {
						contents: expected,
						mode: expected_mode,
					},
					path,
				)?;
				Err(error).context("concurrent managed file was not a regular physical file")
			}
		}
	}

	#[cfg(test)]
	pub(super) fn remove(root: &Path, path: &Path) -> anyhow::Result<bool> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			return Ok(false);
		};
		if stat_at(&parent, &name, path)?.is_none() {
			return Ok(false);
		}
		unlink_at(&parent, &name)
			.with_context(|| format!("cannot remove physical file `{}`", path.display()))?;
		Ok(true)
	}

	mod exchange_rollback {
		use super::*;

		pub(super) fn restore(
			parent: &File,
			name: &CStr,
			temporary_name: &CStr,
			inserted: FileIdentity,
			fallback: RestoreFallback<'_>,
			path: &Path,
		) -> anyhow::Result<()> {
			for _ in 0..32 {
				let Some(restoring) = candidate(
					parent,
					temporary_name,
					fallback.contents,
					fallback.mode,
					path,
				)?
				else {
					continue;
				};
				run_before_mutation_hook();
				if identity_at_optional(parent, temporary_name, path)? != Some(restoring.identity) {
					bail!(
						"rollback candidate for `{}` changed concurrently; preserving it",
						path.display()
					);
				}
				atomic_rename::exchange(parent, temporary_name, name).with_context(|| {
					format!(
						"cannot restore `{}` after a conditional replacement conflict",
						path.display()
					)
				})?;
				let displaced = identity_at(parent, temporary_name, path)?;
				if FileIdentity::regular_at(parent, name, path)? != Some(restoring.identity) {
					bail!(
						"managed file `{}` changed concurrently after rollback exchange; preserving all replacements",
						path.display()
					);
				}
				if displaced == inserted {
					PhysicalEntry::new(parent, temporary_name, path).remove_identity(displaced)?;
					return Ok(());
				}
				if identity_at_optional(parent, name, path)? == Some(restoring.identity)
					&& identity_at_optional(parent, temporary_name, path)? == Some(displaced)
				{
					atomic_rename::exchange(parent, temporary_name, name).with_context(|| {
						format!(
							"cannot restore concurrent public replacement for `{}`",
							path.display()
						)
					})?;
				}
				bail!(
					"managed file `{}` changed concurrently before rollback; replacement preserved",
					path.display()
				);
			}
			bail!(
				"cannot stabilize `{}` after repeated concurrent replacements",
				path.display()
			)
		}

		fn candidate(
			parent: &File,
			temporary_name: &CStr,
			fallback_contents: &[u8],
			fallback_mode: Option<u32>,
			path: &Path,
		) -> anyhow::Result<Option<RestoreCandidate>> {
			if let Some(stat) = stat_at(parent, temporary_name, path)? {
				match stat.st_mode & libc::S_IFMT {
					libc::S_IFREG => {
						let mut candidate = RestoreCandidate::open(parent, temporary_name, path)?;
						let identity = candidate.identity;
						let mode = candidate.mode(path)?;
						let contents = candidate.read_contents(path)?;
						if contents == fallback_contents
							&& fallback_mode.is_none_or(|expected| mode == expected)
							&& identity_at_optional(parent, temporary_name, path)? == Some(identity)
						{
							return Ok(Some(candidate));
						}
						bail!(
							"rollback candidate for `{}` changed concurrently; preserving it",
							path.display()
						);
					}
					libc::S_IFLNK => {
						bail!(
							"rollback candidate for `{}` became a symlink; preserving it",
							path.display()
						);
					}
					_ => {
						bail!(
							"refusing non-regular rollback candidate for `{}`",
							path.display()
						);
					}
				}
			}
			let fd = unsafe {
				libc::openat(
					parent.as_raw_fd(),
					temporary_name.as_ptr(),
					libc::O_RDWR
						| libc::O_CREAT | libc::O_EXCL
						| libc::O_CLOEXEC | libc::O_NOFOLLOW,
					0o666,
				)
			};
			if fd == -1 {
				let error = std::io::Error::last_os_error();
				if error.raw_os_error() == Some(libc::EEXIST) {
					return Ok(None);
				}
				return Err(error).with_context(|| {
					format!(
						"cannot recreate rollback candidate for `{}`",
						path.display()
					)
				});
			}
			let file = write_temporary(
				unsafe { File::from_raw_fd(fd) },
				fallback_contents,
				fallback_mode,
				path,
			)?;
			let candidate = RestoreCandidate::from_file(file, path)?;
			if identity_at_optional(parent, temporary_name, path)? != Some(candidate.identity) {
				return Ok(None);
			}
			Ok(Some(candidate))
		}

		pub(super) fn restore_missing(
			parent: &File,
			name: &CStr,
			temporary_name: &CStr,
			fallback: RestoreFallback<'_>,
			path: &Path,
		) -> anyhow::Result<()> {
			for _ in 0..32 {
				let Some(candidate) = candidate(
					parent,
					temporary_name,
					fallback.contents,
					fallback.mode,
					path,
				)?
				else {
					continue;
				};
				run_before_mutation_hook();
				if identity_at_optional(parent, temporary_name, path)? != Some(candidate.identity) {
					bail!(
						"rollback candidate for `{}` changed concurrently; preserving it",
						path.display()
					);
				}
				match atomic_rename::noreplace(parent, temporary_name, name) {
					Ok(()) => {}
					Err(error) if error.raw_os_error() == Some(libc::EEXIST) => {
						bail!(
							"managed file `{}` changed concurrently during restoration; backup retained",
							path.display()
						);
					}
					Err(error) => {
						return Err(error).with_context(|| {
							format!("cannot restore conditionally removed `{}`", path.display())
						});
					}
				}
				if FileIdentity::regular_at(parent, name, path)? == Some(candidate.identity) {
					return Ok(());
				}
				bail!(
					"managed file `{}` changed concurrently after restoration; preserving the replacement",
					path.display()
				);
			}
			bail!(
				"cannot stabilize `{}` after repeated concurrent restorations",
				path.display()
			)
		}

		#[cfg(test)]
		thread_local! {
			pub(super) static BEFORE_EXCHANGE: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
				std::cell::RefCell::new(None);
		}

		#[cfg(test)]
		pub(super) fn run_before_mutation_hook() {
			BEFORE_EXCHANGE.with(|hook| {
				if let Some(hook) = hook.borrow_mut().take() {
					hook();
				}
			});
		}

		#[cfg(not(test))]
		pub(super) fn run_before_mutation_hook() {}
	}

	pub(super) fn remove_dir(root: &Path, path: &Path) -> anyhow::Result<bool> {
		let Some((parent, name)) = open_parent_optional(root, path)? else {
			return Ok(false);
		};
		let Some(stat) = stat_at(&parent, &name, path)? else {
			return Ok(false);
		};
		if stat.st_mode & libc::S_IFMT != libc::S_IFDIR {
			bail!("`{}` is not a physical directory", path.display());
		}
		let result =
			unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) };
		if result == -1 {
			let error = std::io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ENOTEMPTY) {
				return Ok(false);
			}
			return Err(error)
				.with_context(|| format!("cannot remove physical directory `{}`", path.display()));
		}
		Ok(true)
	}

	fn open_root(root: &Path) -> anyhow::Result<File> {
		if !root.is_absolute() {
			bail!("trusted root `{}` is not absolute", root.display());
		}
		let root_name = os_string(root.as_os_str())?;
		let fd = unsafe {
			libc::open(
				root_name.as_ptr(),
				libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
			)
		};
		if fd == -1 {
			return Err(std::io::Error::last_os_error()).with_context(|| {
				format!("cannot open physical trusted root `{}`", root.display())
			});
		}
		let root = unsafe { File::from_raw_fd(fd) };
		if !root
			.metadata()
			.context("cannot inspect physical trusted root")?
			.is_dir()
		{
			bail!("trusted root is not a physical directory");
		}
		Ok(root)
	}

	fn open_parent(root: &Path, path: &Path) -> anyhow::Result<(File, CString)> {
		open_parent_optional(root, path)?.with_context(|| {
			format!(
				"physical parent of managed file `{}` does not exist",
				path.display()
			)
		})
	}

	fn open_parent_optional(root: &Path, path: &Path) -> anyhow::Result<Option<(File, CString)>> {
		let relative = relative_components(root, path)?;
		let Some((leaf, parents)) = relative.split_last() else {
			bail!("managed file path `{}` has no file name", path.display());
		};
		let mut current = open_root(root)?;
		for component in parents {
			let name = component_name(*component)?;
			current = match open_directory_at(&current, &name) {
				Ok(directory) => directory,
				Err(error) if error.raw_os_error() == Some(libc::ENOENT) => return Ok(None),
				Err(error) => {
					return Err(error).with_context(|| {
						format!(
							"refusing non-physical parent of managed file `{}`",
							path.display()
						)
					});
				}
			};
		}
		Ok(Some((current, component_name(*leaf)?)))
	}

	fn relative_components<'a>(root: &Path, path: &'a Path) -> anyhow::Result<Vec<Component<'a>>> {
		let relative = path.strip_prefix(root).with_context(|| {
			format!(
				"managed path `{}` is outside trusted root `{}`",
				path.display(),
				root.display()
			)
		})?;
		let mut components = Vec::new();
		for component in relative.components() {
			match component {
				Component::Normal(_) => components.push(component),
				Component::CurDir => {}
				_ => bail!("managed path `{}` is not normalized", path.display()),
			}
		}
		Ok(components)
	}

	fn component_name(component: Component<'_>) -> anyhow::Result<CString> {
		let Component::Normal(name) = component else {
			bail!("managed path contains a non-normal component");
		};
		os_string(name)
	}

	fn os_string(value: &OsStr) -> anyhow::Result<CString> {
		CString::new(value.as_bytes()).context("managed path contains a NUL byte")
	}

	fn open_directory_at(parent: &File, name: &CStr) -> std::io::Result<File> {
		let fd = unsafe {
			libc::openat(
				parent.as_raw_fd(),
				name.as_ptr(),
				libc::O_RDONLY | libc::O_DIRECTORY | libc::O_CLOEXEC | libc::O_NOFOLLOW,
			)
		};
		if fd == -1 {
			return Err(std::io::Error::last_os_error());
		}
		Ok(unsafe { File::from_raw_fd(fd) })
	}

	fn mkdir_at(parent: &File, name: &CStr) -> anyhow::Result<()> {
		let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o777) };
		if result == -1 {
			let error = std::io::Error::last_os_error();
			if error.raw_os_error() != Some(libc::EEXIST) {
				return Err(error).context("cannot create physical managed directory");
			}
		}
		Ok(())
	}

	fn stat_at(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<Option<libc::stat>> {
		let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
		let result = unsafe {
			libc::fstatat(
				parent.as_raw_fd(),
				name.as_ptr(),
				stat.as_mut_ptr(),
				libc::AT_SYMLINK_NOFOLLOW,
			)
		};
		if result == -1 {
			let error = std::io::Error::last_os_error();
			if error.raw_os_error() == Some(libc::ENOENT) {
				return Ok(None);
			}
			return Err(error).with_context(|| format!("cannot inspect `{}`", path.display()));
		}
		Ok(Some(unsafe { stat.assume_init() }))
	}

	fn identity_at(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<FileIdentity> {
		identity_at_optional(parent, name, path)?
			.with_context(|| format!("managed entry `{}` disappeared", path.display()))
	}

	fn identity_at_optional(
		parent: &File,
		name: &CStr,
		path: &Path,
	) -> anyhow::Result<Option<FileIdentity>> {
		let Some(stat) = stat_at(parent, name, path)? else {
			return Ok(None);
		};
		Ok(Some(FileIdentity {
			device: stat.st_dev,
			inode: stat.st_ino,
		}))
	}

	fn create_temporary(
		parent: &File,
		name: &CStr,
		path: &Path,
	) -> anyhow::Result<(File, CString)> {
		for _ in 0..32 {
			let sequence = TEMPORARY_SEQUENCE.fetch_add(1, Ordering::Relaxed);
			let mut bytes = name.to_bytes().to_vec();
			bytes.extend_from_slice(format!(".{}.{}.tmp", std::process::id(), sequence).as_bytes());
			let temporary_name =
				CString::new(bytes).context("temporary file name contains a NUL byte")?;
			let fd = unsafe {
				libc::openat(
					parent.as_raw_fd(),
					temporary_name.as_ptr(),
					libc::O_WRONLY
						| libc::O_CREAT | libc::O_EXCL
						| libc::O_CLOEXEC | libc::O_NOFOLLOW,
					0o666,
				)
			};
			if fd != -1 {
				return Ok((unsafe { File::from_raw_fd(fd) }, temporary_name));
			}
			let error = std::io::Error::last_os_error();
			if error.raw_os_error() != Some(libc::EEXIST) {
				return Err(error).with_context(|| {
					format!("cannot create temporary file for `{}`", path.display())
				});
			}
		}
		bail!("cannot allocate a temporary file for `{}`", path.display())
	}

	fn vacant_temporary_name(parent: &File, name: &CStr, path: &Path) -> anyhow::Result<CString> {
		let (temporary, temporary_name) = create_temporary(parent, name, path)?;
		drop(temporary);
		unlink_at(parent, &temporary_name)?;
		Ok(temporary_name)
	}

	fn write_temporary(
		mut temporary: File,
		contents: &[u8],
		mode: Option<u32>,
		path: &Path,
	) -> anyhow::Result<File> {
		temporary
			.write_all(contents)
			.with_context(|| format!("cannot write temporary file for `{}`", path.display()))?;
		if let Some(mode) = mode {
			let result = unsafe { libc::fchmod(temporary.as_raw_fd(), mode as libc::mode_t) };
			if result == -1 {
				return Err(std::io::Error::last_os_error()).with_context(|| {
					format!(
						"cannot set permissions while replacing `{}`",
						path.display()
					)
				});
			}
		}
		Ok(temporary)
	}

	fn unlink_at(parent: &File, name: &CStr) -> anyhow::Result<()> {
		let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) };
		if result == -1 {
			return Err(std::io::Error::last_os_error()).context("cannot unlink managed file");
		}
		Ok(())
	}

	mod atomic_rename {
		use super::*;

		#[cfg(any(target_os = "linux", target_os = "android"))]
		pub(super) fn exchange(parent: &File, from: &CStr, to: &CStr) -> std::io::Result<()> {
			let result = unsafe {
				libc::syscall(
					libc::SYS_renameat2,
					parent.as_raw_fd(),
					from.as_ptr(),
					parent.as_raw_fd(),
					to.as_ptr(),
					libc::RENAME_EXCHANGE,
				)
			};
			if result == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		}

		#[cfg(target_vendor = "apple")]
		pub(super) fn exchange(parent: &File, from: &CStr, to: &CStr) -> std::io::Result<()> {
			let result = unsafe {
				libc::renameatx_np(
					parent.as_raw_fd(),
					from.as_ptr(),
					parent.as_raw_fd(),
					to.as_ptr(),
					libc::RENAME_SWAP,
				)
			};
			if result == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		}

		#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
		pub(super) fn exchange(_parent: &File, _from: &CStr, _to: &CStr) -> std::io::Result<()> {
			Err(std::io::Error::new(
				std::io::ErrorKind::Unsupported,
				"atomic exchange is unavailable on this Unix platform",
			))
		}

		#[cfg(any(target_os = "linux", target_os = "android"))]
		pub(super) fn noreplace(parent: &File, from: &CStr, to: &CStr) -> std::io::Result<()> {
			let result = unsafe {
				libc::syscall(
					libc::SYS_renameat2,
					parent.as_raw_fd(),
					from.as_ptr(),
					parent.as_raw_fd(),
					to.as_ptr(),
					libc::RENAME_NOREPLACE,
				)
			};
			if result == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		}

		#[cfg(target_vendor = "apple")]
		pub(super) fn noreplace(parent: &File, from: &CStr, to: &CStr) -> std::io::Result<()> {
			let result = unsafe {
				libc::renameatx_np(
					parent.as_raw_fd(),
					from.as_ptr(),
					parent.as_raw_fd(),
					to.as_ptr(),
					libc::RENAME_EXCL,
				)
			};
			if result == -1 {
				return Err(std::io::Error::last_os_error());
			}
			Ok(())
		}

		#[cfg(not(any(target_os = "linux", target_os = "android", target_vendor = "apple")))]
		pub(super) fn noreplace(_parent: &File, _from: &CStr, _to: &CStr) -> std::io::Result<()> {
			Err(std::io::Error::new(
				std::io::ErrorKind::Unsupported,
				"exclusive rename is unavailable on this Unix platform",
			))
		}
	}

	#[cfg(test)]
	mod tests {
		use super::*;
		use std::fs;
		use std::os::unix::fs::symlink;

		fn replace_temporary_with_symlink(root: &Path, managed: &Path, external: &Path) {
			let temporary = fs::read_dir(root)
				.unwrap()
				.map(|entry| entry.unwrap().path())
				.find(|entry| {
					entry != managed
						&& entry != external
						&& entry
							.file_name()
							.and_then(|name| name.to_str())
							.is_some_and(|name| name.ends_with(".tmp"))
				})
				.expect("managed temporary file");
			fs::remove_file(&temporary).unwrap();
			symlink(external, temporary).unwrap();
		}

		#[test]
		fn anchored_operations_refuse_symlinked_parents_and_leaves() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path().join("root");
			let external = temp.path().join("external");
			fs::create_dir_all(&root).unwrap();
			fs::create_dir_all(&external).unwrap();
			fs::write(external.join("target"), "unchanged").unwrap();

			symlink(&external, root.join("linked")).unwrap();
			assert!(ensure_dir(&root, &root.join("linked/child")).is_err());
			assert!(write(&root, &root.join("linked/target"), b"changed", None).is_err());
			assert_eq!(
				fs::read_to_string(external.join("target")).unwrap(),
				"unchanged"
			);

			fs::create_dir(root.join("physical")).unwrap();
			symlink(external.join("target"), root.join("physical/leaf")).unwrap();
			assert!(read(&root, &root.join("physical/leaf")).is_err());
			assert!(mode(&root, &root.join("physical/leaf")).is_err());
			assert!(write(&root, &root.join("physical/leaf"), b"changed", None).is_err());
			assert!(remove(&root, &root.join("physical/leaf")).unwrap());
			assert_eq!(
				fs::read_to_string(external.join("target")).unwrap(),
				"unchanged"
			);
		}

		#[test]
		fn anchored_operations_refuse_a_replaced_root_symlink() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path().join("root");
			let moved_root = temp.path().join("moved-root");
			let external = temp.path().join("external");
			fs::create_dir(&root).unwrap();
			fs::create_dir(&external).unwrap();
			fs::write(external.join("target"), "unchanged").unwrap();
			fs::rename(&root, &moved_root).unwrap();
			symlink(&external, &root).unwrap();

			assert!(write(&root, &root.join("target"), b"changed", None).is_err());
			assert_eq!(
				fs::read_to_string(external.join("target")).unwrap(),
				"unchanged"
			);
		}

		#[test]
		fn exclusive_lock_serializes_independent_openers() {
			use std::sync::mpsc;
			use std::time::Duration;

			let temp = tempfile::tempdir().unwrap();
			let root = temp.path().to_path_buf();
			let lock_path = root.join("locks/agent.lock");
			let first = lock_exclusive(&root, &lock_path).unwrap();
			let (started_tx, started_rx) = mpsc::channel();
			let (acquired_tx, acquired_rx) = mpsc::channel();
			let second_root = root.clone();
			let second_path = lock_path.clone();
			let thread = std::thread::spawn(move || {
				started_tx.send(()).unwrap();
				let _second = lock_exclusive(&second_root, &second_path).unwrap();
				acquired_tx.send(()).unwrap();
			});
			started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
			assert!(acquired_rx.recv_timeout(Duration::from_millis(50)).is_err());

			drop(first);
			acquired_rx.recv_timeout(Duration::from_secs(1)).unwrap();
			thread.join().unwrap();
		}

		#[test]
		fn anchored_write_preserves_mode_and_remove_is_bounded() {
			use std::os::unix::fs::PermissionsExt;

			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			ensure_dir(root, &root.join("nested")).unwrap();
			let path = root.join("nested/file");
			write(root, &path, b"first", Some(0o640)).unwrap();
			write(root, &path, b"second", None).unwrap();

			assert_eq!(read(root, &path).unwrap().unwrap(), b"second");
			assert_eq!(mode(root, &path).unwrap(), Some(0o640));
			assert_eq!(
				fs::metadata(&path).unwrap().permissions().mode() & 0o777,
				0o640
			);
			assert!(exists(root, &path).unwrap());
			assert!(remove(root, &path).unwrap());
			assert!(!exists(root, &path).unwrap());
			assert!(!remove(root, &path).unwrap());
		}

		#[test]
		fn conditional_mutations_refuse_concurrent_content_changes() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("file");
			write(root, &path, b"first", None).unwrap();

			assert!(
				write_if_unchanged(root, &path, Some(b"stale"), None, b"replacement", None)
					.is_err()
			);
			assert_eq!(read(root, &path).unwrap().unwrap(), b"first");
			assert!(remove_if_unchanged(root, &path, b"stale", None).is_err());
			assert_eq!(read(root, &path).unwrap().unwrap(), b"first");
			assert!(
				write_if_unchanged(
					root,
					&path,
					Some(b"first"),
					Some(0o600),
					b"replacement",
					None,
				)
				.is_err()
			);
			assert!(remove_if_unchanged(root, &path, b"first", Some(0o600)).is_err());

			write_if_unchanged(root, &path, Some(b"first"), None, b"second", None).unwrap();
			assert_eq!(read(root, &path).unwrap().unwrap(), b"second");
			assert!(remove_if_unchanged(root, &path, b"second", None).unwrap());
			assert!(!exists(root, &path).unwrap());
		}

		#[test]
		fn conditional_create_rejects_a_substituted_temporary_file() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("managed");
			let external = root.join("external");
			fs::write(&external, "external").unwrap();
			let root_for_race = root.to_path_buf();
			let path_for_race = path.clone();
			let external_for_race = external.clone();
			exchange_rollback::BEFORE_EXCHANGE.with(|hook| {
				*hook.borrow_mut() = Some(Box::new(move || {
					replace_temporary_with_symlink(
						&root_for_race,
						&path_for_race,
						&external_for_race,
					);
				}));
			});

			assert!(write_if_unchanged(root, &path, None, None, b"managed", Some(0o600)).is_err());
			assert!(!path.exists());
			assert!(
				root.read_dir()
					.unwrap()
					.filter_map(Result::ok)
					.any(|entry| {
						entry.path() != external
							&& fs::symlink_metadata(entry.path())
								.is_ok_and(|metadata| metadata.file_type().is_symlink())
					})
			);
			assert_eq!(fs::read_to_string(external).unwrap(), "external");
		}

		#[test]
		fn conditional_update_rejects_a_substituted_temporary_file() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("managed");
			let external = root.join("external");
			write(root, &path, b"original", Some(0o600)).unwrap();
			fs::write(&external, "external").unwrap();
			let root_for_race = root.to_path_buf();
			let path_for_race = path.clone();
			let external_for_race = external.clone();
			exchange_rollback::BEFORE_EXCHANGE.with(|hook| {
				*hook.borrow_mut() = Some(Box::new(move || {
					replace_temporary_with_symlink(
						&root_for_race,
						&path_for_race,
						&external_for_race,
					);
				}));
			});

			assert!(
				write_if_unchanged(
					root,
					&path,
					Some(b"original"),
					Some(0o600),
					b"replacement",
					Some(0o600),
				)
				.is_err()
			);
			assert_eq!(read(root, &path).unwrap().unwrap(), b"original");
			assert_eq!(mode(root, &path).unwrap(), Some(0o600));
			assert!(
				root.read_dir()
					.unwrap()
					.filter_map(Result::ok)
					.any(|entry| {
						entry.path() != external
							&& entry.path() != path
							&& fs::symlink_metadata(entry.path())
								.is_ok_and(|metadata| metadata.file_type().is_symlink())
					})
			);
			assert_eq!(fs::read_to_string(external).unwrap(), "external");
		}

		#[test]
		fn conditional_remove_preserves_a_substituted_backup_symlink() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("managed");
			let external = root.join("external");
			write(root, &path, b"original", Some(0o600)).unwrap();
			fs::write(&external, "external").unwrap();
			let root_for_race = root.to_path_buf();
			let path_for_race = path.clone();
			let external_for_race = external.clone();
			exchange_rollback::BEFORE_EXCHANGE.with(|hook| {
				*hook.borrow_mut() = Some(Box::new(move || {
					replace_temporary_with_symlink(
						&root_for_race,
						&path_for_race,
						&external_for_race,
					);
				}));
			});

			assert!(remove_if_unchanged(root, &path, b"original", Some(0o600)).is_err());
			assert!(!path.exists());
			assert!(
				root.read_dir()
					.unwrap()
					.filter_map(Result::ok)
					.any(|entry| {
						entry.path() != external
							&& fs::symlink_metadata(entry.path())
								.is_ok_and(|metadata| metadata.file_type().is_symlink())
					})
			);
			assert_eq!(fs::read_to_string(external).unwrap(), "external");
		}

		#[test]
		fn missing_exchange_backup_is_recreated_before_rollback() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("file");
			write(root, &path, b"replacement", Some(0o640)).unwrap();
			let parent = open_root(root).unwrap();
			let name = os_string(path.file_name().unwrap()).unwrap();
			let temporary_name = CString::new("missing-backup.tmp").unwrap();
			let inserted = identity_at(&parent, &name, &path).unwrap();

			exchange_rollback::restore(
				&parent,
				&name,
				&temporary_name,
				inserted,
				RestoreFallback {
					contents: b"original",
					mode: Some(0o600),
				},
				&path,
			)
			.unwrap();

			assert_eq!(read(root, &path).unwrap().unwrap(), b"original");
			assert_eq!(mode(root, &path).unwrap(), Some(0o600));
		}

		#[test]
		fn linked_exchange_backup_is_preserved_and_rejected() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("file");
			let external = root.join("external");
			write(root, &path, b"replacement", Some(0o640)).unwrap();
			fs::write(&external, "external").unwrap();
			let parent = open_root(root).unwrap();
			let name = os_string(path.file_name().unwrap()).unwrap();
			let temporary_name = CString::new("linked-backup.tmp").unwrap();
			symlink(&external, root.join("linked-backup.tmp")).unwrap();
			let inserted = identity_at(&parent, &name, &path).unwrap();

			assert!(
				exchange_rollback::restore(
					&parent,
					&name,
					&temporary_name,
					inserted,
					RestoreFallback {
						contents: b"original",
						mode: Some(0o600),
					},
					&path,
				)
				.is_err()
			);

			assert_eq!(read(root, &path).unwrap().unwrap(), b"replacement");
			assert_eq!(fs::read_to_string(&external).unwrap(), "external");
			assert!(
				fs::symlink_metadata(root.join("linked-backup.tmp"))
					.unwrap()
					.file_type()
					.is_symlink()
			);
		}

		#[test]
		fn rollback_rejects_a_candidate_replaced_by_a_symlink_before_exchange() {
			let temp = tempfile::tempdir().unwrap();
			let root = temp.path();
			let path = root.join("file");
			let backup = root.join("rollback.tmp");
			let external = root.join("external");
			write(root, &path, b"replacement", Some(0o755)).unwrap();
			write(root, &backup, b"original", Some(0o600)).unwrap();
			fs::write(&external, "external").unwrap();
			let parent = open_root(root).unwrap();
			let name = os_string(path.file_name().unwrap()).unwrap();
			let temporary_name = os_string(backup.file_name().unwrap()).unwrap();
			let inserted = identity_at(&parent, &name, &path).unwrap();
			let backup_for_race = backup.clone();
			let external_for_race = external.clone();
			exchange_rollback::BEFORE_EXCHANGE.with(|hook| {
				*hook.borrow_mut() = Some(Box::new(move || {
					fs::remove_file(&backup_for_race).unwrap();
					symlink(&external_for_race, &backup_for_race).unwrap();
				}));
			});

			assert!(
				exchange_rollback::restore(
					&parent,
					&name,
					&temporary_name,
					inserted,
					RestoreFallback {
						contents: b"original",
						mode: Some(0o600),
					},
					&path,
				)
				.is_err()
			);

			assert_eq!(read(root, &path).unwrap().unwrap(), b"replacement");
			assert_eq!(mode(root, &path).unwrap(), Some(0o755));
			assert!(
				fs::symlink_metadata(&backup)
					.unwrap()
					.file_type()
					.is_symlink()
			);
			assert_eq!(fs::read_to_string(&external).unwrap(), "external");
		}

		#[test]
		fn reading_a_fifo_fails_without_waiting_for_a_writer() {
			use std::os::unix::ffi::OsStrExt;

			let temp = tempfile::tempdir().unwrap();
			let path = temp.path().join("fifo");
			let path_bytes = CString::new(path.as_os_str().as_bytes()).unwrap();
			assert_eq!(unsafe { libc::mkfifo(path_bytes.as_ptr(), 0o600) }, 0);

			let error = read(temp.path(), &path).unwrap_err().to_string();
			assert!(error.contains("not a regular physical file"));
		}
	}
}

#[cfg(not(unix))]
mod imp {
	use std::path::Path;

	use anyhow::bail;

	pub(super) struct ExclusiveLock;

	fn unsupported<T>() -> anyhow::Result<T> {
		bail!(
			"physical agent integration requires macOS or Linux atomic no-follow filesystem operations"
		)
	}

	pub(super) fn ensure_dir(_root: &Path, _directory: &Path) -> anyhow::Result<()> {
		unsupported()
	}

	pub(super) fn lock_exclusive(_root: &Path, _path: &Path) -> anyhow::Result<ExclusiveLock> {
		unsupported()
	}

	pub(super) fn read(_root: &Path, _path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
		unsupported()
	}

	pub(super) fn exists(_root: &Path, _path: &Path) -> anyhow::Result<bool> {
		unsupported()
	}

	pub(super) fn directory_exists(_root: &Path, _path: &Path) -> anyhow::Result<bool> {
		unsupported()
	}

	pub(super) fn mode(_root: &Path, _path: &Path) -> anyhow::Result<Option<u32>> {
		unsupported()
	}

	#[cfg(test)]
	#[allow(dead_code)]
	pub(super) fn write(
		_root: &Path,
		_path: &Path,
		_contents: &[u8],
		_exact_mode: Option<u32>,
	) -> anyhow::Result<()> {
		unsupported()
	}

	pub(super) fn write_if_unchanged(
		_root: &Path,
		_path: &Path,
		_expected: Option<&[u8]>,
		_expected_mode: Option<u32>,
		_contents: &[u8],
		_exact_mode: Option<u32>,
	) -> anyhow::Result<()> {
		unsupported()
	}

	pub(super) fn remove_if_unchanged(
		_root: &Path,
		_path: &Path,
		_expected: &[u8],
		_expected_mode: Option<u32>,
	) -> anyhow::Result<bool> {
		unsupported()
	}

	#[cfg(test)]
	#[allow(dead_code)]
	pub(super) fn remove(_root: &Path, _path: &Path) -> anyhow::Result<bool> {
		unsupported()
	}

	pub(super) fn remove_dir(_root: &Path, _path: &Path) -> anyhow::Result<bool> {
		unsupported()
	}
}

pub(crate) struct ExclusiveLock(#[allow(dead_code)] imp::ExclusiveLock);

pub(crate) fn lock_exclusive(root: &Path, path: &Path) -> anyhow::Result<ExclusiveLock> {
	imp::lock_exclusive(root, path).map(ExclusiveLock)
}

pub(crate) fn ensure_dir(root: &Path, directory: &Path) -> anyhow::Result<()> {
	imp::ensure_dir(root, directory)
}

pub(crate) fn read(root: &Path, path: &Path) -> anyhow::Result<Option<Vec<u8>>> {
	imp::read(root, path)
}

pub(crate) fn exists(root: &Path, path: &Path) -> anyhow::Result<bool> {
	imp::exists(root, path)
}

pub(crate) fn directory_exists(root: &Path, path: &Path) -> anyhow::Result<bool> {
	imp::directory_exists(root, path)
}

pub(crate) fn mode(root: &Path, path: &Path) -> anyhow::Result<Option<u32>> {
	imp::mode(root, path)
}

pub(crate) fn write_if_unchanged(
	root: &Path,
	path: &Path,
	expected: Option<&[u8]>,
	expected_mode: Option<u32>,
	contents: &[u8],
	exact_mode: Option<u32>,
) -> anyhow::Result<()> {
	imp::write_if_unchanged(root, path, expected, expected_mode, contents, exact_mode)
}

pub(crate) fn remove_if_unchanged(
	root: &Path,
	path: &Path,
	expected: &[u8],
	expected_mode: Option<u32>,
) -> anyhow::Result<bool> {
	imp::remove_if_unchanged(root, path, expected, expected_mode)
}

pub(crate) fn remove_dir(root: &Path, path: &Path) -> anyhow::Result<bool> {
	imp::remove_dir(root, path)
}
