use std::fs;
use std::fs::OpenOptions;
use std::hash::{Hash, Hasher};
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::DaemonWorkspaceConfig;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DaemonRegistryEntry {
	pub workspace_root: String,
	pub workspace_roots: Vec<String>,
	pub project: Option<String>,
	pub cache_dir: Option<String>,
	pub live_refresh: Option<String>,
	pub endpoint: String,
	pub token: String,
	pub pid: u32,
	#[serde(default)]
	pub state: DaemonRegistryState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum DaemonRegistryState {
	Indexing,
	#[default]
	Ready,
}

pub fn registry_dir() -> PathBuf {
	std::env::temp_dir().join("code-moniker-daemons")
}

pub fn canonical_workspace_root(root: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
	let root = root.as_ref();
	root.canonicalize()
		.map_err(|err| anyhow::anyhow!("cannot canonicalize {}: {err}", root.display()))
}

pub fn canonical_workspace_roots<I, P>(roots: I) -> anyhow::Result<Vec<PathBuf>>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	let mut canonical = Vec::new();
	for root in roots {
		let root = canonical_workspace_root(root)?;
		if !canonical.contains(&root) {
			canonical.push(root);
		}
	}
	if canonical.is_empty() {
		canonical.push(canonical_workspace_root(".")?);
	}
	Ok(canonical)
}

pub fn daemon_workspace_config<I, P>(
	roots: I,
	project: Option<String>,
	cache_dir: Option<PathBuf>,
	live_refresh: Option<String>,
) -> anyhow::Result<DaemonWorkspaceConfig>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	Ok(DaemonWorkspaceConfig {
		roots: canonical_workspace_roots(roots)?
			.into_iter()
			.map(|root| root.display().to_string())
			.collect(),
		project,
		cache_dir: cache_dir
			.map(normalize_path)
			.transpose()?
			.map(|path| path.display().to_string()),
		live_refresh,
	})
}

pub fn canonical_workspace_config(
	config: DaemonWorkspaceConfig,
) -> anyhow::Result<DaemonWorkspaceConfig> {
	daemon_workspace_config(
		config.roots.iter().map(PathBuf::from),
		config.project,
		config.cache_dir.map(PathBuf::from),
		config.live_refresh,
	)
}

pub fn config_from_roots<I, P>(roots: I) -> anyhow::Result<DaemonWorkspaceConfig>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	daemon_workspace_config(roots, None, None, Some("on-demand".to_string()))
}

pub fn registry_path_for_root(root: impl AsRef<Path>) -> anyhow::Result<PathBuf> {
	registry_path_for_roots([root])
}

pub fn registry_path_for_roots<I, P>(roots: I) -> anyhow::Result<PathBuf>
where
	I: IntoIterator<Item = P>,
	P: AsRef<Path>,
{
	registry_path_for_config(&config_from_roots(roots)?)
}

pub fn registry_path_for_config(config: &DaemonWorkspaceConfig) -> anyhow::Result<PathBuf> {
	let config = canonical_workspace_config(config.clone())?;
	Ok(registry_dir().join(format!("{}.json", stable_config_hash(&config))))
}

pub fn read_registry_entry(
	config: &DaemonWorkspaceConfig,
) -> anyhow::Result<Option<DaemonRegistryEntry>> {
	let path = registry_path_for_config(config)?;
	match fs::read_to_string(&path) {
		Ok(text) => Ok(serde_json::from_str(&text).ok()),
		Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
		Err(err) => Err(err.into()),
	}
}

pub fn write_registry_entry(
	config: &DaemonWorkspaceConfig,
	entry: &DaemonRegistryEntry,
) -> anyhow::Result<()> {
	fs::create_dir_all(registry_dir())?;
	atomic_write_registry_entry(&registry_path_for_config(config)?, entry)?;
	Ok(())
}

pub fn claim_registry_entry(
	config: &DaemonWorkspaceConfig,
	entry: &DaemonRegistryEntry,
) -> anyhow::Result<bool> {
	fs::create_dir_all(registry_dir())?;
	let path = registry_path_for_config(config)?;
	let text = serde_json::to_vec_pretty(entry)?;
	match OpenOptions::new().write(true).create_new(true).open(path) {
		Ok(mut file) => {
			file.write_all(&text)?;
			file.sync_all()?;
			Ok(true)
		}
		Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
		Err(error) => Err(error.into()),
	}
}

pub fn update_registry_entry_if_own(
	config: &DaemonWorkspaceConfig,
	entry: &DaemonRegistryEntry,
) -> anyhow::Result<bool> {
	let path = registry_path_for_config(config)?;
	let current = fs::read_to_string(&path)
		.ok()
		.and_then(|text| serde_json::from_str::<DaemonRegistryEntry>(&text).ok());
	let owned = current
		.map(|current| current.token == entry.token && current.pid == entry.pid)
		.unwrap_or(false);
	if owned {
		atomic_write_registry_entry(&path, entry)?;
	}
	Ok(owned)
}

fn atomic_write_registry_entry(path: &Path, entry: &DaemonRegistryEntry) -> anyhow::Result<()> {
	let temp = path.with_extension(format!("{}.tmp", entry.token));
	let text = serde_json::to_vec_pretty(entry)?;
	{
		let mut file = OpenOptions::new()
			.write(true)
			.create_new(true)
			.open(&temp)?;
		file.write_all(&text)?;
		file.sync_all()?;
	}
	fs::rename(temp, path)?;
	Ok(())
}

// Shutdown-time removal: a successor daemon may have overwritten this path
// with its own entry while we were stopping. Only unlink what is still ours,
// or the new daemon stays alive but invisible to the registry.
pub fn remove_registry_entry_if_own(path: &Path, own: &DaemonRegistryEntry) {
	let current = fs::read_to_string(path)
		.ok()
		.and_then(|text| serde_json::from_str::<DaemonRegistryEntry>(&text).ok());
	let owned = current
		.map(|entry| entry.token == own.token && entry.pid == own.pid)
		.unwrap_or(false);
	if owned {
		let _ = fs::remove_file(path);
	}
}

pub fn list_registry_files() -> anyhow::Result<Vec<(PathBuf, DaemonRegistryEntry)>> {
	let dir = registry_dir();
	if !dir.exists() {
		return Ok(Vec::new());
	}
	let mut entries = Vec::new();
	for entry in fs::read_dir(&dir)? {
		let entry = entry?;
		if entry.path().extension().and_then(|ext| ext.to_str()) != Some("json") {
			continue;
		}
		let text = fs::read_to_string(entry.path())?;
		if let Ok(registry) = serde_json::from_str::<DaemonRegistryEntry>(&text) {
			entries.push((entry.path(), registry));
		}
	}
	entries.sort_by(|(_, a), (_, b)| a.workspace_root.cmp(&b.workspace_root));
	Ok(entries)
}

pub fn pid_is_alive(pid: u32) -> bool {
	#[cfg(unix)]
	{
		// SAFETY: signal 0 never delivers a signal; it only asks the kernel
		// whether the PID exists and whether this process may signal it.
		let result = unsafe { libc::kill(pid as libc::pid_t, 0) };
		let errno = (result != 0)
			.then(|| std::io::Error::last_os_error().raw_os_error())
			.flatten();
		kill_result_means_alive(result, errno)
	}
	#[cfg(not(unix))]
	{
		let _ = pid;
		true
	}
}

#[cfg(unix)]
fn kill_result_means_alive(result: i32, errno: Option<i32>) -> bool {
	result == 0 || errno != Some(libc::ESRCH)
}

pub fn list_registry_entries() -> anyhow::Result<Vec<DaemonRegistryEntry>> {
	let mut entries = Vec::new();
	for (path, entry) in list_registry_files()? {
		if pid_is_alive(entry.pid) {
			entries.push(entry);
		} else {
			remove_registry_entry_if_own(&path, &entry);
		}
	}
	entries.sort_by(|a, b| a.workspace_root.cmp(&b.workspace_root));
	Ok(entries)
}

pub fn config_roots(config: &DaemonWorkspaceConfig) -> Vec<PathBuf> {
	config.roots.iter().map(PathBuf::from).collect()
}

pub fn workspace_label(roots: &[PathBuf]) -> String {
	if roots.len() == 1 {
		roots[0].display().to_string()
	} else {
		roots
			.iter()
			.map(|root| root.display().to_string())
			.collect::<Vec<_>>()
			.join(";")
	}
}

fn normalize_path(path: PathBuf) -> anyhow::Result<PathBuf> {
	if path.is_absolute() {
		Ok(path)
	} else {
		Ok(std::env::current_dir()?.join(path))
	}
}

// The registry key is the workspace identity: what gets indexed (roots,
// project, cache), never how it refreshes. Hashing live_refresh here once
// split one workspace across two registry slots — a daemon started with
// `--live-refresh auto` was invisible to a default-mode `daemon status`.
fn stable_config_hash(config: &DaemonWorkspaceConfig) -> String {
	let mut hasher = StableHasher::default();
	for root in &config.roots {
		root.hash(&mut hasher);
		0xff_u8.hash(&mut hasher);
	}
	config.project.hash(&mut hasher);
	0xfe_u8.hash(&mut hasher);
	config.cache_dir.hash(&mut hasher);
	format!("{:016x}", hasher.finish())
}

#[derive(Default)]
struct StableHasher(u64);

impl Hasher for StableHasher {
	fn finish(&self) -> u64 {
		self.0
	}

	fn write(&mut self, bytes: &[u8]) {
		let mut hash = if self.0 == 0 {
			0xcbf29ce484222325
		} else {
			self.0
		};
		for byte in bytes {
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(0x100000001b3);
		}
		self.0 = hash;
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn entry(token: &str, pid: u32) -> DaemonRegistryEntry {
		DaemonRegistryEntry {
			workspace_root: "/tmp/ws".to_string(),
			workspace_roots: vec!["/tmp/ws".to_string()],
			project: None,
			cache_dir: None,
			live_refresh: None,
			endpoint: "127.0.0.1:1".to_string(),
			token: token.to_string(),
			pid,
			state: DaemonRegistryState::Ready,
		}
	}

	#[test]
	fn registry_identity_ignores_the_refresh_mode() {
		let base = DaemonWorkspaceConfig {
			roots: vec!["/tmp/ws".to_string()],
			project: None,
			cache_dir: None,
			live_refresh: Some("auto".to_string()),
		};
		let mut on_demand = base.clone();
		on_demand.live_refresh = Some("on-demand".to_string());
		assert_eq!(
			stable_config_hash(&base),
			stable_config_hash(&on_demand),
			"one workspace must map to one registry slot, whatever the refresh mode"
		);

		let mut other_project = base.clone();
		other_project.project = Some("api".to_string());
		assert_ne!(
			stable_config_hash(&base),
			stable_config_hash(&other_project),
			"what gets indexed still separates registry slots"
		);
	}

	#[test]
	fn shutdown_removal_spares_a_successor_entry() {
		let dir = tempfile::tempdir().expect("tempdir");
		let path = dir.path().join("ws.json");
		let old = entry("old-token", 111);
		let new = entry("new-token", 222);

		fs::write(&path, serde_json::to_string(&new).expect("json")).expect("write");
		remove_registry_entry_if_own(&path, &old);
		assert!(path.exists(), "the successor's entry must survive");

		remove_registry_entry_if_own(&path, &new);
		assert!(!path.exists(), "the owner removes its own entry");

		remove_registry_entry_if_own(&path, &new);
	}

	#[test]
	fn legacy_registry_entries_default_to_ready() {
		let mut value = serde_json::to_value(entry("legacy", 111)).expect("json");
		value.as_object_mut().expect("object").remove("state");
		let decoded: DaemonRegistryEntry = serde_json::from_value(value).expect("legacy entry");
		assert_eq!(decoded.state, DaemonRegistryState::Ready);
	}

	#[test]
	fn atomic_registry_update_replaces_a_complete_entry() {
		let dir = tempfile::tempdir().expect("tempdir");
		let path = dir.path().join("workspace.json");
		let indexing = DaemonRegistryEntry {
			state: DaemonRegistryState::Indexing,
			..entry("same-daemon", 111)
		};
		atomic_write_registry_entry(&path, &indexing).expect("write indexing entry");
		let ready = DaemonRegistryEntry {
			state: DaemonRegistryState::Ready,
			..indexing.clone()
		};
		atomic_write_registry_entry(&path, &ready).expect("write ready entry");
		let read: DaemonRegistryEntry =
			serde_json::from_str(&fs::read_to_string(path).expect("read entry")).expect("json");
		assert_eq!(read, ready);
	}

	#[cfg(unix)]
	#[test]
	fn permission_denied_pid_is_alive_but_missing_pid_is_dead() {
		assert!(kill_result_means_alive(-1, Some(libc::EPERM)));
		assert!(!kill_result_means_alive(-1, Some(libc::ESRCH)));
		assert!(kill_result_means_alive(0, None));
	}
}
