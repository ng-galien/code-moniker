use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

use super::{
	HookBackend, backend_config_file, backend_hook_event, backend_hook_name, backend_matcher,
	backend_name, backend_project_dir, backend_settings_command, binary_shell_command,
	hook_file_name, hook_script, normalize_relative, resolve_from_root,
};
use crate::args::{AgentClient, HookInstallArgs};

#[derive(Debug)]
pub(crate) struct AgentHookInstallation {
	pub(crate) path: PathBuf,
	pub(crate) fingerprint: Vec<u8>,
	pub(crate) owned: bool,
	pub(crate) config_created: bool,
	pub(crate) config_parent_created: bool,
	pub(crate) hook_directory_created: bool,
	pub(crate) previous_config_checksum: Option<String>,
	pub(crate) config_checksum: Option<String>,
	pub(crate) rollback: Option<AgentHookRollback>,
}

#[derive(Debug)]
pub(crate) struct AgentHookRollback {
	root: PathBuf,
	hook_path: PathBuf,
	config_path: PathBuf,
	previous_hook: Option<Vec<u8>>,
	previous_hook_mode: Option<u32>,
	previous_config: Option<Vec<u8>>,
	previous_config_mode: Option<u32>,
	committed_hook: Option<Vec<u8>>,
	committed_hook_mode: Option<u32>,
	committed_config: Option<Vec<u8>>,
	committed_config_mode: Option<u32>,
	config_parent_created: bool,
	hook_directory_created: bool,
}

#[derive(Clone, Copy)]
pub(crate) struct AgentHookRemovalPolicy<'a> {
	pub(crate) config_created: bool,
	pub(crate) config_parent_created: bool,
	pub(crate) hook_directory_created: bool,
	pub(crate) config_checksum: Option<&'a str>,
}

struct AgentHookPlan {
	backend: HookBackend,
	root: PathBuf,
	scope: PathBuf,
	hook_path: PathBuf,
	config_path: PathBuf,
	config: Value,
	expected_script: String,
	previous_script: Option<Vec<u8>>,
	previous_script_mode: Option<u32>,
	previous_config: Option<Vec<u8>>,
	previous_config_mode: Option<u32>,
	config_parent_created: bool,
	hook_directory_created: bool,
}

struct WrittenAgentHook {
	backend: HookBackend,
	scope: PathBuf,
	hook_path: PathBuf,
	config_path: PathBuf,
	rollback: AgentHookRollback,
}

struct PreparedAgentHookRemoval {
	root: PathBuf,
	hook_path: PathBuf,
	config_path: PathBuf,
	previous_hook: Option<Vec<u8>>,
	previous_hook_mode: Option<u32>,
	previous_config: Option<Vec<u8>>,
	previous_config_mode: Option<u32>,
	updated_config: Option<Vec<u8>>,
	remove_config: bool,
	config_parent_created: bool,
	hook_directory_created: bool,
}

enum PreparedAgentHook {
	External {
		installation: AgentHookInstallation,
		backend: HookBackend,
		scope: PathBuf,
	},
	Install(AgentHookPlan),
}

pub(crate) fn install_for_agent<W: Write>(
	args: &HookInstallArgs,
	client: AgentClient,
	managed_path: Option<&Path>,
	stdout: &mut W,
) -> anyhow::Result<AgentHookInstallation> {
	let plan = match prepare_agent_hook(args, client, managed_path)? {
		PreparedAgentHook::External {
			installation,
			backend,
			scope,
		} => {
			writeln!(
				stdout,
				"Retained matching external {} live hooks on `{}`.",
				backend_name(backend),
				scope.display()
			)?;
			return Ok(installation);
		}
		PreparedAgentHook::Install(plan) => plan,
	};

	let written = write_agent_hook(plan)?;
	let fingerprint = match agent_hook_fingerprint(client, &written.hook_path) {
		Ok(fingerprint) => fingerprint,
		Err(error) => {
			if let Err(rollback) = rollback_agent_hook(&written.rollback) {
				bail!(
					"{error:#}; additionally failed to roll back the hook installation: {rollback:#}"
				);
			}
			return Err(error);
		}
	};
	writeln!(
		stdout,
		"Installed {} live hooks on `{}`.",
		backend_name(written.backend),
		written.scope.display()
	)?;
	writeln!(stdout, "Hook: {}", written.hook_path.display())?;
	writeln!(
		stdout,
		"{} config: {}",
		backend_name(written.backend),
		written.config_path.display()
	)?;
	Ok(AgentHookInstallation {
		fingerprint,
		path: written.hook_path,
		owned: true,
		config_created: written.rollback.previous_config.is_none(),
		config_parent_created: written.rollback.config_parent_created,
		hook_directory_created: written.rollback.hook_directory_created,
		previous_config_checksum: written
			.rollback
			.previous_config
			.as_deref()
			.map(content_checksum),
		config_checksum: written
			.rollback
			.committed_config
			.as_deref()
			.map(content_checksum),
		rollback: Some(written.rollback),
	})
}

fn prepare_agent_hook(
	args: &HookInstallArgs,
	client: AgentClient,
	managed_path: Option<&Path>,
) -> anyhow::Result<PreparedAgentHook> {
	let backend = client_backend(client);
	if !args.root.is_absolute() {
		bail!(
			"agent hook project root `{}` must already be canonical and absolute",
			args.root.display()
		);
	}
	let root = args.root.to_path_buf();
	crate::fs_nofollow::ensure_dir(&root, &root)
		.with_context(|| format!("project root `{}` is not physical", root.display()))?;
	let rules = resolve_from_root(&root, &args.rules);
	let scope = normalize_relative(&args.scope);
	let cfg = code_moniker_check::RuleSetRequest::with_rules(&rules, crate::DEFAULT_SCHEME)
		.with_project_root(&root)
		.load_config()?;
	if let Some(profile) = &args.profile
		&& !cfg.profiles.contains_key(profile)
	{
		bail!(
			"profile `{profile}` is not defined in `{}`; add [profiles.{profile}] before installing the live hooks",
			rules.display()
		);
	}

	let project_dir = root.join(backend_project_dir(backend));
	let config_parent_created = !crate::fs_nofollow::directory_exists(&root, &project_dir)?;
	let hook_directory = project_dir.join("hooks");
	let hook_directory_created = !crate::fs_nofollow::directory_exists(&root, &hook_directory)?;
	let hook_path = project_dir
		.join("hooks")
		.join(hook_file_name(args.profile.as_deref()));
	let config_path = project_dir.join(backend_config_file(backend));
	super::physical_hook::validate(&root, &hook_path, &config_path)?;
	validate_managed_target(managed_path, &hook_path)?;
	let binary = binary_shell_command();
	let expected_script = hook_script(
		args.profile.as_deref(),
		&args.rules,
		&scope,
		args.max_violations,
		backend,
		&binary,
	);
	let previous_config = crate::fs_nofollow::read(&root, &config_path)?;
	let previous_config_mode = crate::fs_nofollow::mode(&root, &config_path)?;
	let config = match &previous_config {
		Some(contents) => parse_hook_json(&config_path, contents)?,
		None => Value::Object(Map::new()),
	};
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	let registration_matches = exact_hook_registration(&config, backend, &command).is_some();
	if managed_path.is_some() && exact_hook_registration_count(&config, backend, &command) != 1 {
		bail!(
			"managed hook registration in `{}` is missing, duplicated, or modified",
			config_path.display()
		);
	}
	let command_registered = hook_registration_contains_command(&config, backend, &command);
	let script = crate::fs_nofollow::read(&root, &hook_path)?;
	let script_mode = crate::fs_nofollow::mode(&root, &hook_path)?;
	let script_exists = script.is_some();
	let script_matches = script
		.as_deref()
		.is_some_and(|contents| contents == expected_script.as_bytes());
	if managed_path.is_none() {
		validate_unmanaged_target(
			&hook_path,
			&config_path,
			script_exists,
			script_matches,
			registration_matches,
			command_registered,
		)?;
		if script_matches && registration_matches {
			let mode = script_mode.context("matching unmanaged hook mode is missing")?;
			ensure_hook_is_executable(&hook_path, mode)?;
			return Ok(PreparedAgentHook::External {
				installation: AgentHookInstallation {
					fingerprint: agent_hook_fingerprint(client, &hook_path)?,
					path: hook_path,
					owned: false,
					config_created: false,
					config_parent_created: false,
					hook_directory_created: false,
					previous_config_checksum: previous_config.as_deref().map(content_checksum),
					config_checksum: previous_config.as_deref().map(content_checksum),
					rollback: None,
				},
				backend,
				scope,
			});
		}
	}
	Ok(PreparedAgentHook::Install(AgentHookPlan {
		backend,
		root,
		scope,
		hook_path,
		config_path: config_path.clone(),
		config: upsert_exact_tool_hook(config, backend, &command, &config_path)?,
		expected_script,
		previous_script: script,
		previous_script_mode: script_mode,
		previous_config,
		previous_config_mode,
		config_parent_created,
		hook_directory_created,
	}))
}

fn validate_managed_target(managed_path: Option<&Path>, hook_path: &Path) -> anyhow::Result<()> {
	if let Some(managed_path) = managed_path
		&& managed_path != hook_path
	{
		bail!(
			"refusing to change the managed hook target from `{}` to `{}`; uninstall the hooks component before changing its profile",
			managed_path.display(),
			hook_path.display()
		);
	}
	Ok(())
}

fn validate_unmanaged_target(
	hook_path: &Path,
	config_path: &Path,
	script_exists: bool,
	script_matches: bool,
	registration_matches: bool,
	command_registered: bool,
) -> anyhow::Result<()> {
	if script_exists && !script_matches {
		bail!(
			"refusing to replace unmanaged hook `{}`",
			hook_path.display()
		);
	}
	if command_registered && !registration_matches {
		bail!(
			"refusing to replace unmanaged hook registration in `{}`",
			config_path.display()
		);
	}
	if script_matches != registration_matches {
		bail!(
			"refusing to claim partial unmanaged hook installation at `{}`",
			hook_path.display()
		);
	}
	Ok(())
}

fn write_agent_hook(plan: AgentHookPlan) -> anyhow::Result<WrittenAgentHook> {
	let AgentHookPlan {
		backend,
		root,
		scope,
		hook_path,
		config_path,
		config,
		expected_script,
		previous_script,
		previous_script_mode,
		previous_config,
		previous_config_mode,
		config_parent_created,
		hook_directory_created,
	} = plan;
	super::physical_hook::ensure_directories(&root, &hook_path)?;
	super::physical_hook::validate(&root, &hook_path, &config_path)?;
	let script_mode = previous_script_mode.unwrap_or(0o644) | 0o755;
	let config_mode = previous_config_mode.unwrap_or(0o600);
	let config_bytes = serde_json::to_vec_pretty(&config)?;
	let write_result = (|| -> anyhow::Result<()> {
		crate::fs_nofollow::write_if_unchanged(
			&root,
			&hook_path,
			previous_script.as_deref(),
			previous_script_mode,
			expected_script.as_bytes(),
			Some(script_mode),
		)?;
		crate::fs_nofollow::write_if_unchanged(
			&root,
			&config_path,
			previous_config.as_deref(),
			previous_config_mode,
			&config_bytes,
			Some(config_mode),
		)?;
		Ok(())
	})();
	if let Err(error) = write_result {
		if let Err(rollback) = restore_hook_file_if_unchanged(
			&root,
			&hook_path,
			Some(expected_script.as_bytes()),
			Some(script_mode),
			previous_script.as_deref(),
			previous_script_mode,
		) {
			bail!("{error:#}; additionally failed to restore the hook: {rollback:#}");
		}
		return Err(error);
	}
	let rollback = AgentHookRollback {
		root,
		hook_path: hook_path.clone(),
		config_path: config_path.clone(),
		previous_hook: previous_script,
		previous_hook_mode: previous_script_mode,
		previous_config,
		previous_config_mode,
		committed_hook: Some(expected_script.into_bytes()),
		committed_hook_mode: Some(script_mode),
		committed_config: Some(config_bytes),
		committed_config_mode: Some(config_mode),
		config_parent_created,
		hook_directory_created,
	};
	Ok(WrittenAgentHook {
		backend,
		scope,
		hook_path,
		config_path,
		rollback,
	})
}

pub(crate) fn agent_hook_fingerprint(
	client: AgentClient,
	hook_path: &Path,
) -> anyhow::Result<Vec<u8>> {
	let backend = client_backend(client);
	let project_dir = hook_path
		.parent()
		.and_then(Path::parent)
		.context("managed hook path must be under a client hooks directory")?;
	let root = project_dir
		.parent()
		.context("managed hook path must be under a project root")?;
	let config_path = project_dir.join(backend_config_file(backend));
	super::physical_hook::validate(root, hook_path, &config_path)?;
	let config = read_hook_json(root, &config_path)?;
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	if exact_hook_registration_count(&config, backend, &command) != 1 {
		bail!("managed hook registration is missing, duplicated, or modified");
	}
	let registration = exact_hook_registration(&config, backend, &command)
		.context("managed hook registration is missing or modified")?;
	let script = crate::fs_nofollow::read(root, hook_path)?
		.with_context(|| format!("managed hook `{}` is missing", hook_path.display()))?;
	let mode = crate::fs_nofollow::mode(root, hook_path)?
		.with_context(|| format!("managed hook `{}` is missing", hook_path.display()))?;
	ensure_hook_is_executable(hook_path, mode)?;
	hook_fingerprint(&script, mode, registration)
}

pub(crate) fn agent_hook_is_missing(client: AgentClient, hook_path: &Path) -> anyhow::Result<bool> {
	let backend = client_backend(client);
	let root = hook_path
		.parent()
		.and_then(Path::parent)
		.and_then(Path::parent)
		.context("managed hook path must be under a project client hooks directory")?;
	let project_dir = hook_path
		.parent()
		.and_then(Path::parent)
		.context("managed hook path must be under a client hooks directory")?;
	let config_path = project_dir.join(backend_config_file(backend));
	super::physical_hook::validate(root, hook_path, &config_path)?;
	let script_exists = crate::fs_nofollow::exists(root, hook_path)?;
	let config = read_hook_json(root, &config_path)?;
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	let registration_exists = exact_hook_registration(&config, backend, &command).is_some();
	Ok(!script_exists && !registration_exists)
}

#[cfg(test)]
pub(crate) fn uninstall_for_agent(
	root: &Path,
	client: AgentClient,
	hook_path: &Path,
	expected_fingerprint: &[u8],
) -> anyhow::Result<AgentHookRollback> {
	uninstall_for_agent_with_policy(
		root,
		client,
		hook_path,
		expected_fingerprint,
		AgentHookRemovalPolicy {
			config_created: false,
			config_parent_created: false,
			hook_directory_created: false,
			config_checksum: None,
		},
	)
}

pub(crate) fn uninstall_for_agent_with_policy(
	root: &Path,
	client: AgentClient,
	hook_path: &Path,
	expected_fingerprint: &[u8],
	policy: AgentHookRemovalPolicy<'_>,
) -> anyhow::Result<AgentHookRollback> {
	let removal =
		prepare_agent_hook_removal(root, client, hook_path, expected_fingerprint, policy)?;
	commit_agent_hook_removal(removal)
}

fn prepare_agent_hook_removal(
	root: &Path,
	client: AgentClient,
	hook_path: &Path,
	expected_fingerprint: &[u8],
	policy: AgentHookRemovalPolicy<'_>,
) -> anyhow::Result<PreparedAgentHookRemoval> {
	let backend = client_backend(client);
	if !root.is_absolute() {
		bail!(
			"agent hook project root `{}` must already be canonical and absolute",
			root.display()
		);
	}
	let root = root.to_path_buf();
	let config_path = root
		.join(backend_project_dir(backend))
		.join(backend_config_file(backend));
	super::physical_hook::validate(&root, hook_path, &config_path)?;
	let previous_config = crate::fs_nofollow::read(&root, &config_path)?;
	let previous_config_mode = crate::fs_nofollow::mode(&root, &config_path)?;
	let previous_hook = crate::fs_nofollow::read(&root, hook_path)?;
	let previous_hook_mode = crate::fs_nofollow::mode(&root, hook_path)?;
	let mut config = parse_hook_json(
		&config_path,
		previous_config
			.as_deref()
			.context("managed hook configuration is missing")?,
	)?;
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	if exact_hook_registration_count(&config, backend, &command) != 1 {
		bail!(
			"managed hook registration in `{}` is missing, duplicated, or modified",
			config_path.display()
		);
	}
	let registration = exact_hook_registration(&config, backend, &command)
		.context("managed hook registration is missing or modified")?;
	let fingerprint = hook_fingerprint(
		previous_hook
			.as_deref()
			.with_context(|| format!("managed hook `{}` is missing", hook_path.display()))?,
		previous_hook_mode
			.with_context(|| format!("managed hook `{}` is missing", hook_path.display()))?,
		registration,
	)?;
	if fingerprint != expected_fingerprint {
		bail!(
			"managed hook `{}` changed after uninstall preflight",
			hook_path.display()
		);
	}
	remove_exact_hook_registration(&mut config, backend, &command);
	let remove_config = policy.config_created
		&& previous_config.as_deref().map(content_checksum).as_deref() == policy.config_checksum;
	let updated_config = if remove_config {
		None
	} else {
		Some(serde_json::to_vec_pretty(&config)?)
	};
	Ok(PreparedAgentHookRemoval {
		root,
		hook_path: hook_path.to_path_buf(),
		config_path,
		previous_hook,
		previous_hook_mode,
		previous_config,
		previous_config_mode,
		updated_config,
		remove_config,
		config_parent_created: policy.config_parent_created,
		hook_directory_created: policy.hook_directory_created,
	})
}

fn commit_agent_hook_removal(
	removal: PreparedAgentHookRemoval,
) -> anyhow::Result<AgentHookRollback> {
	let PreparedAgentHookRemoval {
		root,
		hook_path,
		config_path,
		previous_hook,
		previous_hook_mode,
		previous_config,
		previous_config_mode,
		updated_config,
		remove_config,
		config_parent_created,
		hook_directory_created,
	} = removal;
	if previous_hook.is_some() {
		crate::fs_nofollow::remove_if_unchanged(
			&root,
			&hook_path,
			previous_hook.as_deref().unwrap_or_default(),
			previous_hook_mode,
		)?;
	}
	let config_result = match &updated_config {
		Some(updated_config) => crate::fs_nofollow::write_if_unchanged(
			&root,
			&config_path,
			previous_config.as_deref(),
			previous_config_mode,
			updated_config,
			previous_config_mode,
		),
		None => crate::fs_nofollow::remove_if_unchanged(
			&root,
			&config_path,
			previous_config.as_deref().unwrap_or_default(),
			previous_config_mode,
		)
		.map(|_| ()),
	};
	if let Err(error) = config_result {
		if let Err(rollback) = restore_hook_file_if_unchanged(
			&root,
			&hook_path,
			None,
			None,
			previous_hook.as_deref(),
			previous_hook_mode,
		) {
			bail!("{error:#}; additionally failed to restore the hook: {rollback:#}");
		}
		return Err(error);
	}
	let rollback = AgentHookRollback {
		root,
		hook_path: hook_path.clone(),
		config_path,
		previous_hook,
		previous_hook_mode,
		previous_config,
		previous_config_mode,
		committed_hook: None,
		committed_hook_mode: None,
		committed_config: updated_config,
		committed_config_mode: if remove_config {
			None
		} else {
			previous_config_mode
		},
		config_parent_created,
		hook_directory_created,
	};
	let cleanup_result = (|| -> anyhow::Result<()> {
		if hook_directory_created && let Some(hook_directory) = hook_path.parent() {
			crate::fs_nofollow::remove_dir(&rollback.root, hook_directory)?;
		}
		if remove_config
			&& config_parent_created
			&& let Some(config_parent) = rollback.config_path.parent()
		{
			crate::fs_nofollow::remove_dir(&rollback.root, config_parent)?;
		}
		Ok(())
	})();
	if let Err(error) = cleanup_result {
		if let Err(rollback_error) = rollback_agent_hook(&rollback) {
			bail!(
				"{error:#}; additionally failed to roll back the hook removal: {rollback_error:#}"
			);
		}
		return Err(error);
	}
	Ok(rollback)
}

fn hook_fingerprint(script: &[u8], mode: u32, registration: &Value) -> anyhow::Result<Vec<u8>> {
	let mut fingerprint = script.to_vec();
	fingerprint.push(0xff);
	fingerprint.extend(mode.to_le_bytes());
	fingerprint.push(0xff);
	fingerprint.extend(serde_json::to_vec(registration)?);
	Ok(fingerprint)
}

fn content_checksum(bytes: &[u8]) -> String {
	let mut hash = 0xcbf29ce484222325_u64;
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}
	format!("{hash:016x}")
}

fn ensure_hook_is_executable(path: &Path, mode: u32) -> anyhow::Result<()> {
	if mode & 0o111 == 0 {
		bail!("hook `{}` is not executable", path.display());
	}
	Ok(())
}

fn client_backend(client: AgentClient) -> HookBackend {
	match client {
		AgentClient::Codex => HookBackend::Codex,
		AgentClient::Claude => HookBackend::Claude,
		AgentClient::Gemini => HookBackend::Gemini,
	}
}

pub(crate) fn rollback_agent_hook(rollback: &AgentHookRollback) -> anyhow::Result<()> {
	if rollback.committed_hook.is_none() {
		super::physical_hook::ensure_directories(&rollback.root, &rollback.hook_path)
			.context("cannot recreate hook directories")?;
		restore_hook_file_if_unchanged(
			&rollback.root,
			&rollback.hook_path,
			rollback.committed_hook.as_deref(),
			rollback.committed_hook_mode,
			rollback.previous_hook.as_deref(),
			rollback.previous_hook_mode,
		)
		.context("cannot roll back hook script")?;
		restore_hook_file_if_unchanged(
			&rollback.root,
			&rollback.config_path,
			rollback.committed_config.as_deref(),
			rollback.committed_config_mode,
			rollback.previous_config.as_deref(),
			rollback.previous_config_mode,
		)
		.context("cannot roll back hook configuration")?;
	} else {
		restore_hook_file_if_unchanged(
			&rollback.root,
			&rollback.config_path,
			rollback.committed_config.as_deref(),
			rollback.committed_config_mode,
			rollback.previous_config.as_deref(),
			rollback.previous_config_mode,
		)
		.context("cannot roll back hook configuration")?;
		restore_hook_file_if_unchanged(
			&rollback.root,
			&rollback.hook_path,
			rollback.committed_hook.as_deref(),
			rollback.committed_hook_mode,
			rollback.previous_hook.as_deref(),
			rollback.previous_hook_mode,
		)
		.context("cannot roll back hook script")?;
		if rollback.hook_directory_created
			&& let Some(hook_directory) = rollback.hook_path.parent()
		{
			crate::fs_nofollow::remove_dir(&rollback.root, hook_directory)?;
		}
		if rollback.config_parent_created
			&& let Some(config_parent) = rollback.config_path.parent()
		{
			crate::fs_nofollow::remove_dir(&rollback.root, config_parent)?;
		}
	}
	Ok(())
}

fn restore_hook_file_if_unchanged(
	root: &Path,
	path: &Path,
	current: Option<&[u8]>,
	current_mode: Option<u32>,
	previous: Option<&[u8]>,
	previous_mode: Option<u32>,
) -> anyhow::Result<()> {
	match (current, previous) {
		(Some(current), Some(previous)) => {
			crate::fs_nofollow::write_if_unchanged(
				root,
				path,
				Some(current),
				current_mode,
				previous,
				previous_mode,
			)?;
		}
		(Some(current), None) => {
			crate::fs_nofollow::remove_if_unchanged(root, path, current, current_mode)?;
		}
		(None, Some(previous)) => {
			crate::fs_nofollow::write_if_unchanged(
				root,
				path,
				None,
				None,
				previous,
				previous_mode,
			)?;
		}
		(None, None) => {}
	}
	Ok(())
}

fn read_hook_json(root: &Path, path: &Path) -> anyhow::Result<Value> {
	let Some(contents) = crate::fs_nofollow::read(root, path)? else {
		return Ok(Value::Object(Map::new()));
	};
	parse_hook_json(path, &contents)
}

fn parse_hook_json(path: &Path, contents: &[u8]) -> anyhow::Result<Value> {
	let value: Value = serde_json::from_slice(contents)
		.with_context(|| format!("invalid JSON in `{}`", path.display()))?;
	value
		.as_object()
		.map(|object| Value::Object(object.clone()))
		.with_context(|| format!("`{}` must contain a JSON object", path.display()))
}

fn upsert_exact_tool_hook(
	mut settings: Value,
	backend: HookBackend,
	command: &str,
	path: &Path,
) -> anyhow::Result<Value> {
	remove_exact_hook_registration(&mut settings, backend, command);
	let Some(root) = settings.as_object_mut() else {
		bail!("`{}` must contain a JSON object", path.display());
	};
	let hooks = root
		.entry("hooks")
		.or_insert_with(|| Value::Object(Map::new()))
		.as_object_mut()
		.with_context(|| format!("`{}` field `hooks` must be a JSON object", path.display()))?;
	let event = backend_hook_event(backend);
	let event_hooks = hooks
		.entry(event)
		.or_insert_with(|| Value::Array(Vec::new()))
		.as_array_mut()
		.with_context(|| {
			format!(
				"`{}` field `hooks.{event}` must be a JSON array",
				path.display()
			)
		})?;
	event_hooks.push(expected_hook_registration(backend, command));
	Ok(settings)
}

fn expected_hook_registration(backend: HookBackend, command: &str) -> Value {
	let mut hook = json!({
		"type": "command",
		"command": command
	});
	if let Some(name) = backend_hook_name(backend, command)
		&& let Some(hook) = hook.as_object_mut()
	{
		hook.insert("name".to_string(), Value::String(name));
	}
	json!({
		"matcher": backend_matcher(backend),
		"hooks": [hook]
	})
}

fn exact_hook_registration<'a>(
	settings: &'a Value,
	backend: HookBackend,
	command: &str,
) -> Option<&'a Value> {
	let expected = expected_hook_registration(backend, command);
	settings
		.get("hooks")
		.and_then(|hooks| hooks.get(backend_hook_event(backend)))
		.and_then(Value::as_array)
		.and_then(|entries| entries.iter().find(|entry| *entry == &expected))
}

fn exact_hook_registration_count(settings: &Value, backend: HookBackend, command: &str) -> usize {
	let expected = expected_hook_registration(backend, command);
	settings
		.get("hooks")
		.and_then(|hooks| hooks.get(backend_hook_event(backend)))
		.and_then(Value::as_array)
		.map(|entries| entries.iter().filter(|entry| *entry == &expected).count())
		.unwrap_or(0)
}

fn hook_registration_contains_command(
	settings: &Value,
	backend: HookBackend,
	command: &str,
) -> bool {
	settings
		.get("hooks")
		.and_then(|hooks| hooks.get(backend_hook_event(backend)))
		.and_then(Value::as_array)
		.is_some_and(|entries| {
			entries.iter().any(|entry| {
				entry
					.get("hooks")
					.and_then(Value::as_array)
					.is_some_and(|hooks| {
						hooks.iter().any(|hook| {
							hook.get("command").and_then(Value::as_str) == Some(command)
						})
					})
			})
		})
}

fn remove_exact_hook_registration(settings: &mut Value, backend: HookBackend, command: &str) {
	let expected = expected_hook_registration(backend, command);
	let mut remove_hooks_object = false;
	if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
		let event = backend_hook_event(backend);
		if let Some(event_hooks) = hooks.get_mut(event).and_then(Value::as_array_mut) {
			if let Some(position) = event_hooks.iter().position(|entry| entry == &expected) {
				event_hooks.remove(position);
			}
			if event_hooks.is_empty() {
				hooks.remove(event);
			}
		}
		remove_hooks_object = hooks.is_empty();
	}
	if remove_hooks_object && let Some(root) = settings.as_object_mut() {
		root.remove("hooks");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn prepared_install_refuses_to_overwrite_concurrent_config_update() {
		let directory = tempfile::tempdir().unwrap();
		std::fs::write(
			directory.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		std::fs::create_dir_all(directory.path().join(".codex")).unwrap();
		let config_path = directory.path().join(".codex/hooks.json");
		std::fs::write(&config_path, br#"{"user":"before"}"#).unwrap();
		let args = HookInstallArgs {
			root: directory.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let PreparedAgentHook::Install(plan) =
			prepare_agent_hook(&args, AgentClient::Codex, None).unwrap()
		else {
			panic!("expected a managed hook plan");
		};
		std::fs::write(&config_path, br#"{"user":"concurrent"}"#).unwrap();

		let error = match write_agent_hook(plan) {
			Ok(_) => panic!("concurrent configuration update was overwritten"),
			Err(error) => error.to_string(),
		};

		assert!(error.contains("changed concurrently"));
		assert_eq!(
			std::fs::read(&config_path).unwrap(),
			br#"{"user":"concurrent"}"#
		);
		assert!(
			!directory
				.path()
				.join(".codex/hooks/code-moniker-check.sh")
				.exists()
		);
	}

	#[cfg(unix)]
	#[test]
	fn matching_external_hook_must_be_executable() {
		use std::os::unix::fs::PermissionsExt;

		let directory = tempfile::tempdir().unwrap();
		std::fs::write(
			directory.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: directory.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		install_for_agent(&args, AgentClient::Codex, None, &mut Vec::new()).unwrap();
		let hook_path = directory.path().join(".codex/hooks/code-moniker-check.sh");
		std::fs::set_permissions(&hook_path, std::fs::Permissions::from_mode(0o644)).unwrap();

		let error = match prepare_agent_hook(&args, AgentClient::Codex, None) {
			Ok(_) => panic!("non-executable external hook was accepted"),
			Err(error) => error.to_string(),
		};

		assert!(error.contains("is not executable"));
		assert!(agent_hook_fingerprint(AgentClient::Codex, &hook_path).is_err());
	}

	#[cfg(unix)]
	#[test]
	fn replaced_root_symlink_is_rejected_before_rules_are_read() {
		use std::os::unix::fs::symlink;

		let directory = tempfile::tempdir().unwrap();
		let root = directory.path().join("project");
		let moved = directory.path().join("moved-project");
		let external = directory.path().join("external");
		std::fs::create_dir(&root).unwrap();
		std::fs::write(root.join(".code-moniker.toml"), "default_rules = true\n").unwrap();
		std::fs::rename(&root, &moved).unwrap();
		std::fs::create_dir(&external).unwrap();
		std::fs::write(
			external.join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		std::fs::write(external.join("sentinel"), "unchanged").unwrap();
		symlink(&external, &root).unwrap();
		let args = HookInstallArgs {
			root,
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};

		assert!(prepare_agent_hook(&args, AgentClient::Codex, None).is_err());
		assert_eq!(
			std::fs::read_to_string(external.join("sentinel")).unwrap(),
			"unchanged"
		);
		assert!(!external.join(".codex").exists());
	}

	#[test]
	fn removing_managed_registration_preserves_same_command_in_an_external_entry() {
		let command = "sh -c 'exec managed-hook'";
		let managed = expected_hook_registration(HookBackend::Codex, command);
		let external = json!({
			"matcher": "UserMatcher",
			"hooks": [{"type": "command", "command": command}]
		});
		let mut settings = json!({
			"hooks": {
				"PostToolUse": [managed, external.clone()]
			}
		});

		remove_exact_hook_registration(&mut settings, HookBackend::Codex, command);

		assert_eq!(settings["hooks"]["PostToolUse"], json!([external]));
	}

	#[test]
	fn duplicated_exact_registration_is_drift_and_removal_takes_only_one() {
		let command = "sh -c 'exec managed-hook'";
		let managed = expected_hook_registration(HookBackend::Codex, command);
		let mut settings = json!({
			"hooks": {
				"PostToolUse": [managed.clone(), managed]
			}
		});
		assert_eq!(
			exact_hook_registration_count(&settings, HookBackend::Codex, command),
			2
		);

		remove_exact_hook_registration(&mut settings, HookBackend::Codex, command);

		assert_eq!(
			exact_hook_registration_count(&settings, HookBackend::Codex, command),
			1
		);
	}

	#[cfg(unix)]
	#[test]
	fn rollback_of_new_config_detects_a_concurrent_mode_change() {
		use std::os::unix::fs::PermissionsExt;

		let directory = tempfile::tempdir().unwrap();
		std::fs::write(
			directory.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: directory.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let PreparedAgentHook::Install(plan) =
			prepare_agent_hook(&args, AgentClient::Codex, None).unwrap()
		else {
			panic!("expected a managed hook plan");
		};
		let written = write_agent_hook(plan).unwrap();
		std::fs::set_permissions(&written.config_path, std::fs::Permissions::from_mode(0o644))
			.unwrap();

		let error = format!("{:#}", rollback_agent_hook(&written.rollback).unwrap_err());

		assert!(error.contains("changed concurrently"));
		assert!(written.config_path.exists());
	}
}
