use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::args::{
	AgentClient, AgentCommand, AgentComponent, AgentInspectArgs, AgentInstallArgs,
	AgentUninstallArgs, CodexHarnessArgs,
};
use crate::fs_write::write_atomic;
use crate::{Exit, harness};

const STATE_SCHEMA: u32 = 1;
const SKILL_NAME: &str = "code-moniker";

const SKILL_FILES: &[(&str, &str)] = &[
	(
		"SKILL.md",
		include_str!("../../assets/agent/code-moniker/SKILL.md"),
	),
	(
		"references/diagnose.md",
		include_str!("../../assets/agent/code-moniker/references/diagnose.md"),
	),
	(
		"references/explore.md",
		include_str!("../../assets/agent/code-moniker/references/explore.md"),
	),
	(
		"references/mcp.md",
		include_str!("../../assets/agent/code-moniker/references/mcp.md"),
	),
	(
		"references/query-dsl.md",
		include_str!("../../assets/agent/code-moniker/references/query-dsl.md"),
	),
];

#[derive(Debug, Default, Deserialize, Serialize)]
struct InstallState {
	schema: u32,
	version: String,
	client: String,
	root: String,
	components: BTreeMap<String, ComponentState>,
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
			"this code-moniker binary has no MCP support; reinstall it with `cargo install code-moniker --features tui,mcp`"
		);
	}

	let state_path = state_path(&context, args.client);
	let mut state = read_state(&state_path)?.unwrap_or_else(|| InstallState {
		schema: STATE_SCHEMA,
		version: env!("CARGO_PKG_VERSION").to_string(),
		client: client_name(args.client).to_string(),
		root: context.root.display().to_string(),
		components: BTreeMap::new(),
	});
	validate_state_identity(&state, &context, args.client)?;

	if components.contains(&AgentComponent::Skill) {
		let installed = install_skill(&context, args.client, &state)?;
		let previous_owner = installed.previous_owner.clone();
		state
			.components
			.insert("skill".to_string(), installed.component);
		persist_state(&state_path, &mut state)?;
		if let Some(previous_owner) = previous_owner {
			release_previous_skill_owner(
				&previous_owner,
				state
					.components
					.get("skill")
					.context("installed skill disappeared from integration state")?,
			)?;
		}
		writeln!(stdout, "skill: {}", installed.message)?;
	}
	if components.contains(&AgentComponent::Mcp) {
		let installed = install_mcp(&context, args.client, &state)?;
		state
			.components
			.insert("mcp".to_string(), installed.component);
		persist_state(&state_path, &mut state)?;
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
		let harness_args = CodexHarnessArgs {
			root: context.root.to_path_buf(),
			rules: hook_policy.rules.to_path_buf(),
			profile: hook_policy.profile.clone(),
			scope: hook_policy.check_scope.to_path_buf(),
			max_violations: hook_policy.max_violations,
		};
		let installed =
			harness::install_for_agent(&harness_args, args.client, managed_path, stdout)?;
		state.components.insert(
			"hooks".to_string(),
			ComponentState {
				scope: "project".to_string(),
				path: installed.path.display().to_string(),
				checksum: checksum_bytes(&installed.fingerprint),
				owned: installed.owned,
				version: env!("CARGO_PKG_VERSION").to_string(),
				profile: hook_policy.profile,
				rules: Some(hook_policy.rules.display().to_string()),
				check_scope: Some(hook_policy.check_scope.display().to_string()),
				max_violations: Some(hook_policy.max_violations),
			},
		);
		persist_state(&state_path, &mut state)?;
	}

	writeln!(
		stdout,
		"integration: {} ({})",
		state_path.display(),
		env!("CARGO_PKG_VERSION")
	)?;
	Ok(true)
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

fn status<W: Write>(args: &AgentInspectArgs, stdout: &mut W) -> anyhow::Result<()> {
	let context = install_context(&args.root)?;
	let path = state_path(&context, args.client);
	let Some(state) = read_state(&path)? else {
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
		let current = component_status(name, component, args.client);
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
	let Some(state) = read_state(&path)? else {
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
	for (name, component) in &state.components {
		let status = component_status(name, component, args.client);
		let status_problem = status != "installed" && status != "linked" && status != "external";
		let version_problem = component.version != env!("CARGO_PKG_VERSION");
		if status_problem {
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

fn uninstall<W: Write>(args: &AgentUninstallArgs, stdout: &mut W) -> anyhow::Result<bool> {
	let context = install_context(&args.root)?;
	let path = state_path(&context, args.client);
	let Some(mut state) = read_state(&path)? else {
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
		let status = component_status(name, component, args.client);
		if status != "installed" && status != "linked" && status != "external" {
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
		if component.owned {
			match name.as_str() {
				"skill" => {
					if transfer_shared_skill_ownership(args.client, &path, &component)? {
						writeln!(stdout, "skill: retained shared component")?;
					} else {
						uninstall_skill(Path::new(&component.path))?;
						writeln!(stdout, "{name}: removed managed component")?;
					}
				}
				"mcp" => {
					uninstall_mcp(Path::new(&component.path), args.client)?;
					writeln!(stdout, "{name}: removed managed component")?;
				}
				"hooks" => {
					harness::uninstall_for_agent(
						&context.root,
						args.client,
						Path::new(&component.path),
					)?;
					writeln!(stdout, "{name}: removed managed component")?;
				}
				_ => bail!("unknown managed component `{name}`"),
			}
		} else {
			writeln!(stdout, "{name}: retained pre-existing external component")?;
		}
		state.components.remove(name);
		if state.components.is_empty() {
			fs::remove_file(&path)
				.with_context(|| format!("cannot remove `{}`", path.display()))?;
			remove_empty_parents(path.parent(), context.home.join(".code-moniker"));
		} else {
			persist_state(&path, &mut state)?;
		}
	}
	Ok(true)
}

struct InstalledComponent {
	component: ComponentState,
	message: String,
	previous_owner: Option<PathBuf>,
}

fn install_skill(
	context: &InstallContext,
	client: AgentClient,
	state: &InstallState,
) -> anyhow::Result<InstalledComponent> {
	let path = user_skill_path(&context.home, client);
	let existing = fs::symlink_metadata(&path).is_ok();
	let managed_here = state
		.components
		.get("skill")
		.is_some_and(|component| component.owned);
	let current_state_path = state_path(context, client);
	let shared_owner = if managed_here {
		None
	} else {
		find_owned_skill_state(&current_state_path, client, &path)?
	};
	let managed = managed_here || shared_owner.is_some();
	let matched_before = skill_matches(&path);
	let claim_shared_ownership = shared_owner.is_some() && !matched_before;
	if path.exists() && !managed && !matched_before {
		bail!(
			"refusing to replace unmanaged skill `{}`; move it aside or remove it explicitly",
			path.display()
		);
	}
	if fs::symlink_metadata(&path)
		.map(|metadata| metadata.file_type().is_symlink())
		.unwrap_or(false)
	{
		if !skill_matches(&path) {
			bail!(
				"linked skill `{}` does not match the embedded {} assets",
				path.display(),
				env!("CARGO_PKG_VERSION")
			);
		}
		return Ok(InstalledComponent {
			component: ComponentState {
				scope: "user".to_string(),
				path: path.display().to_string(),
				checksum: skill_checksum(),
				owned: false,
				version: env!("CARGO_PKG_VERSION").to_string(),
				profile: None,
				rules: None,
				check_scope: None,
				max_violations: None,
			},
			message: format!("linked development skill retained at {}", path.display()),
			previous_owner: None,
		});
	}

	for (relative, contents) in SKILL_FILES {
		write_atomic(&path.join(relative), contents.as_bytes())?;
	}
	Ok(InstalledComponent {
		component: ComponentState {
			scope: "user".to_string(),
			path: path.display().to_string(),
			checksum: skill_checksum(),
			owned: managed_here || !existing || claim_shared_ownership,
			version: env!("CARGO_PKG_VERSION").to_string(),
			profile: None,
			rules: None,
			check_scope: None,
			max_violations: None,
		},
		message: format!(
			"installed {} embedded files at {}",
			SKILL_FILES.len(),
			path.display()
		),
		previous_owner: if claim_shared_ownership {
			shared_owner
		} else {
			None
		},
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
	let owned = match client {
		AgentClient::Codex => install_codex_mcp(&config_path, &command, &args, managed)?,
		AgentClient::Claude | AgentClient::Gemini => {
			install_json_mcp(&config_path, &command, &args, managed, client)?
		}
	};
	let checksum = mcp_entry_checksum(&config_path, client)?;
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
		},
		message: format!(
			"registered project-owned stdio server in {}",
			config_path.display()
		),
		previous_owner: None,
	})
}

fn uninstall_skill(path: &Path) -> anyhow::Result<()> {
	if fs::symlink_metadata(path)
		.map(|metadata| metadata.file_type().is_symlink())
		.unwrap_or(false)
	{
		fs::remove_file(path)
			.with_context(|| format!("cannot remove linked skill `{}`", path.display()))?;
		return Ok(());
	}
	for (relative, _) in SKILL_FILES.iter().rev() {
		let file = path.join(relative);
		if file.exists() {
			fs::remove_file(&file)
				.with_context(|| format!("cannot remove `{}`", file.display()))?;
		}
	}
	let _ = fs::remove_dir(path.join("references"));
	let _ = fs::remove_dir(path);
	Ok(())
}

fn uninstall_mcp(path: &Path, client: AgentClient) -> anyhow::Result<()> {
	match client {
		AgentClient::Codex => {
			let mut config = fs::read_to_string(path)
				.with_context(|| format!("cannot read `{}`", path.display()))?
				.parse::<toml::Value>()
				.with_context(|| format!("invalid TOML in `{}`", path.display()))?;
			if let Some(servers) = config
				.get_mut("mcp_servers")
				.and_then(toml::Value::as_table_mut)
			{
				servers.remove(SKILL_NAME);
			}
			write_atomic(path, toml::to_string_pretty(&config)?.as_bytes())
		}
		AgentClient::Claude | AgentClient::Gemini => {
			let mut config = read_json_object(path)?;
			if let Some(servers) = config.get_mut("mcpServers").and_then(Value::as_object_mut) {
				servers.remove(SKILL_NAME);
			}
			write_json_atomic(path, &Value::Object(config))
		}
	}
}

fn install_codex_mcp(
	path: &Path,
	command: &str,
	args: &[String],
	managed: bool,
) -> anyhow::Result<bool> {
	let mut config = if path.exists() {
		fs::read_to_string(path)
			.with_context(|| format!("cannot read `{}`", path.display()))?
			.parse::<toml::Value>()
			.with_context(|| format!("invalid TOML in `{}`", path.display()))?
	} else {
		toml::Value::Table(toml::map::Map::new())
	};
	let root = config
		.as_table_mut()
		.context("Codex configuration root must be a TOML table")?;
	let servers = root
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
		("required".to_string(), toml::Value::Boolean(true)),
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
	servers.insert(SKILL_NAME.to_string(), expected);
	write_atomic(path, toml::to_string_pretty(&config)?.as_bytes())?;
	Ok(owned)
}

fn install_json_mcp(
	path: &Path,
	command: &str,
	args: &[String],
	managed: bool,
	client: AgentClient,
) -> anyhow::Result<bool> {
	let mut config = read_json_object(path)?;
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
	servers.insert(SKILL_NAME.to_string(), expected);
	write_json_atomic(path, &Value::Object(config))?;
	Ok(owned)
}

fn read_json_object(path: &Path) -> anyhow::Result<Map<String, Value>> {
	if !path.exists() {
		return Ok(Map::new());
	}
	let value: Value = serde_json::from_slice(
		&fs::read(path).with_context(|| format!("cannot read `{}`", path.display()))?,
	)
	.with_context(|| format!("invalid JSON in `{}`", path.display()))?;
	value
		.as_object()
		.cloned()
		.with_context(|| format!("`{}` must contain a JSON object", path.display()))
}

fn component_status(name: &str, component: &ComponentState, client: AgentClient) -> &'static str {
	let path = Path::new(&component.path);
	if !path.exists() {
		return "missing";
	}
	if name == "skill" {
		if fs::symlink_metadata(path)
			.map(|metadata| metadata.file_type().is_symlink())
			.unwrap_or(false)
			&& skill_matches(path)
		{
			return "linked";
		}
		if skill_matches(path) {
			return if component.owned {
				"installed"
			} else {
				"external"
			};
		}
		return "stale";
	}
	if name == "hooks" {
		return match harness::agent_hook_fingerprint(client, path) {
			Ok(fingerprint) if checksum_bytes(&fingerprint) == component.checksum => {
				if component.owned {
					"installed"
				} else {
					"external"
				}
			}
			Ok(_) => "stale",
			Err(_) => "stale",
		};
	}
	if name == "mcp" {
		return match mcp_entry_checksum(path, client) {
			Ok(checksum) if checksum == component.checksum && component.owned => "installed",
			Ok(checksum) if checksum == component.checksum => "external",
			Ok(_) => "stale",
			Err(_) => "missing",
		};
	}
	"unknown"
}

fn mcp_entry_checksum(path: &Path, client: AgentClient) -> anyhow::Result<String> {
	match client {
		AgentClient::Codex => {
			let config = fs::read_to_string(path)
				.with_context(|| format!("cannot read `{}`", path.display()))?
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
			let config = read_json_object(path)?;
			let entry = config
				.get("mcpServers")
				.and_then(Value::as_object)
				.and_then(|servers| servers.get(SKILL_NAME))
				.context("Code Moniker MCP entry is missing")?;
			Ok(checksum_bytes(&serde_json::to_vec(entry)?))
		}
	}
}

fn skill_matches(path: &Path) -> bool {
	SKILL_FILES.iter().all(|(relative, contents)| {
		fs::read(path.join(relative))
			.map(|actual| actual == contents.as_bytes())
			.unwrap_or(false)
	})
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
		)?;
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

fn read_state(path: &Path) -> anyhow::Result<Option<InstallState>> {
	if !path.exists() {
		return Ok(None);
	}
	let state = serde_json::from_slice(
		&fs::read(path).with_context(|| format!("cannot read `{}`", path.display()))?,
	)
	.with_context(|| format!("invalid agent integration state `{}`", path.display()))?;
	Ok(Some(state))
}

fn persist_state(path: &Path, state: &mut InstallState) -> anyhow::Result<()> {
	state.version = env!("CARGO_PKG_VERSION").to_string();
	write_json_atomic(path, &serde_json::to_value(state)?)
}

fn display_component_version(version: &str) -> &str {
	if version.is_empty() {
		"unknown"
	} else {
		version
	}
}

fn find_owned_skill_state(
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
		let Some(other) = read_state(&candidate_path)? else {
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

fn release_previous_skill_owner(
	owner_state_path: &Path,
	current_component: &ComponentState,
) -> anyhow::Result<()> {
	let mut state = read_state(owner_state_path)?.with_context(|| {
		format!(
			"skill owner state `{}` disappeared",
			owner_state_path.display()
		)
	})?;
	let skill = state
		.components
		.get_mut("skill")
		.context("previous skill owner no longer tracks the skill")?;
	skill.owned = false;
	skill.checksum = current_component.checksum.clone();
	skill.version = current_component.version.clone();
	persist_state(owner_state_path, &mut state)
}

fn transfer_shared_skill_ownership(
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
		let Some(mut other) = read_state(&candidate_path)? else {
			continue;
		};
		if other.client != client_name(client) {
			continue;
		}
		let Some(other_skill) = other.components.get_mut("skill") else {
			continue;
		};
		if other_skill.path != component.path {
			continue;
		}
		other_skill.owned = true;
		other_skill.checksum = component.checksum.clone();
		other_skill.version = component.version.clone();
		persist_state(&candidate_path, &mut other)?;
		return Ok(true);
	}
	Ok(false)
}

fn write_json_atomic(path: &Path, value: &Value) -> anyhow::Result<()> {
	write_atomic(path, &serde_json::to_vec_pretty(value)?)
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
	use tempfile::tempdir;

	use crate::Cli;

	#[test]
	fn embedded_skill_matches_canonical_agent_source() {
		let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
		for (relative, embedded) in SKILL_FILES {
			let canonical = root.join("agents/skills/code-moniker").join(relative);
			assert_eq!(
				fs::read_to_string(&canonical).unwrap(),
				*embedded,
				"run scripts/sync-agent-assets.sh after editing {}",
				canonical.display()
			);
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
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
		)
		.unwrap_err();
		assert!(error.to_string().contains("refusing to replace unmanaged"));
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
		assert!(first.message.contains("installed 5 embedded files"));
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		assert!(skill_matches(&skill));

		let mut managed = InstallState::default();
		managed
			.components
			.insert("skill".to_string(), first.component);
		let second = install_skill(&context, AgentClient::Codex, &managed).unwrap();
		assert!(second.message.contains("installed 5 embedded files"));
		assert!(skill_matches(&skill));
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
			component_status("mcp", &installed.component, AgentClient::Codex),
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
		let mut config = read_json_object(path).unwrap();
		config["mcpServers"]["code-moniker"]["timeout"] = json!(1);
		write_json_atomic(path, &Value::Object(config)).unwrap();
		assert_eq!(
			component_status("mcp", &installed.component, AgentClient::Gemini),
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
			component_status("mcp", &adopted.component, AgentClient::Codex),
			"external"
		);
	}

	#[cfg(unix)]
	#[test]
	fn matching_pre_existing_skill_symlink_remains_external() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		let source = dir.path().join("source");
		for (relative, contents) in SKILL_FILES {
			write_atomic(&source.join(relative), contents.as_bytes()).unwrap();
		}
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: PathBuf::from("/bin/code-moniker"),
			root: dir.path().canonicalize().unwrap(),
		};
		let skill = user_skill_path(&context.home, AgentClient::Codex);
		fs::create_dir_all(skill.parent().unwrap()).unwrap();
		symlink(&source, &skill).unwrap();

		let adopted =
			install_skill(&context, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(!adopted.component.owned);
		assert_eq!(
			component_status("skill", &adopted.component, AgentClient::Codex),
			"linked"
		);
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
		uninstall_skill(path).unwrap();
		assert_eq!(
			fs::read_to_string(path.join("user-note.md")).unwrap(),
			"keep"
		);
		assert!(!path.join("SKILL.md").exists());
	}

	#[test]
	fn uninstall_mcp_preserves_unrelated_json_configuration() {
		let dir = tempdir().unwrap();
		let path = dir.path().join(".mcp.json");
		fs::write(
			&path,
			r#"{"permissions":{"allow":["Read"]},"mcpServers":{"other":{"command":"other"}}}"#,
		)
		.unwrap();
		install_json_mcp(
			&path,
			"/bin/code-moniker",
			&["mcp".to_string(), "/project".to_string()],
			false,
			AgentClient::Claude,
		)
		.unwrap();
		uninstall_mcp(&path, AgentClient::Claude).unwrap();
		let config: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
		assert_eq!(config["permissions"]["allow"][0], "Read");
		assert_eq!(config["mcpServers"]["other"]["command"], "other");
		assert!(config["mcpServers"].get("code-moniker").is_none());
	}

	#[test]
	fn shared_user_skill_transfers_ownership_between_project_states() {
		let dir = tempdir().unwrap();
		let home = dir.path().join("home");
		let root_a = dir.path().join("project-a");
		let root_b = dir.path().join("project-b");
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
		};
		let state_a_path = state_path(&context_a, AgentClient::Codex);
		persist_state(&state_a_path, &mut state_a).unwrap();

		let second =
			install_skill(&context_b, AgentClient::Codex, &InstallState::default()).unwrap();
		assert!(!second.component.owned);
		let mut state_b = InstallState {
			schema: STATE_SCHEMA,
			version: String::new(),
			client: "codex".to_string(),
			root: context_b.root.display().to_string(),
			components: BTreeMap::from([("skill".to_string(), second.component)]),
		};
		let state_b_path = state_path(&context_b, AgentClient::Codex);
		persist_state(&state_b_path, &mut state_b).unwrap();

		let skill_path = Path::new(&state_a.components["skill"].path);
		fs::write(skill_path.join("SKILL.md"), "old version").unwrap();
		let updated = install_skill(&context_b, AgentClient::Codex, &state_b).unwrap();
		assert!(updated.component.owned);
		assert_eq!(
			updated.previous_owner.as_deref(),
			Some(state_a_path.as_path())
		);
		state_b
			.components
			.insert("skill".to_string(), updated.component);
		persist_state(&state_b_path, &mut state_b).unwrap();
		release_previous_skill_owner(
			updated.previous_owner.as_ref().unwrap(),
			&state_b.components["skill"],
		)
		.unwrap();
		assert!(skill_matches(skill_path));
		assert!(!read_state(&state_a_path).unwrap().unwrap().components["skill"].owned);

		let component_b = state_b.components.get("skill").unwrap();
		assert!(
			transfer_shared_skill_ownership(AgentClient::Codex, &state_b_path, component_b)
				.unwrap()
		);
		assert!(Path::new(&component_b.path).exists());
		let transferred = read_state(&state_a_path).unwrap().unwrap();
		assert!(transferred.components["skill"].owned);

		fs::remove_file(&state_b_path).unwrap();
		let component_a = &transferred.components["skill"];
		assert!(
			!transfer_shared_skill_ownership(AgentClient::Codex, &state_a_path, component_a)
				.unwrap()
		);
		uninstall_skill(Path::new(&component_a.path)).unwrap();
		assert!(!Path::new(&component_a.path).exists());
	}

	#[cfg(unix)]
	#[test]
	fn atomic_rewrite_preserves_permissions_and_symlinks() {
		use std::os::unix::fs::{PermissionsExt, symlink};

		let dir = tempdir().unwrap();
		let target = dir.path().join("config-target.json");
		fs::write(&target, b"old").unwrap();
		fs::set_permissions(&target, fs::Permissions::from_mode(0o600)).unwrap();
		let link = dir.path().join("config.json");
		symlink(&target, &link).unwrap();

		write_atomic(&link, b"new").unwrap();

		assert!(
			fs::symlink_metadata(&link)
				.unwrap()
				.file_type()
				.is_symlink()
		);
		assert_eq!(fs::read(&target).unwrap(), b"new");
		assert_eq!(
			fs::metadata(&target).unwrap().permissions().mode() & 0o777,
			0o600
		);
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
	fn status_without_state_is_read_only() {
		let dir = tempdir().unwrap();
		let context = InstallContext {
			home: dir.path().join("home"),
			binary: dir.path().join("code-moniker"),
			root: dir.path().to_path_buf(),
		};
		assert!(!state_path(&context, AgentClient::Codex).exists());
	}
}
