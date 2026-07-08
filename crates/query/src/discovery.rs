use std::fs;
use std::hash::{Hash, Hasher};
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
	fs::write(
		registry_path_for_config(config)?,
		serde_json::to_string_pretty(entry)?,
	)?;
	Ok(())
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
		std::process::Command::new("kill")
			.args(["-0", &pid.to_string()])
			.status()
			.map(|status| status.success())
			.unwrap_or(true)
	}
	#[cfg(not(unix))]
	{
		let _ = pid;
		true
	}
}

pub fn list_registry_entries() -> anyhow::Result<Vec<DaemonRegistryEntry>> {
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
			entries.push(registry);
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

fn stable_config_hash(config: &DaemonWorkspaceConfig) -> String {
	let mut hasher = StableHasher::default();
	for root in &config.roots {
		root.hash(&mut hasher);
		0xff_u8.hash(&mut hasher);
	}
	config.project.hash(&mut hasher);
	0xfe_u8.hash(&mut hasher);
	config.cache_dir.hash(&mut hasher);
	0xfd_u8.hash(&mut hasher);
	config.live_refresh.hash(&mut hasher);
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
