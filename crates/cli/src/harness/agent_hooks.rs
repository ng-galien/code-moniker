use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

use super::{
	HarnessBackend, backend_config_file, backend_hook_event, backend_hook_name, backend_matcher,
	backend_name, backend_project_dir, backend_settings_command, binary_shell_command,
	hook_file_name, hook_script, make_executable, normalize_relative, read_json_object,
	resolve_from_root,
};
use crate::args::{AgentClient, CodexHarnessArgs};
use crate::fs_write::write_atomic;

#[derive(Debug)]
pub(crate) struct AgentHookInstallation {
	pub(crate) path: PathBuf,
	pub(crate) fingerprint: Vec<u8>,
	pub(crate) owned: bool,
}

struct AgentHookPlan {
	backend: HarnessBackend,
	scope: PathBuf,
	hook_path: PathBuf,
	config_path: PathBuf,
	config: Value,
	expected_script: String,
}

enum PreparedAgentHook {
	External {
		installation: AgentHookInstallation,
		backend: HarnessBackend,
		scope: PathBuf,
	},
	Install(AgentHookPlan),
}

pub(crate) fn install_for_agent<W: Write>(
	args: &CodexHarnessArgs,
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
				"Retained matching external {} live harness on `{}`.",
				backend_name(backend),
				scope.display()
			)?;
			return Ok(installation);
		}
		PreparedAgentHook::Install(plan) => plan,
	};

	write_agent_hook(&plan)?;
	writeln!(
		stdout,
		"Installed {} live harness on `{}`.",
		backend_name(plan.backend),
		plan.scope.display()
	)?;
	writeln!(stdout, "Hook: {}", plan.hook_path.display())?;
	writeln!(
		stdout,
		"{} config: {}",
		backend_name(plan.backend),
		plan.config_path.display()
	)?;
	Ok(AgentHookInstallation {
		fingerprint: agent_hook_fingerprint(client, &plan.hook_path)?,
		path: plan.hook_path,
		owned: true,
	})
}

fn prepare_agent_hook(
	args: &CodexHarnessArgs,
	client: AgentClient,
	managed_path: Option<&Path>,
) -> anyhow::Result<PreparedAgentHook> {
	let backend = client_backend(client);
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let rules = resolve_from_root(&root, &args.rules);
	let scope = normalize_relative(&args.scope);
	let cfg = code_moniker_check::RuleSetRequest::with_rules(&rules, crate::DEFAULT_SCHEME)
		.load_config()?;
	if let Some(profile) = &args.profile
		&& !cfg.profiles.contains_key(profile)
	{
		bail!(
			"profile `{profile}` is not defined in `{}`; add [profiles.{profile}] before installing the live harness",
			rules.display()
		);
	}

	let project_dir = root.join(backend_project_dir(backend));
	let hook_path = project_dir
		.join("hooks")
		.join(hook_file_name(args.profile.as_deref()));
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
	let config_path = project_dir.join(backend_config_file(backend));
	let config = read_json_object(&config_path)?;
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	let registration_matches = exact_hook_registration(&config, backend, &command).is_some();
	let command_registered = hook_registration_contains_command(&config, backend, &command);
	let script_exists = fs::symlink_metadata(&hook_path).is_ok();
	let script_matches = fs::read(&hook_path)
		.map(|contents| contents == expected_script.as_bytes())
		.unwrap_or(false);
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
			return Ok(PreparedAgentHook::External {
				installation: AgentHookInstallation {
					fingerprint: agent_hook_fingerprint(client, &hook_path)?,
					path: hook_path,
					owned: false,
				},
				backend,
				scope,
			});
		}
	}
	Ok(PreparedAgentHook::Install(AgentHookPlan {
		backend,
		scope,
		hook_path,
		config_path: config_path.clone(),
		config: upsert_exact_tool_hook(config, backend, &command, &config_path)?,
		expected_script,
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

fn write_agent_hook(plan: &AgentHookPlan) -> anyhow::Result<()> {
	let hooks_dir = plan
		.hook_path
		.parent()
		.context("generated hook path has no parent directory")?;
	fs::create_dir_all(hooks_dir)
		.with_context(|| format!("cannot create `{}`", hooks_dir.display()))?;
	let script_exists = fs::symlink_metadata(&plan.hook_path).is_ok();
	let previous_script = if script_exists {
		Some(
			fs::read(&plan.hook_path)
				.with_context(|| format!("cannot back up `{}`", plan.hook_path.display()))?,
		)
	} else {
		None
	};
	let previous_permissions = fs::metadata(&plan.hook_path)
		.ok()
		.map(|metadata| metadata.permissions());
	let write_result = (|| -> anyhow::Result<()> {
		write_atomic(&plan.hook_path, plan.expected_script.as_bytes())?;
		make_executable(&plan.hook_path)?;
		write_atomic(&plan.config_path, &serde_json::to_vec_pretty(&plan.config)?)?;
		Ok(())
	})();
	if let Err(error) = write_result {
		if let Err(rollback) = restore_hook_file(
			&plan.hook_path,
			previous_script.as_deref(),
			previous_permissions,
		) {
			bail!("{error:#}; additionally failed to restore the hook: {rollback:#}");
		}
		return Err(error);
	}
	Ok(())
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
	let config_path = project_dir.join(backend_config_file(backend));
	let config = read_json_object(&config_path)?;
	let command = backend_settings_command(backend, &hook_path.display().to_string());
	let registration = exact_hook_registration(&config, backend, &command)
		.context("managed hook registration is missing or modified")?;
	let mut fingerprint = fs::read(hook_path)
		.with_context(|| format!("cannot read managed hook `{}`", hook_path.display()))?;
	fingerprint.push(0xff);
	fingerprint.extend(serde_json::to_vec(registration)?);
	Ok(fingerprint)
}

pub(crate) fn uninstall_for_agent(
	root: &Path,
	client: AgentClient,
	hook_path: &Path,
) -> anyhow::Result<()> {
	let backend = client_backend(client);
	let config_path = root
		.join(backend_project_dir(backend))
		.join(backend_config_file(backend));
	if config_path.exists() {
		let mut config = read_json_object(&config_path)?;
		let command = backend_settings_command(backend, &hook_path.display().to_string());
		remove_exact_hook_registration(&mut config, backend, &command);
		write_atomic(&config_path, &serde_json::to_vec_pretty(&config)?)?;
	}
	if hook_path.exists() {
		fs::remove_file(hook_path)
			.with_context(|| format!("cannot remove `{}`", hook_path.display()))?;
	}
	Ok(())
}

fn client_backend(client: AgentClient) -> HarnessBackend {
	match client {
		AgentClient::Codex => HarnessBackend::Codex,
		AgentClient::Claude => HarnessBackend::Claude,
		AgentClient::Gemini => HarnessBackend::Gemini,
	}
}

fn restore_hook_file(
	path: &Path,
	contents: Option<&[u8]>,
	permissions: Option<fs::Permissions>,
) -> anyhow::Result<()> {
	if let Some(contents) = contents {
		write_atomic(path, contents)?;
		if let Some(permissions) = permissions {
			fs::set_permissions(path, permissions)
				.with_context(|| format!("cannot restore permissions on `{}`", path.display()))?;
		}
	} else if fs::symlink_metadata(path).is_ok() {
		fs::remove_file(path).with_context(|| format!("cannot roll back `{}`", path.display()))?;
	}
	Ok(())
}

fn upsert_exact_tool_hook(
	mut settings: Value,
	backend: HarnessBackend,
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

fn expected_hook_registration(backend: HarnessBackend, command: &str) -> Value {
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
	backend: HarnessBackend,
	command: &str,
) -> Option<&'a Value> {
	let expected = expected_hook_registration(backend, command);
	settings
		.get("hooks")
		.and_then(|hooks| hooks.get(backend_hook_event(backend)))
		.and_then(Value::as_array)
		.and_then(|entries| entries.iter().find(|entry| *entry == &expected))
}

fn hook_registration_contains_command(
	settings: &Value,
	backend: HarnessBackend,
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

fn remove_exact_hook_registration(settings: &mut Value, backend: HarnessBackend, command: &str) {
	let mut remove_hooks_object = false;
	if let Some(hooks) = settings.get_mut("hooks").and_then(Value::as_object_mut) {
		let event = backend_hook_event(backend);
		if let Some(event_hooks) = hooks.get_mut(event).and_then(Value::as_array_mut) {
			for entry in event_hooks.iter_mut() {
				if let Some(entry_hooks) = entry.get_mut("hooks").and_then(Value::as_array_mut) {
					entry_hooks.retain(|hook| {
						hook.get("command").and_then(Value::as_str) != Some(command)
					});
				}
			}
			event_hooks.retain(|entry| {
				entry
					.get("hooks")
					.and_then(Value::as_array)
					.is_none_or(|hooks| !hooks.is_empty())
			});
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
