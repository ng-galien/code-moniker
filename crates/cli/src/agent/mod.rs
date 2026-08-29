use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::args::{
	AgentClient, AgentCommand, AgentComponent, AgentInspectArgs, AgentInstallArgs,
	AgentUninstallArgs, HookInstallArgs,
};
use crate::{Exit, hooks};

mod physical_config;
mod physical_skill;

const STATE_SCHEMA: u32 = 1;
const SKILL_NAME: &str = "code-moniker";

// Code Moniker enriches an agent session, but it must never prevent the owning
// client from starting when the local MCP process is temporarily unavailable
// (for example after EMFILE or during a binary replacement).
const CODEX_MCP_REQUIRED_FOR_CLIENT_STARTUP: bool = false;

const SKILL_FILES: &[(&str, &str)] = &[
	(
		"SKILL.md",
		include_str!("../../assets/agent/code-moniker/SKILL.md"),
	),
	(
		"postures/onboard.md",
		include_str!("../../assets/agent/code-moniker/postures/onboard.md"),
	),
	(
		"postures/develop.md",
		include_str!("../../assets/agent/code-moniker/postures/develop.md"),
	),
	(
		"postures/guard.md",
		include_str!("../../assets/agent/code-moniker/postures/guard.md"),
	),
	(
		"postures/review.md",
		include_str!("../../assets/agent/code-moniker/postures/review.md"),
	),
];

#[derive(Debug, Default, Deserialize, Serialize)]
struct InstallState {
	schema: u32,
	version: String,
	client: String,
	root: String,
	components: BTreeMap<String, ComponentState>,
	#[serde(skip)]
	persisted_contents: Option<Vec<u8>>,
	#[serde(skip)]
	persisted_mode: Option<u32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct ComponentState {
	scope: String,
	path: String,
	checksum: String,
	#[serde(default)]
	owned: bool,
	#[serde(default)]
	version: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	profile: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	rules: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	check_scope: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	max_violations: Option<usize>,
	#[serde(default)]
	config_created: bool,
	#[serde(default)]
	config_parent_created: bool,
	#[serde(default)]
	hook_directory_created: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	config_checksum: Option<String>,
}

struct InstallContext {
	home: PathBuf,
	binary: PathBuf,
	root: PathBuf,
}

struct HookPolicy {
	rules: PathBuf,
	profile: Option<String>,
	check_scope: PathBuf,
	max_violations: usize,
}

impl HookPolicy {
	fn from_args(args: &AgentInstallArgs) -> Self {
		Self {
			rules: args.rules.to_path_buf(),
			profile: args.profile.clone(),
			check_scope: args.check_scope.to_path_buf(),
			max_violations: args.max_violations,
		}
	}

	fn from_component(component: &ComponentState) -> Self {
		Self {
			rules: component
				.rules
				.as_deref()
				.unwrap_or(".code-moniker.toml")
				.into(),
			profile: component.profile.clone(),
			check_scope: component.check_scope.as_deref().unwrap_or(".").into(),
			max_violations: component.max_violations.unwrap_or(10),
		}
	}
}

pub fn run<W1: Write, W2: Write>(
	args: &crate::args::AgentArgs,
	stdout: &mut W1,
	stderr: &mut W2,
) -> Exit {
	let result = match &args.command {
		AgentCommand::Install(args) => install(args, false, stdout),
		AgentCommand::Update(args) => install(args, true, stdout),
		AgentCommand::Status(args) => status(args, stdout).map(|_| true),
		AgentCommand::Doctor(args) => doctor(args, stdout),
		AgentCommand::Uninstall(args) => uninstall(args, stdout),
		AgentCommand::ToolFiles(args) => hooks::write_tool_files(args, stdout).map(|_| true),
	};
	match result {
		Ok(true) => Exit::Match,
		Ok(false) => Exit::NoMatch,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

fn install<W: Write>(
	args: &AgentInstallArgs,
	preserve_hook_policy: bool,
	stdout: &mut W,
) -> anyhow::Result<bool> {
	let context = install_context(&args.root)?;
	let components = resolved_install_components(&args.components);
	if components.contains(&AgentComponent::Mcp) && !cfg!(feature = "mcp") {
		bail!(
			"this code-moniker binary has no MCP support; reinstall it with `cargo install code-moniker --features mcp`"
		);
	}

	let state_path = state_path(&context, args.client);
	let _state_lock =
		crate::fs_nofollow::lock_exclusive(&context.home, &state_lock_path(&state_path))?;
	let mut state = read_state(&context.home, &state_path)?.unwrap_or_else(|| InstallState {
		schema: STATE_SCHEMA,
		version: env!("CARGO_PKG_VERSION").to_string(),
		client: client_name(args.client).to_string(),
		root: context.root.display().to_string(),
		components: BTreeMap::new(),
		persisted_contents: None,
		persisted_mode: None,
	});
	validate_state_identity(&state, &context, args.client)?;

	if components.contains(&AgentComponent::Skill) {
		let installed = install_skill(&context, args.client, &state)?;
		persist_installed_skill(
			&context.home,
			&state_path,
			&mut state,
			installed.component,
			installed.skill_rollback,
		)?;
		writeln!(stdout, "skill: {}", installed.message)?;
	}
	if components.contains(&AgentComponent::Mcp) {
		let installed = install_mcp(&context, args.client, &state)?;
		persist_installed_mcp(
			&context.home,
			&state_path,
			&mut state,
			installed.component,
			installed.config_rollback,
		)?;
		writeln!(stdout, "mcp: {}", installed.message)?;
	}
	if components.contains(&AgentComponent::Hooks) {
		let hook_policy = if preserve_hook_policy {
			HookPolicy::from_component(
				state
					.components
					.get("hooks")
					.context("hooks are not installed; use `agent install` to create them")?,
			)
		} else {
			HookPolicy::from_args(args)
		};
		let managed_path = state
			.components
			.get("hooks")
			.filter(|component| component.owned)
			.map(|component| Path::new(&component.path));
		let hook_args = HookInstallArgs {
			root: context.root.to_path_buf(),
			rules: hook_policy.rules.to_path_buf(),
			profile: hook_policy.profile.clone(),
			scope: hook_policy.check_scope.to_path_buf(),
			max_violations: hook_policy.max_violations,
		};
		let mut hook_output = Vec::new();
		let installed =
			hooks::install_for_agent(&hook_args, args.client, managed_path, &mut hook_output)?;
		let component = hook_component(&installed, hook_policy, state.components.get("hooks"));
		persist_installed_hook(
			&context.home,
			&state_path,
			&mut state,
			component,
			installed.rollback,
		)?;
		stdout.write_all(&hook_output)?;
		write_hook_activation_note(stdout, args.client, true)?;
	}

	writeln!(
		stdout,
		"integration: {} ({})",
		state_path.display(),
		env!("CARGO_PKG_VERSION")
	)?;
	Ok(true)
}

fn hook_component(
	installed: &hooks::AgentHookInstallation,
	policy: HookPolicy,
	previous: Option<&ComponentState>,
) -> ComponentState {
	let retains_config_ownership = previous.is_some_and(|component| {
		component.config_created
			&& component.config_checksum.as_deref() == installed.previous_config_checksum.as_deref()
	});
	ComponentState {
		scope: "project".to_string(),
		path: installed.path.display().to_string(),
		checksum: checksum_bytes(&installed.fingerprint),
		owned: installed.owned,
		version: env!("CARGO_PKG_VERSION").to_string(),
		profile: policy.profile,
		rules: Some(policy.rules.display().to_string()),
		check_scope: Some(policy.check_scope.display().to_string()),
		max_violations: Some(policy.max_violations),
		config_created: retains_config_ownership || installed.config_created,
		config_parent_created: previous.is_some_and(|component| component.config_parent_created)
			|| installed.config_parent_created,
		hook_directory_created: previous.is_some_and(|component| component.hook_directory_created)
			|| installed.hook_directory_created,
		config_checksum: installed.config_checksum.clone(),
	}
}

fn resolved_install_components(requested: &[AgentComponent]) -> BTreeSet<AgentComponent> {
	if !requested.is_empty() {
		return requested.iter().copied().collect();
	}
	let mut components = BTreeSet::from([AgentComponent::Skill]);
	if cfg!(feature = "mcp") {
		components.insert(AgentComponent::Mcp);
	}
	components
}

fn persist_installed_hook(
	home: &Path,
	state_path: &Path,
	state: &mut InstallState,
	component: ComponentState,
	rollback: Option<hooks::AgentHookRollback>,
) -> anyhow::Result<()> {
	let previous = state.components.insert("hooks".to_string(), component);
	if let Err(error) = persist_state(home, state_path, state) {
		if let Some(previous) = previous {
			state.components.insert("hooks".to_string(), previous);
		} else {
			state.components.remove("hooks");
		}
		if let Some(rollback) = rollback
			&& let Err(rollback_error) = hooks::rollback_agent_hook(&rollback)
		{
			bail!(
				"{error:#}; additionally failed to roll back the hook installation: {rollback_error:#}"
			);
		}
		return Err(error);
	}
	Ok(())
}

fn persist_installed_skill(
	home: &Path,
	state_path: &Path,
	state: &mut InstallState,
	component: ComponentState,
	rollback: Option<physical_skill::Mutation>,
) -> anyhow::Result<()> {
	let previous = state.components.insert("skill".to_string(), component);
	if let Err(error) = persist_state(home, state_path, state) {
		if let Some(previous) = previous {
			state.components.insert("skill".to_string(), previous);
		} else {
			state.components.remove("skill");
		}
		if let Some(rollback) = &rollback
			&& let Err(rollback_error) = physical_skill::rollback(home, rollback)
		{
			bail!(
				"{error:#}; additionally failed to roll back the skill installation: {rollback_error:#}"
			);
		}
		return Err(error);
	}
	Ok(())
}

fn persist_installed_mcp(
	home: &Path,
	state_path: &Path,
	state: &mut InstallState,
	component: ComponentState,
	rollback: Option<physical_config::Mutation>,
) -> anyhow::Result<()> {
	let previous = state.components.insert("mcp".to_string(), component);
	if let Err(error) = persist_state(home, state_path, state) {
		if let Some(previous) = previous {
			state.components.insert("mcp".to_string(), previous);
		} else {
			state.components.remove("mcp");
		}
		if let Some(rollback) = &rollback
			&& let Err(rollback_error) = physical_config::rollback(rollback)
		{
			bail!(
				"{error:#}; additionally failed to roll back the MCP configuration: {rollback_error:#}"
			);
		}
		return Err(error);
	}
	Ok(())
}

fn status<W: Write>(args: &AgentInspectArgs, stdout: &mut W) -> anyhow::Result<()> {
	let context = install_context(&args.root)?;
	let path = state_path(&context, args.client);
	let _state_lock = crate::fs_nofollow::lock_exclusive(&context.home, &state_lock_path(&path))?;
	let Some(state) = read_state(&context.home, &path)? else {
		writeln!(
			stdout,
			"No managed {} integration for `{}`.",
			client_name(args.client),
			context.root.display()
		)?;
		return Ok(());
	};
	validate_state_identity(&state, &context, args.client)?;
	writeln!(
		stdout,
		"client  component  scope    state      version  location"
	)?;
	for (name, component) in &state.components {
		let current = component_status(&context.home, &context.root, name, component, args.client);
		writeln!(
			stdout,
			"{:<7} {:<10} {:<8} {:<10} {:<8} {}",
			client_name(args.client),
			name,
			component.scope,
			current,
			component.version,
			component.path
		)?;
	}
	Ok(())
}

fn doctor<W: Write>(args: &AgentInspectArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let context = install_context(&args.root)?;
	let path = state_path(&context, args.client);
	let _state_lock = crate::fs_nofollow::lock_exclusive(&context.home, &state_lock_path(&path))?;
	let Some(state) = read_state(&context.home, &path)? else {
		writeln!(
			stdout,
			"problem: no managed {} integration for `{}`",
			client_name(args.client),
			context.root.display()
		)?;
		writeln!(
			stdout,
			"fix: code-moniker agent install --client {} {}",
			client_name(args.client),
			shell_arg(&context.root.display().to_string())
		)?;
		return Ok(false);
	};
	validate_state_identity(&state, &context, args.client)?;

	let mut problems = Vec::new();
	let mut reinstall = BTreeSet::new();
	let mut forget_external = BTreeSet::new();
	let mut hooks_coherent = false;
	for (name, component) in &state.components {
		let status = component_status(&context.home, &context.root, name, component, args.client);
		let status_problem = status != "installed" && status != "external";
		if name == "hooks" && !status_problem {
			hooks_coherent = true;
		}
		let version_problem = component.version != env!("CARGO_PKG_VERSION");
		if status == "outdated" {
			problems.push(format!("skill update available at {}", component.path));
		} else if status_problem {
			problems.push(format!("{name} is {status} at {}", component.path));
		}
		if version_problem {
			problems.push(format!(
				"{name} is version {}, binary is {}",
				display_component_version(&component.version),
				env!("CARGO_PKG_VERSION")
			));
		}
		if name == "mcp" && !cfg!(feature = "mcp") {
			problems.push("this binary was built without MCP support".to_string());
		} else if status_problem || version_problem {
			if component.owned {
				reinstall.insert(name.as_str());
			} else {
				forget_external.insert(name.as_str());
			}
		}
	}
	write_hook_activation_note(stdout, args.client, hooks_coherent)?;

	if problems.is_empty() {
		writeln!(
			stdout,
			"ok: {} integration is coherent for `{}`",
			client_name(args.client),
			context.root.display()
		)?;
		return Ok(true);
	}
	for problem in problems {
		writeln!(stdout, "problem: {problem}")?;
	}
	for name in reinstall {
		let component = &state.components[name];
		write_component_repair(stdout, args.client, &context.root, name, component)?;
	}
	for name in forget_external {
		let component = &state.components[name];
		writeln!(
			stdout,
			"fix: restore external {name} at {}, or forget it with `code-moniker agent uninstall --client {} --components {name} {}`",
			component.path,
			client_name(args.client),
			shell_arg(&context.root.display().to_string())
		)?;
	}
	if state.components.contains_key("mcp") && !cfg!(feature = "mcp") {
		writeln!(
			stdout,
			"fix: reinstall code-moniker with MCP support before repairing the mcp component"
		)?;
	}
	Ok(false)
}

fn write_hook_activation_note<W: Write>(
	stdout: &mut W,
	client: AgentClient,
	hooks_installed: bool,
) -> anyhow::Result<()> {
	if client == AgentClient::Codex && hooks_installed {
		writeln!(
			stdout,
			"action: approve this project hook in Codex app Settings; CLI status and doctor cannot observe app approval"
		)?;
	}
	Ok(())
}

fn write_component_repair<W: Write>(
	stdout: &mut W,
	client: AgentClient,
	root: &Path,
	name: &str,
	component: &ComponentState,
) -> anyhow::Result<()> {
	write!(
		stdout,
		"fix: code-moniker agent install --client {} --components {name}",
		client_name(client)
	)?;
	if name == "hooks"
		&& let Some(profile) = &component.profile
	{
		write!(stdout, " --profile {}", shell_arg(profile))?;
	}
	if name == "hooks" {
		let policy = HookPolicy::from_component(component);
		write!(
			stdout,
			" --rules {} --check-scope {} --max-violations {}",
			shell_arg(&policy.rules.display().to_string()),
			shell_arg(&policy.check_scope.display().to_string()),
			policy.max_violations
		)?;
	}
	writeln!(stdout, " {}", shell_arg(&root.display().to_string()))?;
	Ok(())
}

fn shell_arg(value: &str) -> String {
	if !value.is_empty()
		&& value
			.chars()
			.all(|character| character.is_ascii_alphanumeric() || "/._-".contains(character))
	{
		return value.to_string();
	}
	format!("'{}'", value.replace('\'', r#"'\''"#))
}

enum ComponentRollback {
	Hook(hooks::AgentHookRollback),
	Mcp(physical_config::Mutation),
	Skill(physical_skill::Mutation),
}

fn uninstall<W: Write>(args: &AgentUninstallArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let context = install_context(&args.root)?;
	let path = state_path(&context, args.client);
	let _state_lock = crate::fs_nofollow::lock_exclusive(&context.home, &state_lock_path(&path))?;
	let Some(mut state) = read_state(&context.home, &path)? else {
		writeln!(
			stdout,
			"No managed {} integration for `{}`.",
			client_name(args.client),
			context.root.display()
		)?;
		return Ok(false);
	};
	validate_state_identity(&state, &context, args.client)?;

	let requested: BTreeSet<String> = if args.components.is_empty() {
		state.components.keys().cloned().collect()
	} else {
		args.components
			.iter()
			.map(|component| component_name(*component).to_string())
			.collect()
	};
	for name in &requested {
		let component = state
			.components
			.get(name)
			.with_context(|| format!("component `{name}` is not managed for this integration"))?;
		if !component.owned {
			continue;
		}
		let status = component_status(&context.home, &context.root, name, component, args.client);
		if status != "installed" && status != "external" {
			bail!(
				"refusing to remove {name}: managed content is {status} at `{}`",
				component.path
			);
		}
	}

	for name in &requested {
		let component =
			state.components.get(name).cloned().with_context(|| {
				format!("component `{name}` disappeared after uninstall preflight")
			})?;
		let mut rollback = None;
		let message = if component.owned {
			match name.as_str() {
				"skill" => {
					if shared_skill_is_referenced(&context.home, args.client, &path, &component)? {
						"skill: retained shared component".to_string()
					} else {
						rollback = Some(ComponentRollback::Skill(uninstall_skill(
							&context.home,
							Path::new(&component.path),
						)?));
						format!("{name}: removed managed component")
					}
				}
				"mcp" => {
					rollback = Some(ComponentRollback::Mcp(uninstall_mcp(
						&context.root,
						Path::new(&component.path),
						args.client,
						&component,
					)?));
					format!("{name}: removed managed component")
				}
				"hooks" => {
					let expected_fingerprint =
						hooks::agent_hook_fingerprint(args.client, Path::new(&component.path))?;
					if checksum_bytes(&expected_fingerprint) != component.checksum {
						bail!(
							"refusing to remove hooks: managed content changed after uninstall preflight at `{}`",
							component.path
						);
					}
					rollback = Some(ComponentRollback::Hook(
						hooks::uninstall_for_agent_with_policy(
							&context.root,
							args.client,
							Path::new(&component.path),
							&expected_fingerprint,
							hooks::AgentHookRemovalPolicy {
								config_created: component.config_created,
								config_parent_created: component.config_parent_created,
								hook_directory_created: component.hook_directory_created,
								config_checksum: component.config_checksum.as_deref(),
							},
						)?,
					));
					format!("{name}: removed managed component")
				}
				_ => bail!("unknown managed component `{name}`"),
			}
		} else {
			format!("{name}: retained pre-existing external component")
		};
		persist_removed_component(&path, &context.home, &mut state, name, component, rollback)?;
		writeln!(stdout, "{message}")?;
	}
	Ok(true)
}

fn persist_removed_component(
	state_path: &Path,
	home: &Path,
	state: &mut InstallState,
	name: &str,
	component: ComponentState,
	rollback: Option<ComponentRollback>,
) -> anyhow::Result<()> {
	state.components.remove(name);
	let persist_result = if state.components.is_empty() {
		(|| -> anyhow::Result<()> {
			let contents = state.persisted_contents.as_deref().with_context(|| {
				format!(
					"agent state `{}` has no persisted snapshot",
					state_path.display()
				)
			})?;
			crate::fs_nofollow::remove_if_unchanged(
				home,
				state_path,
				contents,
				state.persisted_mode,
			)?;
			Ok(())
		})()
		.map(|()| {
			state.persisted_contents = None;
			state.persisted_mode = None;
			remove_empty_parents(state_path.parent(), home.join(".code-moniker"));
		})
	} else {
		persist_state(home, state_path, state)
	};
	if let Err(error) = persist_result {
		state.components.insert(name.to_string(), component);
		let rollback_result = match rollback {
			Some(ComponentRollback::Hook(rollback)) => hooks::rollback_agent_hook(&rollback),
			Some(ComponentRollback::Mcp(rollback)) => physical_config::rollback(&rollback),
			Some(ComponentRollback::Skill(rollback)) => physical_skill::rollback(home, &rollback),
			None => Ok(()),
		};
		if let Err(rollback_error) = rollback_result {
			bail!("{error:#}; additionally failed to roll back removal: {rollback_error:#}");
		}
		return Err(error);
	}
	Ok(())
}

struct InstalledComponent {
	component: ComponentState,
	message: String,
	config_rollback: Option<physical_config::Mutation>,
	skill_rollback: Option<physical_skill::Mutation>,
}

fn install_skill(
	context: &InstallContext,
	client: AgentClient,
	state: &InstallState,
) -> anyhow::Result<InstalledComponent> {
	let path = user_skill_path(&context.home, client);
	physical_skill::ensure_parent(&context.home, &path)?;
	let existing = fs::symlink_metadata(&path).is_ok();
	let is_symlink = fs::symlink_metadata(&path)
		.map(|metadata| metadata.file_type().is_symlink())
		.unwrap_or(false);
	let managed_here = state
		.components
		.get("skill")
		.is_some_and(|component| component.owned);
	let current_state_path = state_path(context, client);
	let shared_owner = if managed_here {
		None
	} else {
		find_owned_skill_state(&context.home, &current_state_path, client, &path)?
	};
	let managed = managed_here || shared_owner.is_some();
	let matched_before = physical_skill::matches(&context.home, &path);
	if is_symlink {
		bail!(
			"refusing linked skill `{}`; remove it before installing a physical skill",
			path.display()
		);
	}
	if existing && !managed && !matched_before {
		bail!(
			"refusing to replace unmanaged skill `{}`; move it aside or remove it explicitly",
			path.display()
		);
	}
	if existing {
		physical_skill::ensure_replaceable(&path)?;
		let metadata = fs::symlink_metadata(&path)
			.with_context(|| format!("cannot inspect skill `{}`", path.display()))?;
		if metadata.is_dir() {
			fs::remove_dir_all(&path)
				.with_context(|| format!("cannot replace skill `{}`", path.display()))?;
		} else {
			fs::remove_file(&path)
				.with_context(|| format!("cannot replace skill `{}`", path.display()))?;
		}
	}
	let skill_rollback = physical_skill::write_assets(&context.home, &path)?;
	Ok(InstalledComponent {
		component: ComponentState {
			scope: "user".to_string(),
			path: path.display().to_string(),
			checksum: skill_checksum(),
			owned: true,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: None,
			check_scope: None,
			max_violations: None,
			config_created: false,
			config_parent_created: false,
			hook_directory_created: false,
			config_checksum: None,
		},
		message: format!(
			"installed {} embedded files at {}",
			SKILL_FILES.len(),
			path.display()
		),
		config_rollback: None,
		skill_rollback: Some(skill_rollback),
	})
}

fn install_mcp(
	context: &InstallContext,
	client: AgentClient,
	state: &InstallState,
) -> anyhow::Result<InstalledComponent> {
	let config_path = mcp_config_path(&context.root, client);
	let command = context.binary.display().to_string();
	let args = vec![
		"mcp".to_string(),
		context.root.display().to_string(),
		"--transport".to_string(),
		"stdio".to_string(),
		"--live-refresh".to_string(),
		"auto".to_string(),
	];
	let managed = state
		.components
		.get("mcp")
		.is_some_and(|component| component.owned);
	let (owned, config_rollback, observed_config) = match client {
		AgentClient::Codex => {
			install_codex_mcp(&context.root, &config_path, &command, &args, managed)?
		}
		AgentClient::Claude | AgentClient::Gemini => install_json_mcp(
			&context.root,
			&config_path,
			&command,
			&args,
			managed,
			client,
		)?,
	};
	let committed_config = config_rollback
		.as_ref()
		.and_then(physical_config::Mutation::committed_contents)
		.or(observed_config.as_deref());
	let checksum = match committed_config
		.with_context(|| format!("MCP configuration `{}` is missing", config_path.display()))
		.and_then(|contents| mcp_entry_checksum_contents(contents, &config_path, client))
	{
		Ok(checksum) => checksum,
		Err(error) => {
			if let Some(config_rollback) = &config_rollback
				&& let Err(rollback_error) = physical_config::rollback(config_rollback)
			{
				bail!(
					"{error:#}; additionally failed to roll back the MCP configuration: {rollback_error:#}"
				);
			}
			return Err(error);
		}
	};
	let observed_config_checksum = observed_config.as_deref().map(checksum_bytes);
	let committed_config_checksum = match &config_rollback {
		Some(mutation) => mutation.committed_contents().map(checksum_bytes),
		None => observed_config_checksum.clone(),
	};
	let previous_mcp = state.components.get("mcp");
	let retains_config_ownership = previous_mcp.is_some_and(|component| {
		component.config_created
			&& component.config_checksum.as_deref() == observed_config_checksum.as_deref()
	});
	Ok(InstalledComponent {
		component: ComponentState {
			scope: "project".to_string(),
			path: config_path.display().to_string(),
			checksum,
			owned,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: None,
			check_scope: None,
			max_violations: None,
			config_created: retains_config_ownership
				|| config_rollback
					.as_ref()
					.is_some_and(physical_config::Mutation::created_file),
			config_parent_created: previous_mcp
				.is_some_and(|component| component.config_parent_created)
				|| config_rollback
					.as_ref()
					.is_some_and(physical_config::Mutation::created_parent),
			hook_directory_created: false,
			config_checksum: committed_config_checksum,
		},
		message: format!(
			"registered project-owned stdio server in {}",
			config_path.display()
		),
		config_rollback,
		skill_rollback: None,
	})
}

fn uninstall_skill(home: &Path, path: &Path) -> anyhow::Result<physical_skill::Mutation> {
	if fs::symlink_metadata(path)
		.map(|metadata| metadata.file_type().is_symlink())
		.unwrap_or(false)
	{
		bail!("refusing to remove linked skill `{}`", path.display());
	}
	physical_skill::remove(home, path)
}

fn uninstall_mcp(
	root: &Path,
	path: &Path,
	client: AgentClient,
	component: &ComponentState,
) -> anyhow::Result<physical_config::Mutation> {
	let snapshot = physical_config::snapshot(root, path)?;
	let current_checksum = snapshot.contents().map(checksum_bytes);
	if component.config_created
		&& current_checksum.is_some()
		&& current_checksum == component.config_checksum
	{
		return physical_config::remove(snapshot, component.config_parent_created);
	}
	let contents = match client {
		AgentClient::Codex => {
			let contents = snapshot
				.contents()
				.with_context(|| format!("MCP configuration `{}` is missing", path.display()))?;
			let mut config = std::str::from_utf8(contents)
				.with_context(|| format!("`{}` is not valid UTF-8", path.display()))?
				.parse::<toml::Value>()
				.with_context(|| format!("invalid TOML in `{}`", path.display()))?;
			if let Some(servers) = config
				.get_mut("mcp_servers")
				.and_then(toml::Value::as_table_mut)
			{
				servers.remove(SKILL_NAME);
			}
			toml::to_string_pretty(&config)?.into_bytes()
		}
		AgentClient::Claude | AgentClient::Gemini => {
			let mut config = read_json_object_contents(snapshot.contents(), path)?;
			if let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) {
				servers.remove(SKILL_NAME);
			}
			serde_json::to_vec_pretty(&Value::Object(config))?
		}
	};
	physical_config::write(snapshot, &contents)
}

fn install_codex_mcp(
	root: &Path,
	path: &Path,
	command: &str,
	args: &[String],
	managed: bool,
) -> anyhow::Result<(bool, Option<physical_config::Mutation>, Option<Vec<u8>>)> {
	let snapshot = physical_config::snapshot(root, path)?;
	let observed_config = snapshot.contents().map(ToOwned::to_owned);
	let mut config = if let Some(contents) = snapshot.contents() {
		std::str::from_utf8(contents)
			.with_context(|| format!("`{}` is not valid UTF-8", path.display()))?
			.parse::<toml::Value>()
			.with_context(|| format!("invalid TOML in `{}`", path.display()))?
	} else {
		toml::Value::Table(toml::map::Map::new())
	};
	let config_root = config
		.as_table_mut()
		.context("Codex configuration root must be a TOML table")?;
	let servers = config_root
		.entry("mcp_servers")
		.or_insert_with(|| toml::Value::Table(toml::map::Map::new()))
		.as_table_mut()
		.context("`mcp_servers` must be a TOML table")?;
	let expected = toml::Value::Table(toml::map::Map::from_iter([
		(
			"command".to_string(),
			toml::Value::String(command.to_string()),
		),
		(
			"args".to_string(),
			toml::Value::Array(
				args.iter()
					.map(|arg| toml::Value::String(arg.clone()))
					.collect(),
			),
		),
		(
			"required".to_string(),
			toml::Value::Boolean(CODEX_MCP_REQUIRED_FOR_CLIENT_STARTUP),
		),
		("startup_timeout_sec".to_string(), toml::Value::Integer(45)),
		("tool_timeout_sec".to_string(), toml::Value::Integer(120)),
	]));
	let existing = servers.get(SKILL_NAME);
	if let Some(existing) = existing
		&& existing != &expected
		&& !managed
	{
		bail!(
			"refusing to replace unmanaged MCP server `{SKILL_NAME}` in `{}`",
			path.display()
		);
	}
	let owned = managed || existing.is_none();
	if existing == Some(&expected) {
		return Ok((owned, None, observed_config));
	}
	servers.insert(SKILL_NAME.to_string(), expected);
	let contents = toml::to_string_pretty(&config)?;
	let mutation = physical_config::write(snapshot, contents.as_bytes())?;
	Ok((owned, Some(mutation), observed_config))
}

fn install_json_mcp(
	root: &Path,
	path: &Path,
	command: &str,
	args: &[String],
	managed: bool,
	client: AgentClient,
) -> anyhow::Result<(bool, Option<physical_config::Mutation>, Option<Vec<u8>>)> {
	let snapshot = physical_config::snapshot(root, path)?;
	let observed_config = snapshot.contents().map(ToOwned::to_owned);
	let mut config = read_json_object_contents(snapshot.contents(), path)?;
	let servers = config
		.entry("mcpServers".to_string())
		.or_insert_with(|| Value::Object(Map::new()))
		.as_object_mut()
		.context("`mcpServers` must be a JSON object")?;
	let expected = match client {
		AgentClient::Claude => json!({
			"type": "stdio",
			"command": command,
			"args": args,
			"env": {}
		}),
		AgentClient::Gemini => json!({
			"command": command,
			"args": args,
			"timeout": 120000
		}),
		AgentClient::Codex => unreachable!(),
	};
	let existing = servers.get(SKILL_NAME);
	if let Some(existing) = existing
		&& existing != &expected
		&& !managed
	{
		bail!(
			"refusing to replace unmanaged MCP server `{SKILL_NAME}` in `{}`",
			path.display()
		);
	}
	let owned = managed || existing.is_none();
	if existing == Some(&expected) {
		return Ok((owned, None, observed_config));
	}
	servers.insert(SKILL_NAME.to_string(), expected);
	let contents = serde_json::to_vec_pretty(&Value::Object(config))?;
	let mutation = physical_config::write(snapshot, &contents)?;
	Ok((owned, Some(mutation), observed_config))
}

#[cfg(test)]
fn read_json_object(root: &Path, path: &Path) -> anyhow::Result<Map<String, Value>> {
	let contents = physical_config::read(root, path)?;
	read_json_object_contents(contents.as_deref(), path)
}

fn read_json_object_contents(
	contents: Option<&[u8]>,
	path: &Path,
) -> anyhow::Result<Map<String, Value>> {
	let Some(contents) = contents else {
		return Ok(Map::new());
	};
	let value: Value = serde_json::from_slice(contents)
		.with_context(|| format!("invalid JSON in `{}`", path.display()))?;
	value
		.as_object()
		.cloned()
		.with_context(|| format!("`{}` must contain a JSON object", path.display()))
}

fn component_status(
	home: &Path,
	root: &Path,
	name: &str,
	component: &ComponentState,
	client: AgentClient,
) -> &'static str {
	let path = Path::new(&component.path);
	if name == "hooks" {
		return match hooks::agent_hook_fingerprint(client, path) {
			Ok(fingerprint) if checksum_bytes(&fingerprint) == component.checksum => {
				if component.owned {
					"installed"
				} else {
					"external"
				}
			}
			Ok(_) => "stale",
			Err(_) => match hooks::agent_hook_is_missing(client, path) {
				Ok(true) => "missing",
				Ok(false) | Err(_) => "stale",
			},
		};
	}
	if name == "skill" {
		match physical_skill::exists(home, path) {
			Err(_) => return "stale",
			Ok(false) => return "missing",
			Ok(true) => {}
		}
		if physical_skill::matches(home, path) {
			return if component.owned {
				"installed"
			} else {
				"external"
			};
		}
		if component.checksum != skill_checksum() {
			return "outdated";
		}
		return "stale";
	}
	if name == "mcp" {
		return match mcp_entry_checksum(root, path, client) {
			Ok(checksum) if checksum == component.checksum && component.owned => "installed",
			Ok(checksum) if checksum == component.checksum => "external",
			Ok(_) => "stale",
			Err(_) => match crate::fs_nofollow::exists(root, path) {
				Ok(false) => "missing",
				Ok(true) | Err(_) => "stale",
			},
		};
	}
	if !path.exists() {
		return "missing";
	}
	"unknown"
}

fn mcp_entry_checksum(root: &Path, path: &Path, client: AgentClient) -> anyhow::Result<String> {
	let contents = physical_config::read(root, path)?
		.with_context(|| format!("MCP configuration `{}` is missing", path.display()))?;
	mcp_entry_checksum_contents(&contents, path, client)
}

fn mcp_entry_checksum_contents(
	contents: &[u8],
	path: &Path,
	client: AgentClient,
) -> anyhow::Result<String> {
	match client {
		AgentClient::Codex => {
			let config = std::str::from_utf8(contents)
				.with_context(|| format!("`{}` is not valid UTF-8", path.display()))?
				.parse::<toml::Value>()
				.with_context(|| format!("invalid TOML in `{}`", path.display()))?;
			let entry = config
				.get("mcp_servers")
				.and_then(toml::Value::as_table)
				.and_then(|servers| servers.get(SKILL_NAME))
				.context("Code Moniker MCP entry is missing")?;
			Ok(checksum_bytes(toml::to_string(entry)?.as_bytes()))
		}
		AgentClient::Claude | AgentClient::Gemini => {
			let config = read_json_object_contents(Some(contents), path)?;
			let entry = config
				.get("mcpServers")
				.and_then(Value::as_object)
				.and_then(|servers| servers.get(SKILL_NAME))
				.context("Code Moniker MCP entry is missing")?;
			Ok(checksum_bytes(&serde_json::to_vec(entry)?))
		}
	}
}

fn skill_checksum() -> String {
	checksum_parts(
		SKILL_FILES
			.iter()
			.flat_map(|(path, contents)| [*path, *contents]),
	)
}

fn checksum_parts<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
	let mut hash = 0xcbf29ce484222325_u64;
	for part in parts {
		for byte in part.as_bytes() {
			hash ^= u64::from(*byte);
			hash = hash.wrapping_mul(0x100000001b3);
		}
		hash ^= 0xff;
		hash = hash.wrapping_mul(0x100000001b3);
	}
	format!("{hash:016x}")
}

fn checksum_bytes(bytes: &[u8]) -> String {
	let mut hash = 0xcbf29ce484222325_u64;
	for byte in bytes {
		hash ^= u64::from(*byte);
		hash = hash.wrapping_mul(0x100000001b3);
	}
	format!("{hash:016x}")
}

fn install_context(root: &Path) -> anyhow::Result<InstallContext> {
	let root = root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", root.display()))?;
	let home = std::env::var_os("CODE_MONIKER_AGENT_HOME")
		.or_else(|| std::env::var_os("HOME"))
		.filter(|value| !value.is_empty())
		.map(PathBuf::from)
		.context(
			"neither CODE_MONIKER_AGENT_HOME nor HOME is set; cannot resolve the user integration directory",
		)?
		.canonicalize()
		.context("cannot resolve the user integration directory to a physical path")?;
	let binary =
		std::env::current_exe().context("cannot resolve the current code-moniker executable")?;
	Ok(InstallContext { home, binary, root })
}

fn user_skill_path(home: &Path, client: AgentClient) -> PathBuf {
	home.join(match client {
		AgentClient::Codex => ".codex/skills",
		AgentClient::Claude => ".claude/skills",
		AgentClient::Gemini => ".gemini/skills",
	})
	.join(SKILL_NAME)
}

fn mcp_config_path(root: &Path, client: AgentClient) -> PathBuf {
	match client {
		AgentClient::Codex => root.join(".codex/config.toml"),
		AgentClient::Claude => root.join(".mcp.json"),
		AgentClient::Gemini => root.join(".gemini/settings.json"),
	}
}

fn state_path(context: &InstallContext, client: AgentClient) -> PathBuf {
	let root_hash = checksum_parts([context.root.to_string_lossy().as_ref()]);
	context
		.home
		.join(".code-moniker/agent")
		.join(client_name(client))
		.join(format!("{root_hash}.json"))
}

fn state_lock_path(state_path: &Path) -> PathBuf {
	state_path.with_file_name(".lock")
}

fn client_name(client: AgentClient) -> &'static str {
	match client {
		AgentClient::Codex => "codex",
		AgentClient::Claude => "claude",
		AgentClient::Gemini => "gemini",
	}
}

fn component_name(component: AgentComponent) -> &'static str {
	match component {
		AgentComponent::Skill => "skill",
		AgentComponent::Mcp => "mcp",
		AgentComponent::Hooks => "hooks",
	}
}

fn validate_state_identity(
	state: &InstallState,
	context: &InstallContext,
	client: AgentClient,
) -> anyhow::Result<()> {
	if state.schema != STATE_SCHEMA {
		bail!(
			"unsupported agent integration state schema {} (expected {STATE_SCHEMA})",
			state.schema
		);
	}
	if state.client != client_name(client) || state.root != context.root.display().to_string() {
		bail!("agent integration state identity does not match the requested client and root");
	}
	Ok(())
}

fn read_state(home: &Path, path: &Path) -> anyhow::Result<Option<InstallState>> {
	let Some(contents) = crate::fs_nofollow::read(home, path)? else {
		return Ok(None);
	};
	let mode = crate::fs_nofollow::mode(home, path)?;
	let mut state: InstallState = serde_json::from_slice(&contents)
		.with_context(|| format!("invalid agent integration state `{}`", path.display()))?;
	state.persisted_contents = Some(contents);
	state.persisted_mode = mode;
	Ok(Some(state))
}

fn persist_state(home: &Path, path: &Path, state: &mut InstallState) -> anyhow::Result<()> {
	state.version = env!("CARGO_PKG_VERSION").to_string();
	let contents = serde_json::to_vec_pretty(state)?;
	let parent = path
		.parent()
		.with_context(|| format!("agent state `{}` has no parent", path.display()))?;
	crate::fs_nofollow::ensure_dir(home, parent)?;
	crate::fs_nofollow::write_if_unchanged(
		home,
		path,
		state.persisted_contents.as_deref(),
		state.persisted_mode,
		&contents,
		Some(state.persisted_mode.unwrap_or(0o600)),
	)?;
	state.persisted_contents = Some(contents);
	state.persisted_mode = Some(state.persisted_mode.unwrap_or(0o600));
	Ok(())
}

fn display_component_version(version: &str) -> &str {
	if version.is_empty() {
		"unknown"
	} else {
		version
	}
}

fn find_owned_skill_state(
	home: &Path,
	current_state_path: &Path,
	client: AgentClient,
	skill_path: &Path,
) -> anyhow::Result<Option<PathBuf>> {
	let Some(state_dir) = current_state_path.parent() else {
		return Ok(None);
	};
	if !state_dir.exists() {
		return Ok(None);
	}
	let mut candidates = fs::read_dir(state_dir)
		.with_context(|| format!("cannot inspect `{}`", state_dir.display()))?
		.collect::<Result<Vec<_>, _>>()
		.with_context(|| format!("cannot inspect `{}`", state_dir.display()))?;
	candidates.sort_by_key(|entry| entry.path());
	for candidate in candidates {
		let candidate_path = candidate.path();
		if candidate_path == current_state_path
			|| candidate_path.extension().and_then(|value| value.to_str()) != Some("json")
		{
			continue;
		}
		let Some(other) = read_state(home, &candidate_path)? else {
			continue;
		};
		let owns_skill = other.client == client_name(client)
			&& other.components.get("skill").is_some_and(|component| {
				component.owned && Path::new(&component.path) == skill_path
			});
		if owns_skill {
			return Ok(Some(candidate_path));
		}
	}
	Ok(None)
}

fn shared_skill_is_referenced(
	home: &Path,
	client: AgentClient,
	current_state_path: &Path,
	component: &ComponentState,
) -> anyhow::Result<bool> {
	let Some(state_dir) = current_state_path.parent() else {
		return Ok(false);
	};
	if !state_dir.exists() {
		return Ok(false);
	}
	let mut candidates = fs::read_dir(state_dir)
		.with_context(|| format!("cannot inspect `{}`", state_dir.display()))?
		.collect::<Result<Vec<_>, _>>()
		.with_context(|| format!("cannot inspect `{}`", state_dir.display()))?;
	candidates.sort_by_key(|entry| entry.path());
	for candidate in candidates {
		let candidate_path = candidate.path();
		if candidate_path == current_state_path
			|| candidate_path.extension().and_then(|value| value.to_str()) != Some("json")
		{
			continue;
		}
		let Some(other) = read_state(home, &candidate_path)? else {
			continue;
		};
		if other.client != client_name(client) {
			continue;
		}
		let Some(other_skill) = other.components.get("skill") else {
			continue;
		};
		if other_skill.path != component.path {
			continue;
		}
		return Ok(true);
	}
	Ok(false)
}

fn remove_empty_parents(mut current: Option<&Path>, stop: PathBuf) {
	while let Some(path) = current {
		if path == stop || !path.starts_with(&stop) || fs::remove_dir(path).is_err() {
			break;
		}
		current = path.parent();
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use clap::Parser;
	use pulldown_cmark::{Event, Parser as MarkdownParser, Tag};
	use tempfile::tempdir;

	use crate::Cli;

	fn write_test_file(path: &Path, contents: &[u8]) {
		fs::create_dir_all(path.parent().unwrap()).unwrap();
		fs::write(path, contents).unwrap();
	}

	fn collect_skill_asset_paths(root: &Path, directory: &Path, paths: &mut BTreeSet<String>) {
		for entry in fs::read_dir(directory).unwrap() {
			let path = entry.unwrap().path();
			if fs::metadata(&path).unwrap().is_dir() {
				collect_skill_asset_paths(root, &path, paths);
			} else {
				let relative = path.strip_prefix(root).unwrap();
				let relative = relative
					.components()
					.map(|component| component.as_os_str().to_string_lossy())
					.collect::<Vec<_>>()
					.join("/");
				paths.insert(relative);
			}
		}
	}

	fn resolve_skill_reference(owner: &str, reference: &str) -> String {
		let mut parts = Vec::new();
		let parent = Path::new(owner).parent().unwrap_or_else(|| Path::new(""));
		let referenced_path = parent.join(reference);
		for component in referenced_path.components() {
			match component {
				std::path::Component::Normal(part) => {
					parts.push(part.to_string_lossy().into_owned())
				}
				std::path::Component::CurDir => {}
				std::path::Component::ParentDir => {
					if parts.pop().is_none() {
						panic!("reference escapes the skill root");
					}
				}
				std::path::Component::RootDir | std::path::Component::Prefix(_) => {
					panic!("local skill reference must be relative")
				}
			}
		}
		parts.join("/")
	}

	fn has_uri_scheme(value: &str) -> bool {
		let Some((scheme, _)) = value.split_once(':') else {
			return false;
		};
		let mut characters = scheme.chars();
		characters
			.next()
			.is_some_and(|first| first.is_ascii_alphabetic())
			&& characters.all(|character| {
				character.is_ascii_alphanumeric() || matches!(character, '+' | '-' | '.')
			})
	}

	fn local_markdown_reference(value: &str) -> Option<&str> {
		let value = value.trim();
		if value.is_empty()
			|| value.starts_with('/')
			|| value.starts_with('#')
			|| has_uri_scheme(value)
		{
			return None;
		}
		let without_fragment = value.split_once('#').map_or(value, |(path, _)| path);
		let path = without_fragment
			.split_once('?')
			.map_or(without_fragment, |(path, _)| path);
		Path::new(path)
			.extension()
			.is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
			.then_some(path)
	}

	fn markdown_asset_references(contents: &str) -> BTreeSet<String> {
		MarkdownParser::new(contents)
			.filter_map(|event| match event {
				Event::Code(candidate)
				| Event::Start(Tag::Link {
					dest_url: candidate,
					..
				}) => local_markdown_reference(&candidate).map(ToOwned::to_owned),
				_ => None,
			})
			.collect()
	}

	#[test]
	fn markdown_asset_references_only_collect_relative_markdown_targets() {
		let markdown = r#"
[local](references/rules.md#taxonomy)
[external](https://example.org/README.md)
`architecture.md`
`référence avec espaces.MD`
plain.md
`code-moniker rules show .`
"#;

		assert_eq!(
			markdown_asset_references(markdown),
			BTreeSet::from([
				"architecture.md".to_string(),
				"references/rules.md".to_string(),
				"référence avec espaces.MD".to_string(),
			])
		);
	}

	#[test]
	fn embedded_skill_inventory_covers_every_packaged_asset() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/agent/code-moniker");
		let mut packaged = BTreeSet::new();
		collect_skill_asset_paths(&root, &root, &mut packaged);
		let embedded = SKILL_FILES
			.iter()
			.map(|(relative, _)| (*relative).to_string())
			.collect::<BTreeSet<_>>();

		assert_eq!(embedded, packaged);
	}

	#[test]
	fn skill_router_lists_every_learn_topic() {
		let skill = SKILL_FILES
			.iter()
			.find_map(|(relative, contents)| (*relative == "SKILL.md").then_some(*contents))
			.expect("embedded SKILL.md");

		for topic in crate::rules::learn_topic_names() {
			let command = format!("code-moniker rules learn {topic}");
			assert!(
				skill.contains(&command),
				"skill router must list `{command}`"
			);
		}
	}

	#[test]
	fn packaged_skill_assets_match_canonical_tree_in_checkout() {
		let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
		let workspace = manifest.join("../..");
		if !workspace.join("Cargo.toml").is_file() {
			return;
		}
		let canonical = workspace.join("agents/skills/code-moniker");
		let packaged = manifest.join("assets/agent/code-moniker");
		let mut canonical_paths = BTreeSet::new();
		let mut packaged_paths = BTreeSet::new();
		collect_skill_asset_paths(&canonical, &canonical, &mut canonical_paths);
		collect_skill_asset_paths(&packaged, &packaged, &mut packaged_paths);

		assert_eq!(canonical_paths, packaged_paths);
		for relative in canonical_paths {
			assert_eq!(
				fs::read(canonical.join(&relative)).unwrap(),
				fs::read(packaged.join(&relative)).unwrap(),
				"packaged skill asset `{relative}` differs from its canonical source"
			);
		}
	}

	#[test]
	fn embedded_skill_resolves_every_local_markdown_reference() {
		let embedded = SKILL_FILES
			.iter()
			.map(|(relative, _)| *relative)
			.collect::<BTreeSet<_>>();

		for (owner, contents) in SKILL_FILES {
			for referenced in markdown_asset_references(contents) {
				let resolved = resolve_skill_reference(owner, &referenced);
				assert!(
					embedded.contains(resolved.as_str()),
					"{owner} references missing embedded asset {} (resolved as {resolved})",
					referenced,
				);
			}
		}
	}

	#[test]
	fn agent_install_defaults_follow_the_binary_capabilities_without_a_rules_profile() {
		let cli =
			Cli::try_parse_from(["code-moniker", "agent", "install", "--client", "codex"]).unwrap();
		let crate::Command::Agent(args) = cli.command else {
			panic!("expected agent command");
		};
		let AgentCommand::Install(args) = args.command else {
			panic!("expected install command");
		};
		assert!(args.components.is_empty());
		let resolved = resolved_install_components(&args.components);
		assert!(resolved.contains(&AgentComponent::Skill));
		assert_eq!(
			resolved.contains(&AgentComponent::Mcp),
			cfg!(feature = "mcp")
		);
		assert_eq!(args.profile, None);
	}

	#[test]
	fn json_mcp_install_preserves_unrelated_configuration() {
		let dir = tempdir().unwrap();
		let path = dir.path().join(".mcp.json");
		fs::write(
			&path,
			r#"{"permissions":{"allow":["Read"]},"mcpServers":{"other":{"command":"other"}}}"#,
		)
		.unwrap();
		install_json_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
			AgentClient::Claude,
		)
		.unwrap();
		let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		assert_eq!(config["permissions"]["allow"][0], "Read");
		assert_eq!(config["mcpServers"]["other"]["command"], "other");
		assert_eq!(
			config["mcpServers"]["code-moniker"]["command"],
			"/bin/code-moniker"
		);
		assert!(
			config["mcpServers"]["code-moniker"]
				.get("required")
				.is_none(),
			"the Claude generator must not emit Codex's fatal-startup flag"
		);
	}

	#[test]
	fn gemini_mcp_install_does_not_emit_a_fatal_startup_flag() {
		let dir = tempdir().unwrap();
		let path = dir.path().join("settings.json");
		install_json_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
			AgentClient::Gemini,
		)
		.unwrap();

		let config: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
		assert!(
			config["mcpServers"]["code-moniker"]
				.get("required")
				.is_none(),
			"the Gemini generator must not emit Codex's fatal-startup flag"
		);
	}

	#[test]
	fn matching_external_mcp_configuration_is_not_rewritten() {
		let dir = tempdir().unwrap();
		let path = dir.path().join(".mcp.json");
		let original = br#"{
  "mcpServers": {
    "code-moniker": { "type": "stdio", "command": "/bin/code-moniker", "args": ["mcp", "/project"], "env": {} }
  }
}
"#;
		fs::write(&path, original).unwrap();

		let (owned, mutation, observed) = install_json_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
			AgentClient::Claude,
		)
		.unwrap();

		assert!(!owned);
		assert!(mutation.is_none());
		assert_eq!(observed.as_deref(), Some(original.as_slice()));
		assert_eq!(fs::read(path).unwrap(), original);
	}

	#[test]
	fn mcp_install_refuses_a_change_after_its_parsed_snapshot() {
		let dir = tempdir().unwrap();
		let path = dir.path().join(".mcp.json");
		fs::write(&path, br#"{"before":true}"#).unwrap();
		let path_for_race = path.clone();
		physical_config::BEFORE_WRITE.with(|hook| {
			*hook.borrow_mut() = Some(Box::new(move |_| {
				fs::write(&path_for_race, br#"{"concurrent":true}"#).unwrap();
			}));
		});

		let error = install_json_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
			AgentClient::Claude,
		)
		.err()
		.unwrap()
		.to_string();

		assert!(!error.is_empty());
		assert_eq!(fs::read_to_string(path).unwrap(), r#"{"concurrent":true}"#);
	}

	#[test]
	fn codex_mcp_install_refuses_to_claim_an_unmanaged_entry() {
		let dir = tempdir().unwrap();
		let path = dir.path().join("config.toml");
		fs::write(
			&path,
			r#"[mcp_servers.code-moniker]
command = "other"
"#,
		)
		.unwrap();
		let error = install_codex_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
		)
		.unwrap_err();
		assert!(error.to_string().contains("refusing to replace unmanaged"));
	}

	#[test]
	fn codex_mcp_install_keeps_agent_sessions_available_when_mcp_startup_fails() {
		let dir = tempdir().unwrap();
		let path = dir.path().join("config.toml");
		install_codex_mcp(
			dir.path(),
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
		)
		.unwrap();

		let config = fs::read_to_string(path)
			.unwrap()
			.parse::<toml::Value>()
			.unwrap();
		assert_eq!(
			config["mcp_servers"][SKILL_NAME]["required"].as_bool(),
			Some(false)
		);
	}

	#[cfg(unix)]
	#[test]
	fn mcp_install_rejects_linked_config_without_touching_the_target() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let external = dir.path().join("external.json");
		fs::write(&external, r#"{"external":true}"#).unwrap();
		symlink(&external, dir.path().join(".mcp.json")).unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};

		let error = install_mcp(&context, AgentClient::Claude, &InstallState::default())
			.err()
			.unwrap()
			.to_string();

		assert!(!error.is_empty());
		assert_eq!(
			fs::read_to_string(external).unwrap(),
			r#"{"external":true}"#
		);
	}

	#[cfg(unix)]
	#[test]
	fn mcp_install_rejects_linked_parent_without_writing_outside_the_repo() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let external = dir.path().join("external");
		fs::create_dir(&external).unwrap();
		symlink(&external, dir.path().join(".codex")).unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};

		let error = install_mcp(&context, AgentClient::Codex, &InstallState::default())
			.err()
			.unwrap()
			.to_string();

		assert!(!error.is_empty());
		assert!(!external.join("config.toml").exists());
	}

	#[test]
	fn embedded_skill_install_is_complete_and_idempotent() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().to_path_buf(),
		};
		let first = install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(
			first
				.message
				.contains(&format!("installed {} embedded files", SKILL_FILES.len()))
		);
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		assert!(physical_skill::matches(&context.home, &skill));

		let mut managed = InstallState::default();
		managed
			.components
			.insert("skill".to_string(), first.component);
		let second = install_skill(&context, AgentClient::Codex, &managed).unwrap();
		assert!(
			second
				.message
				.contains(&format!("installed {} embedded files", SKILL_FILES.len()))
		);
		assert!(physical_skill::matches(&context.home, &skill));
	}

	#[test]
	fn managed_skill_reinstall_replaces_the_whole_directory() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().to_path_buf(),
		};
		let first = install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		write_test_file(&skill.join("references/legacy.md"), b"obsolete");
		write_test_file(&skill.join("local.md"), b"local customization");

		let mut managed = InstallState::default();
		managed
			.components
			.insert("skill".to_string(), first.component);
		install_skill(&context, AgentClient::Codex, &managed).unwrap();

		assert!(physical_skill::matches(&context.home, &skill));
		assert!(!skill.join("references/legacy.md").exists());
		assert!(!skill.join("local.md").exists());
	}

	#[test]
	fn skill_status_distinguishes_embedded_update_from_local_drift() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().to_path_buf(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = Path::new(&installed.component.path);
		fs::write(skill.join(SKILL_FILES[0].0), "older embedded skill").unwrap();

		let mut outdated = installed.component.clone();
		outdated.checksum = "previous-embedded-checksum".to_string();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&outdated,
				AgentClient::Codex,
			),
			"outdated"
		);

		let mut drifted = outdated;
		drifted.checksum = skill_checksum();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&drifted,
				AgentClient::Codex,
			),
			"stale"
		);
	}

	#[test]
	fn codex_mcp_install_uses_the_canonical_project_root() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_mcp(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"mcp",
				&installed.component,
				AgentClient::Codex,
			),
			"installed"
		);
		let config: toml::Value = fs::read_to_string(&installed.component.path)
			.unwrap()
			.parse()
			.unwrap();
		let args = config["mcp_servers"]["code-moniker"]["args"]
			.as_array()
			.unwrap();
		assert_eq!(args[1].as_str(), Some(context.root.to_str().unwrap()));
		assert_eq!(args[2].as_str(), Some("--transport"));
		assert_eq!(args[3].as_str(), Some("stdio"));
	}

	#[test]
	fn mcp_status_detects_configuration_drift() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_mcp(&context, AgentClient::Gemini, &InstallState::default()).unwrap();
		let path = Path::new(&installed.component.path);
		let mut config = read_json_object(&context.root, path).unwrap();
		config["mcpServers"]["code-moniker"]["timeout"] = json!(1);
		physical_config::write(
			physical_config::snapshot(&context.root, path).unwrap(),
			&serde_json::to_vec_pretty(&Value::Object(config)).unwrap(),
		)
		.unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"mcp",
				&installed.component,
				AgentClient::Gemini,
			),
			"stale"
		);
	}

	#[test]
	fn matching_pre_existing_mcp_entry_remains_external() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let first = install_mcp(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(first.component.owned);

		let adopted = install_mcp(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(!adopted.component.owned);
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"mcp",
				&adopted.component,
				AgentClient::Codex,
			),
			"external"
		);
	}

	#[cfg(unix)]
	#[test]
	fn matching_pre_existing_skill_symlink_is_rejected_unchanged() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let source = dir.path().join("source");
		for (relative, contents) in SKILL_FILES {
			write_test_file(&source.join(relative), contents.as_bytes());
		}
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		fs::create_dir_all(skill.parent().unwrap()).unwrap();
		symlink(&source, &skill).unwrap();

		let error = match install_skill(&context, AgentClient::Codex, &InstallState::default()) {
			Ok(_) => panic!("linked skill was accepted"),
			Err(error) => error.to_string(),
		};
		assert!(error.contains("refusing linked skill"));
		assert!(
			fs::symlink_metadata(&skill)
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert!(source.join("SKILL.md").exists());
	}

	#[cfg(unix)]
	#[test]
	fn linked_skill_parent_is_rejected_without_writing_its_target() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let linked_target = dir.path().join("linked-skills");
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let skills = user_skill_path(&context.home, AgentClient::Codex)
			.parent()
			.unwrap()
			.to_path_buf();
		fs::create_dir_all(skills.parent().unwrap()).unwrap();
		fs::create_dir_all(&linked_target).unwrap();
		fs::write(linked_target.join("sentinel"), "unchanged").unwrap();
		symlink(&linked_target, &skills).unwrap();

		let error = install_skill(&context, AgentClient::Codex, &InstallState::default())
			.err()
			.unwrap()
			.to_string();

		assert!(error.contains("refusing non-physical directory component"));
		assert!(
			fs::symlink_metadata(&skills)
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert_eq!(
			fs::read_to_string(linked_target.join("sentinel")).unwrap(),
			"unchanged"
		);
		assert!(!linked_target.join(SKILL_NAME).exists());
	}

	#[cfg(unix)]
	#[test]
	fn linked_skill_parent_drift_is_stale_before_uninstall() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = Path::new(&installed.component.path);
		let skills = skill.parent().unwrap();
		let moved_skills = dir.path().join("moved-skills");
		fs::rename(skills, &moved_skills).unwrap();
		symlink(&moved_skills, skills).unwrap();

		assert!(moved_skills.join(SKILL_NAME).join("SKILL.md").exists());
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&installed.component,
				AgentClient::Codex,
			),
			"stale"
		);
		assert!(moved_skills.join(SKILL_NAME).join("SKILL.md").exists());
	}

	#[cfg(unix)]
	#[test]
	fn linked_skill_asset_is_rejected_without_writing_its_target() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = Path::new(&installed.component.path);
		let external = dir.path().join("external-skill.md");
		fs::write(&external, "do not overwrite").unwrap();
		fs::remove_file(skill.join("SKILL.md")).unwrap();
		symlink(&external, skill.join("SKILL.md")).unwrap();
		let mut state = InstallState::default();
		state
			.components
			.insert("skill".to_string(), installed.component.clone());

		let error = install_skill(&context, AgentClient::Codex, &state)
			.err()
			.unwrap()
			.to_string();

		assert!(error.contains("refusing linked skill asset"));
		assert_eq!(fs::read_to_string(&external).unwrap(), "do not overwrite");
		assert!(
			fs::symlink_metadata(skill.join("SKILL.md"))
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&installed.component,
				AgentClient::Codex,
			),
			"stale"
		);
	}

	#[cfg(unix)]
	#[test]
	fn matching_skill_symlink_is_reported_stale() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let source = dir.path().join("source");
		for (relative, contents) in SKILL_FILES {
			write_test_file(&source.join(relative), contents.as_bytes());
		}
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = Path::new(&installed.component.path);
		fs::remove_dir_all(skill).unwrap();
		symlink(&source, skill).unwrap();

		assert_eq!(
			fs::read_to_string(skill.join("SKILL.md")).unwrap(),
			SKILL_FILES[0].1
		);
		assert!(!physical_skill::matches(&context.home, skill));
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&installed.component,
				AgentClient::Codex,
			),
			"stale"
		);
	}

	#[cfg(unix)]
	#[test]
	fn dangling_managed_skill_symlink_is_stale_not_missing() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill = Path::new(&installed.component.path);
		fs::remove_dir_all(skill).unwrap();
		symlink(dir.path().join("missing-skill"), skill).unwrap();

		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"skill",
				&installed.component,
				AgentClient::Codex,
			),
			"stale"
		);
	}

	#[cfg(unix)]
	#[test]
	fn non_matching_skill_symlink_is_rejected_unchanged() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let source = dir.path().join("source");
		fs::create_dir_all(&source).unwrap();
		fs::write(source.join("SKILL.md"), "external").unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		fs::create_dir_all(skill.parent().unwrap()).unwrap();
		symlink(&source, &skill).unwrap();

		let error = install_skill(&context, AgentClient::Codex, &InstallState::default())
			.err()
			.unwrap()
			.to_string();

		assert!(error.contains("refusing linked skill"));
		assert!(
			fs::symlink_metadata(&skill)
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert_eq!(
			fs::read_to_string(source.join("SKILL.md")).unwrap(),
			"external"
		);
	}

	#[cfg(unix)]
	#[test]
	fn dangling_skill_symlink_is_rejected_unchanged() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let missing = dir.path().join("missing");
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		fs::create_dir_all(skill.parent().unwrap()).unwrap();
		symlink(&missing, &skill).unwrap();

		let error = install_skill(&context, AgentClient::Codex, &InstallState::default())
			.err()
			.unwrap()
			.to_string();

		assert!(error.contains("refusing linked skill"));
		assert!(
			fs::symlink_metadata(&skill)
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert!(!missing.exists());
	}

	#[test]
	fn uninstall_removes_only_managed_skill_files() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Claude, &InstallState::default()).unwrap();
		let path = Path::new(&installed.component.path);
		fs::write(path.join("user-note.md"), "keep").unwrap();
		uninstall_skill(&context.home, path).unwrap();
		assert_eq!(
			fs::read_to_string(path.join("user-note.md")).unwrap(),
			"keep"
		);
		assert!(!path.join("SKILL.md").exists());
	}

	#[test]
	fn uninstall_mcp_preserves_unrelated_json_configuration() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let path = context.root.join(".mcp.json");
		fs::write(
			&path,
			r#"{"permissions":{"allow":["Read"]},"mcpServers":{"other":{"command":"other"}}}"#,
		)
		.unwrap();
		let installed =
			install_mcp(&context, AgentClient::Claude, &InstallState::default()).unwrap();
		uninstall_mcp(
			&context.root,
			&path,
			AgentClient::Claude,
			&installed.component,
		)
		.unwrap();
		let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		assert_eq!(config["permissions"]["allow"][0], "Read");
		assert_eq!(config["mcpServers"]["other"]["command"], "other");
		assert!(config["mcpServers"].get("code-moniker").is_none());
	}

	#[test]
	fn uninstall_mcp_removes_a_fresh_configuration_and_parent() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_mcp(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let config_path = PathBuf::from(&installed.component.path);
		assert!(installed.component.config_created);
		assert!(installed.component.config_parent_created);

		uninstall_mcp(
			&context.root,
			&config_path,
			AgentClient::Codex,
			&installed.component,
		)
		.unwrap();

		assert!(!config_path.exists());
		assert!(!context.root.join(".codex").exists());
	}

	#[test]
	fn mcp_reinstall_invalidates_full_file_ownership_after_a_foreign_addition() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().to_path_buf(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let first = install_mcp(&context, AgentClient::Gemini, &InstallState::default()).unwrap();
		assert!(first.component.config_created);
		let config_path = PathBuf::from(&first.component.path);
		let mut config = read_json_object(&context.root, &config_path).unwrap();
		config.insert("foreign".to_string(), Value::Bool(true));
		fs::write(
			&config_path,
			serde_json::to_vec_pretty(&Value::Object(config)).unwrap(),
		)
		.unwrap();
		let state = InstallState {
			components: BTreeMap::from([("mcp".to_string(), first.component)]),
			..InstallState::default()
		};

		let second = install_mcp(&context, AgentClient::Gemini, &state).unwrap();
		assert!(!second.component.config_created);
		uninstall_mcp(
			&context.root,
			&config_path,
			AgentClient::Gemini,
			&second.component,
		)
		.unwrap();

		let remaining = read_json_object(&context.root, &config_path).unwrap();
		assert_eq!(remaining.get("foreign"), Some(&Value::Bool(true)));
		assert!(remaining.get("mcpServers").is_none_or(Value::is_object));
	}

	#[test]
	fn hook_install_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		fs::write(
			dir.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			hooks::install_for_agent(&args, AgentClient::Codex, None, &mut Vec::new()).unwrap();
		let hook_path = installed.path.clone();
		let component = ComponentState {
			scope: "project".to_string(),
			path: hook_path.display().to_string(),
			checksum: checksum_bytes(&installed.fingerprint),
			owned: installed.owned,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: Some(".code-moniker.toml".to_string()),
			check_scope: Some(".".to_string()),
			max_violations: Some(10),
			config_created: false,
			config_parent_created: false,
			hook_directory_created: false,
			config_checksum: None,
		};
		let mut state = InstallState::default();
		let state_parent = dir.path().join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_installed_hook(
			dir.path(),
			&state_parent.join("state.json"),
			&mut state,
			component,
			installed.rollback,
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert!(!hook_path.exists());
		assert!(!dir.path().join(".codex/hooks.json").exists());
		assert!(!dir.path().join(".codex").exists());
		assert!(!state.components.contains_key("hooks"));
	}

	#[test]
	fn fresh_hook_uninstall_removes_owned_files_and_directories() {
		let dir = tempdir().unwrap();
		fs::write(
			dir.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			hooks::install_for_agent(&args, AgentClient::Codex, None, &mut Vec::new()).unwrap();

		hooks::uninstall_for_agent_with_policy(
			dir.path(),
			AgentClient::Codex,
			&installed.path,
			&installed.fingerprint,
			hooks::AgentHookRemovalPolicy {
				config_created: installed.config_created,
				config_parent_created: installed.config_parent_created,
				hook_directory_created: installed.hook_directory_created,
				config_checksum: installed.config_checksum.as_deref(),
			},
		)
		.unwrap();

		assert!(!installed.path.exists());
		assert!(!dir.path().join(".codex/hooks.json").exists());
		assert!(!dir.path().join(".codex").exists());
	}

	#[test]
	fn hook_reinstall_invalidates_full_file_ownership_after_a_foreign_addition() {
		let dir = tempdir().unwrap();
		fs::write(
			dir.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let first =
			hooks::install_for_agent(&args, AgentClient::Gemini, None, &mut Vec::new()).unwrap();
		let first_component = hook_component(
			&first,
			HookPolicy::from_args(&AgentInstallArgs {
				client: AgentClient::Gemini,
				components: vec![AgentComponent::Hooks],
				root: dir.path().to_path_buf(),
				rules: ".code-moniker.toml".into(),
				profile: None,
				check_scope: ".".into(),
				max_violations: 10,
			}),
			None,
		);
		let config_path = dir.path().join(".gemini/settings.json");
		let mut config = read_json_object(dir.path(), &config_path).unwrap();
		config.insert("foreign".to_string(), Value::Bool(true));
		fs::write(
			&config_path,
			serde_json::to_vec_pretty(&Value::Object(config)).unwrap(),
		)
		.unwrap();
		let second = hooks::install_for_agent(
			&args,
			AgentClient::Gemini,
			Some(Path::new(&first_component.path)),
			&mut Vec::new(),
		)
		.unwrap();
		let second_component = hook_component(
			&second,
			HookPolicy::from_args(&AgentInstallArgs {
				client: AgentClient::Gemini,
				components: vec![AgentComponent::Hooks],
				root: dir.path().to_path_buf(),
				rules: ".code-moniker.toml".into(),
				profile: None,
				check_scope: ".".into(),
				max_violations: 10,
			}),
			Some(&first_component),
		);
		assert!(!second_component.config_created);
		let fingerprint =
			hooks::agent_hook_fingerprint(AgentClient::Gemini, Path::new(&second_component.path))
				.unwrap();
		hooks::uninstall_for_agent_with_policy(
			dir.path(),
			AgentClient::Gemini,
			Path::new(&second_component.path),
			&fingerprint,
			hooks::AgentHookRemovalPolicy {
				config_created: second_component.config_created,
				config_parent_created: second_component.config_parent_created,
				hook_directory_created: second_component.hook_directory_created,
				config_checksum: second_component.config_checksum.as_deref(),
			},
		)
		.unwrap();

		let remaining = read_json_object(dir.path(), &config_path).unwrap();
		assert_eq!(remaining.get("foreign"), Some(&Value::Bool(true)));
	}

	#[test]
	fn skill_install_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let context = InstallContext {
			home: home.clone(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let skill_path = PathBuf::from(&installed.component.path);
		let mut state = InstallState::default();
		let state_parent = home.join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_installed_skill(
			&home,
			&state_parent.join("state.json"),
			&mut state,
			installed.component,
			installed.skill_rollback,
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert!(!skill_path.exists());
		assert!(!state.components.contains_key("skill"));
	}

	#[test]
	fn mcp_install_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let context = InstallContext {
			home: home.clone(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_mcp(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let config_path = PathBuf::from(&installed.component.path);
		let mut state = InstallState::default();
		let state_parent = home.join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_installed_mcp(
			&home,
			&state_parent.join("state.json"),
			&mut state,
			installed.component,
			installed.config_rollback,
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert!(!config_path.exists());
		assert!(!state.components.contains_key("mcp"));
	}

	#[test]
	fn skill_write_rolls_back_assets_changed_before_a_later_failure() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let skill = home.join(".codex/skills/code-moniker");
		for (relative, _) in SKILL_FILES {
			write_test_file(&skill.join(relative), b"previous");
		}
		let failing_asset = skill.join("postures/develop.md");
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| {
			let failing_asset = failing_asset.clone();
			*hook.borrow_mut() = Some(Box::new(move |path| {
				if path == failing_asset {
					fs::remove_file(path).unwrap();
					fs::create_dir(path).unwrap();
				}
			}));
		});

		let error = physical_skill::write_assets(&home, &skill)
			.err()
			.unwrap()
			.to_string();
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| *hook.borrow_mut() = None);

		assert!(!error.is_empty());
		assert_eq!(fs::read(skill.join("SKILL.md")).unwrap(), b"previous");
		assert!(failing_asset.is_dir());
		assert_eq!(
			fs::read(skill.join("postures/onboard.md")).unwrap(),
			b"previous"
		);
	}

	#[test]
	fn skill_rollback_refuses_an_asset_changed_after_its_cas() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let skill = home.join(".codex/skills/code-moniker");
		let first_asset = skill.join(SKILL_FILES[0].0);
		let second_asset = skill.join(SKILL_FILES[1].0);
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| {
			let first_asset = first_asset.clone();
			let second_asset = second_asset.clone();
			*hook.borrow_mut() = Some(Box::new(move |path| {
				if path == second_asset {
					fs::write(&first_asset, "concurrent").unwrap();
				}
			}));
		});

		let mutation = physical_skill::write_assets(&home, &skill).unwrap();
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| *hook.borrow_mut() = None);
		let error = physical_skill::rollback(&home, &mutation)
			.unwrap_err()
			.to_string();

		assert!(!error.is_empty());
		assert_eq!(fs::read_to_string(first_asset).unwrap(), "concurrent");
	}

	#[test]
	fn skill_remove_restores_assets_removed_before_a_later_conflict() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let skill = home.join(".codex/skills/code-moniker");
		physical_skill::write_assets(&home, &skill).unwrap();
		let conflicting_asset = skill.join("postures/develop.md");
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| {
			let conflicting_asset = conflicting_asset.clone();
			*hook.borrow_mut() = Some(Box::new(move |path| {
				if path == conflicting_asset {
					fs::write(path, "concurrent").unwrap();
				}
			}));
		});

		let error = physical_skill::remove(&home, &skill)
			.err()
			.unwrap()
			.to_string();
		physical_skill::BEFORE_ASSET_MUTATION.with(|hook| *hook.borrow_mut() = None);

		assert!(!error.is_empty());
		assert_eq!(
			fs::read_to_string(&conflicting_asset).unwrap(),
			"concurrent"
		);
		for (relative, contents) in SKILL_FILES {
			if *relative != "postures/develop.md" {
				assert_eq!(fs::read_to_string(skill.join(relative)).unwrap(), *contents);
			}
		}
	}

	#[test]
	fn hook_uninstall_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		fs::write(
			dir.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			hooks::install_for_agent(&args, AgentClient::Claude, None, &mut Vec::new()).unwrap();
		let hook_before = fs::read(&installed.path).unwrap();
		let config_path = dir.path().join(".claude/settings.json");
		let config_before = fs::read(&config_path).unwrap();
		let component = ComponentState {
			scope: "project".to_string(),
			path: installed.path.display().to_string(),
			checksum: checksum_bytes(&installed.fingerprint),
			owned: installed.owned,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: Some(".code-moniker.toml".to_string()),
			check_scope: Some(".".to_string()),
			max_violations: Some(10),
			config_created: installed.config_created,
			config_parent_created: installed.config_parent_created,
			hook_directory_created: installed.hook_directory_created,
			config_checksum: installed.config_checksum.clone(),
		};
		let rollback = hooks::uninstall_for_agent_with_policy(
			dir.path(),
			AgentClient::Claude,
			&installed.path,
			&installed.fingerprint,
			hooks::AgentHookRemovalPolicy {
				config_created: component.config_created,
				config_parent_created: component.config_parent_created,
				hook_directory_created: component.hook_directory_created,
				config_checksum: component.config_checksum.as_deref(),
			},
		)
		.unwrap();
		let mut state = InstallState {
			components: BTreeMap::from([
				("hooks".to_string(), component.clone()),
				(
					"dummy".to_string(),
					ComponentState {
						scope: "project".to_string(),
						path: "dummy".to_string(),
						checksum: String::new(),
						owned: false,
						version: String::new(),
						profile: None,
						rules: None,
						check_scope: None,
						max_violations: None,
						config_created: false,
						config_parent_created: false,
						hook_directory_created: false,
						config_checksum: None,
					},
				),
			]),
			..InstallState::default()
		};
		let state_parent = dir.path().join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_removed_component(
			&state_parent.join("state.json"),
			dir.path(),
			&mut state,
			"hooks",
			component,
			Some(ComponentRollback::Hook(rollback)),
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert_eq!(fs::read(&installed.path).unwrap(), hook_before);
		assert_eq!(fs::read(&config_path).unwrap(), config_before);
		assert!(state.components.contains_key("hooks"));
	}

	#[test]
	fn skill_uninstall_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let context = InstallContext {
			home: home.clone(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		let component = installed.component;
		let skill_path = PathBuf::from(&component.path);
		let rollback = uninstall_skill(&home, &skill_path).unwrap();
		assert!(!skill_path.exists());
		let mut state = InstallState {
			components: BTreeMap::from([
				("skill".to_string(), component.clone()),
				(
					"dummy".to_string(),
					ComponentState {
						scope: "project".to_string(),
						path: "dummy".to_string(),
						checksum: String::new(),
						owned: false,
						version: String::new(),
						profile: None,
						rules: None,
						check_scope: None,
						max_violations: None,
						config_created: false,
						config_parent_created: false,
						hook_directory_created: false,
						config_checksum: None,
					},
				),
			]),
			..InstallState::default()
		};
		let state_parent = home.join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_removed_component(
			&state_parent.join("state.json"),
			&home,
			&mut state,
			"skill",
			component,
			Some(ComponentRollback::Skill(rollback)),
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert!(physical_skill::matches(&home, &skill_path));
		assert!(state.components.contains_key("skill"));
	}

	#[test]
	fn mcp_uninstall_is_rolled_back_when_state_persistence_fails() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let context = InstallContext {
			home: home.clone(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let installed =
			install_mcp(&context, AgentClient::Claude, &InstallState::default()).unwrap();
		let component = installed.component;
		let config_path = PathBuf::from(&component.path);
		let rollback =
			uninstall_mcp(&context.root, &config_path, AgentClient::Claude, &component).unwrap();
		let mut state = InstallState::default();
		state
			.components
			.insert("mcp".to_string(), component.clone());
		let state_parent = home.join("state-parent");
		fs::write(&state_parent, "not a directory").unwrap();

		let error = persist_removed_component(
			&state_parent.join("state.json"),
			&home,
			&mut state,
			"mcp",
			component.clone(),
			Some(ComponentRollback::Mcp(rollback)),
		)
		.unwrap_err()
		.to_string();

		assert!(!error.is_empty());
		assert_eq!(
			component_status(&home, &context.root, "mcp", &component, AgentClient::Claude,),
			"installed"
		);
		assert!(state.components.contains_key("mcp"));
	}

	#[test]
	fn shared_user_skill_uses_independent_state_references() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		let root_a = dir.path().join("project-a");
		let root_b = dir.path().join("project-b");
		fs::create_dir_all(&home).unwrap();
		fs::create_dir_all(&root_a).unwrap();
		fs::create_dir_all(&root_b).unwrap();
		let context_a = InstallContext {
			home: home.clone(),
			binary: PathBuf::from("/bin/code-moniker"),
			root: root_a.canonicalize().unwrap(),
		};
		let context_b = InstallContext {
			home,
			binary: PathBuf::from("/bin/code-moniker"),
			root: root_b.canonicalize().unwrap(),
		};

		let first =
			install_skill(&context_a, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(first.component.owned);
		let first_component = first.component;
		let mut state_a = InstallState {
			schema: STATE_SCHEMA,
			version: String::new(),
			client: "codex".to_string(),
			root: context_a.root.display().to_string(),
			components: BTreeMap::from([("skill".to_string(), first_component)]),
			persisted_contents: None,
			persisted_mode: None,
		};
		let state_a_path = state_path(&context_a, AgentClient::Codex);
		persist_state(&context_a.home, &state_a_path, &mut state_a).unwrap();

		let second =
			install_skill(&context_b, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(second.component.owned);
		let mut state_b = InstallState {
			schema: STATE_SCHEMA,
			version: String::new(),
			client: "codex".to_string(),
			root: context_b.root.display().to_string(),
			components: BTreeMap::from([("skill".to_string(), second.component)]),
			persisted_contents: None,
			persisted_mode: None,
		};
		let state_b_path = state_path(&context_b, AgentClient::Codex);
		persist_state(&context_b.home, &state_b_path, &mut state_b).unwrap();

		let component_b = state_b.components.get("skill").unwrap();
		assert!(
			shared_skill_is_referenced(
				&context_b.home,
				AgentClient::Codex,
				&state_b_path,
				component_b,
			)
			.unwrap()
		);
		assert!(Path::new(&component_b.path).exists());
		assert!(
			read_state(&context_a.home, &state_a_path)
				.unwrap()
				.unwrap()
				.components["skill"]
				.owned
		);

		let state_b_contents = crate::fs_nofollow::read(&context_b.home, &state_b_path)
			.unwrap()
			.unwrap();
		let state_b_mode = crate::fs_nofollow::mode(&context_b.home, &state_b_path).unwrap();
		crate::fs_nofollow::remove_if_unchanged(
			&context_b.home,
			&state_b_path,
			&state_b_contents,
			state_b_mode,
		)
		.unwrap();
		let component_a = &state_a.components["skill"];
		assert!(
			!shared_skill_is_referenced(
				&context_a.home,
				AgentClient::Codex,
				&state_a_path,
				component_a,
			)
			.unwrap()
		);
		uninstall_skill(&context_a.home, Path::new(&component_a.path)).unwrap();
		assert!(!Path::new(&component_a.path).exists());
	}

	#[test]
	fn hook_repair_command_targets_hooks_and_preserves_the_profile() {
		let component = ComponentState {
			scope: "project".to_string(),
			path: "/project/.codex/hooks/code-moniker-agent.sh".to_string(),
			checksum: "checksum".to_string(),
			owned: true,
			version: "0.4.0".to_string(),
			profile: Some("fast profile".to_string()),
			rules: Some("config/agent.toml".to_string()),
			check_scope: Some("src".to_string()),
			max_violations: Some(7),
			config_created: false,
			config_parent_created: false,
			hook_directory_created: false,
			config_checksum: None,
		};
		let mut output = Vec::new();
		write_component_repair(
			&mut output,
			AgentClient::Codex,
			Path::new("/project with space"),
			"hooks",
			&component,
		)
		.unwrap();
		assert_eq!(
			String::from_utf8(output).unwrap(),
			"fix: code-moniker agent install --client codex --components hooks --profile 'fast profile' --rules config/agent.toml --check-scope src --max-violations 7 '/project with space'\n"
		);
	}

	#[test]
	fn codex_hook_output_requires_explicit_app_approval() {
		let mut output = Vec::new();
		write_hook_activation_note(&mut output, AgentClient::Codex, true).unwrap();
		assert_eq!(
			String::from_utf8(output).unwrap(),
			"action: approve this project hook in Codex app Settings; CLI status and doctor cannot observe app approval\n"
		);

		let mut other_output = Vec::new();
		write_hook_activation_note(&mut other_output, AgentClient::Claude, true).unwrap();
		write_hook_activation_note(&mut other_output, AgentClient::Codex, false).unwrap();
		assert!(other_output.is_empty());
	}

	#[cfg(unix)]
	#[test]
	fn dangling_hook_layout_is_stale_not_missing() {
		use std::os::unix::fs::PermissionsExt;
		use std::os::unix::fs::symlink;
		let dir = tempdir().unwrap();
		fs::write(
			dir.path().join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			hooks::install_for_agent(&args, AgentClient::Codex, None, &mut Vec::new()).unwrap();
		let component = ComponentState {
			scope: "project".to_string(),
			path: installed.path.display().to_string(),
			checksum: checksum_bytes(&installed.fingerprint),
			owned: true,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: Some(".code-moniker.toml".to_string()),
			check_scope: Some(".".to_string()),
			max_violations: Some(10),
			config_created: false,
			config_parent_created: false,
			hook_directory_created: false,
			config_checksum: None,
		};
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let hook_path = PathBuf::from(&component.path);
		let config_path = context.root.join(".codex/hooks.json");
		let script = fs::read(&hook_path).unwrap();
		let config = fs::read(&config_path).unwrap();
		let hook_permissions = fs::metadata(&hook_path).unwrap().permissions();

		fs::set_permissions(&hook_path, fs::Permissions::from_mode(0o644)).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"hooks",
				&component,
				AgentClient::Codex,
			),
			"stale"
		);
		fs::set_permissions(&hook_path, hook_permissions).unwrap();

		fs::remove_file(&hook_path).unwrap();
		symlink(context.root.join("missing-hook"), &hook_path).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"hooks",
				&component,
				AgentClient::Codex,
			),
			"stale"
		);
		fs::remove_file(&hook_path).unwrap();
		fs::write(&hook_path, &script).unwrap();

		fs::remove_file(&config_path).unwrap();
		symlink(context.root.join("missing-config"), &config_path).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"hooks",
				&component,
				AgentClient::Codex,
			),
			"stale"
		);
		fs::remove_file(&config_path).unwrap();
		fs::write(&config_path, &config).unwrap();

		fs::remove_file(&hook_path).unwrap();
		fs::remove_file(&config_path).unwrap();
		symlink(context.root.join("missing-config"), &config_path).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"hooks",
				&component,
				AgentClient::Codex,
			),
			"stale"
		);
		fs::remove_file(&config_path).unwrap();
		fs::write(&config_path, &config).unwrap();
		fs::write(&hook_path, &script).unwrap();

		let hooks_dir = hook_path.parent().unwrap();
		let moved_hooks = context.root.join("moved-hooks");
		fs::rename(hooks_dir, &moved_hooks).unwrap();
		symlink(context.root.join("missing-hooks"), hooks_dir).unwrap();
		assert_eq!(
			component_status(
				&context.home,
				&context.root,
				"hooks",
				&component,
				AgentClient::Codex,
			),
			"stale"
		);
		assert!(moved_hooks.join(hook_path.file_name().unwrap()).exists());
	}

	#[test]
	fn status_without_state_is_read_only() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: dir.path().join("code-moniker"),
			root: dir.path().to_path_buf(),
		};
		assert!(!state_path(&context, AgentClient::Codex).exists());
	}

	#[cfg(unix)]
	#[test]
	fn state_io_rejects_symlinks_without_touching_the_target() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		let state_dir = home.join(".code-moniker/agent/codex");
		fs::create_dir_all(&state_dir).unwrap();
		let state_path = state_dir.join("state.json");
		let external = dir.path().join("external.json");
		fs::write(&external, br#"{"external":true}"#).unwrap();
		symlink(&external, &state_path).unwrap();

		assert!(read_state(&home, &state_path).is_err());
		let mut state = InstallState::default();
		assert!(persist_state(&home, &state_path, &mut state).is_err());
		assert_eq!(fs::read(&external).unwrap(), br#"{"external":true}"#);
	}

	#[test]
	fn state_persistence_refuses_a_change_after_the_initial_read() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		fs::create_dir(&home).unwrap();
		let state_path = home.join("state.json");
		let mut initial = InstallState {
			schema: STATE_SCHEMA,
			version: String::new(),
			client: "codex".to_string(),
			root: "/project".to_string(),
			..InstallState::default()
		};
		persist_state(&home, &state_path, &mut initial).unwrap();
		let mut loaded = read_state(&home, &state_path).unwrap().unwrap();
		fs::write(&state_path, br#"{"concurrent":true}"#).unwrap();
		loaded.version = "changed".to_string();

		let error = persist_state(&home, &state_path, &mut loaded)
			.unwrap_err()
			.to_string();

		assert!(!error.is_empty());
		assert_eq!(
			fs::read_to_string(state_path).unwrap(),
			r#"{"concurrent":true}"#
		);
	}

	#[cfg(unix)]
	#[test]
	fn state_read_rejects_fifo_without_blocking() {
		use std::ffi::CString;
		use std::os::unix::ffi::OsStrExt;

		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		let state_dir = home.join(".code-moniker/agent/codex");
		fs::create_dir_all(&state_dir).unwrap();
		let state_path = state_dir.join("state.json");
		let raw = CString::new(state_path.as_os_str().as_bytes()).unwrap();
		assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);

		assert!(read_state(&home, &state_path).is_err());
	}
}
