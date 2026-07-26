use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::Context;
use serde_json::Value;

#[cfg(test)]
use crate::args::HookInstallArgs;
use crate::args::{ToolBackend, ToolFilesArgs};

mod agent_hooks;
mod physical_hook;

#[cfg(test)]
use agent_hooks::uninstall_for_agent;
pub(crate) use agent_hooks::{
	AgentHookInstallation, AgentHookRemovalPolicy, AgentHookRollback, agent_hook_fingerprint,
	agent_hook_is_missing, install_for_agent, rollback_agent_hook, uninstall_for_agent_with_policy,
};

const CODEX_MATCHER: &str = "apply_patch|Write|Edit|MultiEdit";
const CLAUDE_MATCHER: &str = "Edit|Write|MultiEdit";
const GEMINI_MATCHER: &str = "write_file|replace|edit";

#[derive(Copy, Clone)]
enum HookBackend {
	Codex,
	Claude,
	Gemini,
}

#[cfg(test)]
fn install<W: Write>(
	args: &HookInstallArgs,
	backend: HookBackend,
	stdout: &mut W,
) -> anyhow::Result<()> {
	let client = match backend {
		HookBackend::Codex => crate::AgentClient::Codex,
		HookBackend::Claude => crate::AgentClient::Claude,
		HookBackend::Gemini => crate::AgentClient::Gemini,
	};
	install_for_agent(args, client, None, stdout)?;
	Ok(())
}

fn backend_name(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => "Codex",
		HookBackend::Claude => "Claude",
		HookBackend::Gemini => "Gemini CLI",
	}
}

fn backend_project_dir(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => ".codex",
		HookBackend::Claude => ".claude",
		HookBackend::Gemini => ".gemini",
	}
}

fn backend_config_file(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => "hooks.json",
		HookBackend::Claude | HookBackend::Gemini => "settings.json",
	}
}

fn backend_env_var(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => "CODEX_PROJECT_DIR",
		HookBackend::Claude => "CLAUDE_PROJECT_DIR",
		HookBackend::Gemini => "GEMINI_PROJECT_DIR",
	}
}

fn backend_hook_event(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex | HookBackend::Claude => "PostToolUse",
		HookBackend::Gemini => "AfterTool",
	}
}

fn backend_matcher(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => CODEX_MATCHER,
		HookBackend::Claude => CLAUDE_MATCHER,
		HookBackend::Gemini => GEMINI_MATCHER,
	}
}

fn backend_hook_name(backend: HookBackend, command: &str) -> Option<String> {
	match backend {
		HookBackend::Codex | HookBackend::Claude => None,
		HookBackend::Gemini => Some(hook_name_from_command(command)),
	}
}

fn backend_settings_command(backend: HookBackend, hook_command: &str) -> String {
	format!(
		"sh -c 'root=\"${{{}:-$(pwd)}}\"; exec \"$root/{}\"'",
		backend_env_var(backend),
		relative_hook_path(backend, hook_command)
	)
}

fn backend_check_format_arg(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => " --format codex-hook",
		HookBackend::Claude | HookBackend::Gemini => "",
	}
}

fn backend_tool_backend_arg(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Codex => "codex",
		HookBackend::Claude => "claude",
		HookBackend::Gemini => "gemini",
	}
}

fn relative_hook_path(backend: HookBackend, hook_command: &str) -> String {
	Path::new(hook_command)
		.file_name()
		.and_then(|name| name.to_str())
		.map(|file| format!("{}/hooks/{file}", backend_project_dir(backend)))
		.unwrap_or_else(|| hook_command.to_string())
}

fn resolve_from_root(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		root.join(path)
	}
}

fn normalize_relative(path: &Path) -> PathBuf {
	path.components().collect()
}

pub(crate) fn hook_file_name(profile: Option<&str>) -> String {
	let slug_src = profile.unwrap_or("check");
	let slug: String = slug_src
		.chars()
		.map(|c| {
			if c.is_ascii_alphanumeric() {
				c.to_ascii_lowercase()
			} else {
				'-'
			}
		})
		.collect::<String>()
		.trim_matches('-')
		.to_string();
	let slug = if slug.is_empty() {
		"profile".to_string()
	} else {
		slug
	};
	format!("code-moniker-{slug}.sh")
}

fn hook_script(
	profile: Option<&str>,
	rules: &Path,
	scope: &Path,
	max_violations: usize,
	backend: HookBackend,
	binary: &str,
) -> String {
	let root_expr = format!(
		r#"root="${{{}:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}}""#,
		backend_env_var(backend)
	);
	let files_setup = hook_files_setup(scope, backend, binary);
	let no_files = hook_no_files(backend);
	let command = hook_check_command(profile, rules, max_violations, backend, binary);
	match backend {
		HookBackend::Codex => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

{files_setup}
{no_files}

exec {command}
"#
		),
		HookBackend::Claude => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

{files_setup}
{no_files}

set +e
output=$({command} 2>&1)
status=$?
set -e

if [ -n "$output" ]; then
	if [ "$status" -eq 0 ]; then
		printf '%s\n' "$output"
	else
		printf '%s\n' "$output" >&2
	fi
fi

if [ "$status" -eq 1 ]; then
	exit 2
fi

exit "$status"
"#
		),
		HookBackend::Gemini => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

{files_setup}
{no_files}

set +e
output=$({command} 2>&1)
status=$?
set -e

if [ "$status" -eq 0 ]; then
	printf '%s\n' '{{"decision":"allow"}}'
	exit 0
fi

if [ -n "$output" ]; then
	printf '%s\n' "$output" >&2
fi

exit 2
"#
		),
	}
}

fn hook_files_setup(scope: &Path, backend: HookBackend, binary: &str) -> String {
	let scope_var = format!("scope={}", sh_quote(&scope.display().to_string()));
	format!(
		r#"input_file=$(mktemp "${{TMPDIR:-/tmp}}/code-moniker-hook.XXXXXX")
trap 'rm -f "$input_file"' EXIT HUP INT TERM
cat > "$input_file"
files=$({binary} agent tool-files {} "$input_file" 2>/dev/null) || {{
	printf '%s\n' 'code-moniker hook could not inspect tool input' >&2
	exit 2
}}

{scope_var}
set -- "$scope"
while IFS= read -r file; do
	[ -n "$file" ] || continue
	# Tool calls also report paths they deleted or renamed away; the check has
	# nothing to read there. Accept absolute, project-relative, and
	# scope-relative paths; `check --file` resolves the same three forms.
	case "$file" in
		/*) existing_file="$file" ;;
		*)
			existing_file="$file"
			[ -f "$existing_file" ] || existing_file="$scope/$file"
			;;
	esac
	[ -f "$existing_file" ] || continue
	set -- "$@" --file "$file"
done <<CODE_MONIKER_FILES
$files
CODE_MONIKER_FILES
"#,
		backend_tool_backend_arg(backend)
	)
}

fn hook_no_files(backend: HookBackend) -> &'static str {
	match backend {
		HookBackend::Gemini => {
			r#"if [ "$#" -eq 1 ]; then
	printf '%s\n' '{"decision":"allow"}'
	exit 0
fi"#
		}
		HookBackend::Codex | HookBackend::Claude => {
			r#"if [ "$#" -eq 1 ]; then
	exit 0
fi"#
		}
	}
}

fn hook_check_command(
	profile: Option<&str>,
	rules: &Path,
	max_violations: usize,
	backend: HookBackend,
	binary: &str,
) -> String {
	let profile_arg = profile
		.map(|profile| format!(" --profile {}", sh_quote(profile)))
		.unwrap_or_default();
	format!(
		r#"{binary} check --rules {}{}{} --max-violations {} "$@""#,
		sh_quote(&rules.display().to_string()),
		profile_arg,
		backend_check_format_arg(backend),
		max_violations,
	)
}

fn binary_shell_command() -> String {
	if let Some(path) = std::env::var_os("CODE_MONIKER_BIN")
		.filter(|path| !path.is_empty())
		.map(PathBuf::from)
	{
		return sh_quote(&path.display().to_string());
	}
	if let Ok(path) = std::env::current_exe()
		&& path.file_name().and_then(|name| name.to_str()) == Some("code-moniker")
	{
		return sh_quote(&path.display().to_string());
	}
	"\"$HOME/.cargo/bin/code-moniker\"".to_string()
}

pub(crate) fn write_tool_files<W: Write>(
	args: &ToolFilesArgs,
	stdout: &mut W,
) -> anyhow::Result<()> {
	let raw = fs::read_to_string(&args.input)
		.with_context(|| format!("cannot read `{}`", args.input.display()))?;
	for file in touched_files_from_hook_input(args.backend, &raw)? {
		writeln!(stdout, "{file}")?;
	}
	Ok(())
}

fn touched_files_from_hook_input(backend: ToolBackend, raw: &str) -> anyhow::Result<Vec<String>> {
	let mut files = Vec::new();
	let value = serde_json::from_str::<Value>(raw).context("hook input is not valid JSON")?;
	collect_json_file_paths(&value, &mut files);
	if backend == ToolBackend::Codex {
		collect_codex_apply_patch_paths(&value, &mut files);
	}
	if backend == ToolBackend::Codex {
		collect_apply_patch_paths(raw, &mut files);
	}
	Ok(dedup_strings(files))
}

fn collect_json_file_paths(value: &Value, files: &mut Vec<String>) {
	collect_tool_payload_paths(value.get("tool_input"), files, false);
	collect_tool_payload_paths(value.get("tool_response"), files, false);
	if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
		for call in calls {
			collect_tool_payload_paths(call.get("tool_input"), files, false);
			collect_tool_payload_paths(call.get("tool_response"), files, false);
		}
	}
}

fn collect_codex_apply_patch_paths(value: &Value, files: &mut Vec<String>) {
	collect_tool_payload_paths(value.get("tool_input"), files, true);
	if let Some(calls) = value.get("tool_calls").and_then(Value::as_array) {
		for call in calls {
			collect_tool_payload_paths(call.get("tool_input"), files, true);
		}
	}
}

fn collect_tool_payload_paths(value: Option<&Value>, files: &mut Vec<String>, command: bool) {
	let Some(value) = value else {
		return;
	};
	collect_object_file_path(value, files);
	collect_apply_patch_operation_paths(value.get("operation"), files);
	if let Some(operations) = value.get("operations").and_then(Value::as_array) {
		for operation in operations {
			collect_apply_patch_operation_paths(Some(operation), files);
		}
	}
	if command && let Some(command) = value.get("command").and_then(Value::as_str) {
		collect_apply_patch_paths(command, files);
	}
}

fn collect_object_file_path(value: &Value, files: &mut Vec<String>) {
	if let Some(path) = value.get("file_path").and_then(Value::as_str) {
		files.push(path.to_string());
	}
	if let Some(path) = value.get("filePath").and_then(Value::as_str) {
		files.push(path.to_string());
	}
}

fn collect_apply_patch_operation_paths(value: Option<&Value>, files: &mut Vec<String>) {
	if let Some(path) = value
		.and_then(|value| value.get("path"))
		.and_then(Value::as_str)
	{
		files.push(path.to_string());
	}
}

fn collect_apply_patch_paths(command: &str, files: &mut Vec<String>) {
	for line in command.lines() {
		for prefix in [
			"*** Add File: ",
			"*** Update File: ",
			"*** Delete File: ",
			"*** Move to: ",
		] {
			if let Some(path) = line.strip_prefix(prefix) {
				let path = path.trim();
				if !path.is_empty() {
					files.push(path.to_string());
				}
			}
		}
	}
}

fn dedup_strings(values: Vec<String>) -> Vec<String> {
	use std::collections::HashSet;
	let mut seen = HashSet::new();
	let mut out = Vec::new();
	for value in values {
		if value.is_empty() || !seen.insert(value.clone()) {
			continue;
		}
		out.push(value);
	}
	out
}

fn sh_quote(value: &str) -> String {
	let escaped = value.replace('\'', r#"'\''"#);
	format!("'{escaped}'")
}

fn hook_name_from_command(command: &str) -> String {
	let script = command
		.split('/')
		.next_back()
		.and_then(|tail| tail.split('"').next())
		.unwrap_or(command);
	Path::new(script)
		.file_stem()
		.and_then(|stem| stem.to_str())
		.unwrap_or("code-moniker-check")
		.to_string()
}

#[cfg(all(test, unix))]
fn make_executable(path: &Path) -> anyhow::Result<()> {
	use std::os::unix::fs::PermissionsExt;
	let mut perms = fs::metadata(path)?.permissions();
	perms.set_mode(perms.mode() | 0o755);
	fs::set_permissions(path, perms)?;
	Ok(())
}

#[cfg(all(test, not(unix)))]
fn make_executable(_path: &Path) -> anyhow::Result<()> {
	Ok(())
}

#[cfg(test)]
mod tests {
	use std::path::PathBuf;

	use tempfile::tempdir;

	use super::{HookBackend, install};
	use crate::args::HookInstallArgs;

	fn write_architecture_profile(root: &std::path::Path) {
		std::fs::write(
			root.join(".code-moniker.toml"),
			r#"
[profiles.architecture]
enable = [".*"]
"#,
		)
		.unwrap();
		std::fs::create_dir(root.join("src")).unwrap();
	}

	#[test]
	fn tool_files_extracts_claude_and_gemini_file_path_inputs() {
		let claude = r#"{
			"tool_name": "Edit",
			"tool_input": {
				"file_path": "/repo/src/order.ts",
				"old_string": "old",
				"new_string": "new"
			},
			"tool_response": {"filePath": "/repo/src/order.ts"}
		}"#;
		assert_eq!(
			super::touched_files_from_hook_input(crate::ToolBackend::Claude, claude).unwrap(),
			vec!["/repo/src/order.ts"]
		);

		let gemini = r#"{
			"tool_name": "replace",
			"tool_input": {
				"file_path": "src/service.go",
				"old_string": "old",
				"new_string": "new"
			}
		}"#;
		assert_eq!(
			super::touched_files_from_hook_input(crate::ToolBackend::Gemini, gemini).unwrap(),
			vec!["src/service.go"]
		);
	}

	#[test]
	fn tool_files_extracts_codex_apply_patch_paths() {
		let codex = r#"{
			"tool_name": "apply_patch",
			"tool_input": {
				"command": "*** Begin Patch\n*** Update File: crates/cli/src/lib.rs\n*** Move to: crates/cli/src/runner.rs\n@@\n*** Delete File: old.ts\n*** End Patch\n"
			},
			"tool_response": {}
		}"#;

		assert_eq!(
			super::touched_files_from_hook_input(crate::ToolBackend::Codex, codex).unwrap(),
			vec![
				"crates/cli/src/lib.rs",
				"crates/cli/src/runner.rs",
				"old.ts"
			]
		);
	}

	#[test]
	fn tool_files_extracts_codex_apply_patch_paths_inside_tool_calls() {
		let codex = r#"{
			"tool_calls": [
				{
					"tool_name": "apply_patch",
					"tool_input": {
						"command": "*** Begin Patch\n*** Update File: crates/cli/src/hooks.rs\n*** End Patch\n"
					}
				}
			]
		}"#;

		assert_eq!(
			super::touched_files_from_hook_input(crate::ToolBackend::Codex, codex).unwrap(),
			vec!["crates/cli/src/hooks.rs"]
		);
	}

	#[test]
	fn tool_files_rejects_invalid_json_instead_of_skipping_the_check() {
		let error = super::touched_files_from_hook_input(crate::ToolBackend::Codex, "{not-json")
			.unwrap_err()
			.to_string();

		assert!(error.contains("hook input is not valid JSON"));
	}

	#[test]
	fn tool_files_accepts_valid_json_without_touched_files() {
		assert!(
			super::touched_files_from_hook_input(crate::ToolBackend::Claude, "{}")
				.unwrap()
				.is_empty()
		);
	}

	#[test]
	fn codex_hook_installs_direct_code_moniker_hook() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let script =
			std::fs::read_to_string(dir.path().join(".codex/hooks/code-moniker-check.sh")).unwrap();
		assert!(script.contains("\"$HOME/.cargo/bin/code-moniker\" check"));
		assert!(script.contains("--format codex-hook"));
		assert!(script.contains("--max-violations 10"));
		assert!(!script.contains("hookSpecificOutput"));
		assert!(!script.contains("python3"));
		assert!(script.contains("$HOME/.cargo/bin/code-moniker"));
		assert!(!script.contains("--profile"));
		assert!(script.contains("'.'"));
		assert!(!script.contains("npm"));
	}

	#[test]
	fn agent_hook_install_skips_performance_template_and_uninstalls_cleanly() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let mut stdout = Vec::new();
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut stdout).unwrap();
		assert!(installed.owned);
		let hook = dir.path().join(".codex/hooks/code-moniker-check.sh");
		assert!(hook.exists());
		assert_eq!(installed.path, hook);
		assert_eq!(
			installed.fingerprint,
			super::agent_hook_fingerprint(crate::AgentClient::Codex, &hook).unwrap()
		);
		assert!(
			!dir.path()
				.join(".codex/code-moniker-performance.md")
				.exists()
		);

		super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Codex,
			&hook,
			&installed.fingerprint,
		)
		.unwrap();
		assert!(!hook.exists());
		let settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(dir.path().join(".codex/hooks.json")).unwrap())
				.unwrap();
		assert!(settings.get("hooks").is_none());
	}

	#[cfg(unix)]
	#[test]
	fn agent_hook_rejects_linked_hooks_directory_without_writing_target() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let client_dir = dir.path().join(".codex");
		let external = dir.path().join("external-hooks");
		std::fs::create_dir_all(&client_dir).unwrap();
		std::fs::create_dir_all(&external).unwrap();
		std::fs::write(external.join("sentinel"), "unchanged").unwrap();
		symlink(&external, client_dir.join("hooks")).unwrap();
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};

		let error =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap_err()
				.to_string();

		assert!(error.contains("is not a physical directory"));
		assert_eq!(
			std::fs::read_to_string(external.join("sentinel")).unwrap(),
			"unchanged"
		);
		assert!(!external.join("code-moniker-check.sh").exists());
		assert!(
			std::fs::symlink_metadata(client_dir.join("hooks"))
				.unwrap()
				.file_type()
				.is_symlink()
		);
	}

	#[cfg(unix)]
	#[test]
	fn agent_hook_rejects_linked_script_and_configuration() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let hooks = dir.path().join(".claude/hooks");
		std::fs::create_dir_all(&hooks).unwrap();
		let external_script = dir.path().join("external-hook.sh");
		std::fs::write(&external_script, "external script").unwrap();
		let hook = hooks.join("code-moniker-check.sh");
		symlink(&external_script, &hook).unwrap();
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};

		assert!(
			super::install_for_agent(&args, crate::AgentClient::Claude, None, &mut Vec::new(),)
				.unwrap_err()
				.to_string()
				.contains("refusing linked hook path component")
		);
		assert_eq!(
			std::fs::read_to_string(&external_script).unwrap(),
			"external script"
		);

		std::fs::remove_file(&hook).unwrap();
		let external_config = dir.path().join("external-settings.json");
		std::fs::write(&external_config, "{}").unwrap();
		symlink(&external_config, dir.path().join(".claude/settings.json")).unwrap();
		assert!(
			super::install_for_agent(&args, crate::AgentClient::Claude, None, &mut Vec::new(),)
				.unwrap_err()
				.to_string()
				.contains("refusing linked hook configuration path component")
		);
		assert_eq!(std::fs::read_to_string(&external_config).unwrap(), "{}");
	}

	#[cfg(unix)]
	#[test]
	fn linked_hooks_directory_drift_blocks_fingerprint_and_uninstall() {
		use std::os::unix::fs::symlink;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Gemini, None, &mut Vec::new())
				.unwrap();
		let hooks = installed.path.parent().unwrap();
		let moved_hooks = dir.path().join("moved-hooks");
		std::fs::rename(hooks, &moved_hooks).unwrap();
		symlink(&moved_hooks, hooks).unwrap();

		assert!(
			super::agent_hook_fingerprint(crate::AgentClient::Gemini, &installed.path).is_err()
		);
		assert!(
			super::uninstall_for_agent(
				dir.path(),
				crate::AgentClient::Gemini,
				&installed.path,
				&installed.fingerprint,
			)
			.is_err()
		);
		assert!(
			moved_hooks
				.join(installed.path.file_name().unwrap())
				.exists()
		);
	}

	#[cfg(unix)]
	#[test]
	fn agent_hook_uninstall_restores_script_when_config_write_fails() {
		use std::os::unix::fs::PermissionsExt;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		let client_dir = dir.path().join(".codex");
		let config_path = client_dir.join("hooks.json");
		let script_before = std::fs::read(&installed.path).unwrap();
		let config_before = std::fs::read(&config_path).unwrap();
		let original_permissions = std::fs::metadata(&client_dir).unwrap().permissions();
		std::fs::set_permissions(&client_dir, std::fs::Permissions::from_mode(0o555)).unwrap();

		let result = super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Codex,
			&installed.path,
			&installed.fingerprint,
		);

		std::fs::set_permissions(&client_dir, original_permissions).unwrap();
		assert!(result.is_err());
		assert_eq!(std::fs::read(&installed.path).unwrap(), script_before);
		assert_eq!(std::fs::read(&config_path).unwrap(), config_before);
	}

	#[test]
	fn agent_hook_uninstall_refuses_script_change_after_preflight() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		std::fs::write(&installed.path, "user replacement").unwrap();

		let error = super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Codex,
			&installed.path,
			&installed.fingerprint,
		)
		.unwrap_err()
		.to_string();

		assert!(error.contains("changed after uninstall preflight"));
		assert_eq!(
			std::fs::read_to_string(&installed.path).unwrap(),
			"user replacement"
		);
		let config: serde_json::Value =
			serde_json::from_slice(&std::fs::read(dir.path().join(".codex/hooks.json")).unwrap())
				.unwrap();
		assert_eq!(config["hooks"]["PostToolUse"].as_array().unwrap().len(), 1);
	}

	#[cfg(unix)]
	#[test]
	fn agent_hook_fingerprint_rejects_non_executable_mode() {
		use std::os::unix::fs::PermissionsExt;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		std::fs::set_permissions(&installed.path, std::fs::Permissions::from_mode(0o644)).unwrap();

		let error = super::agent_hook_fingerprint(crate::AgentClient::Codex, &installed.path)
			.unwrap_err()
			.to_string();
		assert!(error.contains("is not executable"));
		assert!(
			super::uninstall_for_agent(
				dir.path(),
				crate::AgentClient::Codex,
				&installed.path,
				&installed.fingerprint,
			)
			.is_err()
		);
		assert!(installed.path.exists());
	}

	#[test]
	fn rollback_keeps_script_when_configuration_cannot_be_restored() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		let config_path = dir.path().join(".codex/hooks.json");
		std::fs::write(&config_path, br#"{"concurrent":true}"#).unwrap();

		let error = super::rollback_agent_hook(installed.rollback.as_ref().unwrap())
			.unwrap_err()
			.to_string();

		assert!(error.contains("cannot roll back hook configuration"));
		assert!(installed.path.exists());
		assert_eq!(
			std::fs::read(&config_path).unwrap(),
			br#"{"concurrent":true}"#
		);
	}

	#[test]
	fn uninstall_rollback_does_not_register_a_concurrent_script() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		let rollback = super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Codex,
			&installed.path,
			&installed.fingerprint,
		)
		.unwrap();
		std::fs::write(&installed.path, "concurrent script").unwrap();

		let error = super::rollback_agent_hook(&rollback)
			.unwrap_err()
			.to_string();

		assert!(error.contains("cannot roll back hook script"));
		assert_eq!(
			std::fs::read_to_string(&installed.path).unwrap(),
			"concurrent script"
		);
		let config: serde_json::Value =
			serde_json::from_slice(&std::fs::read(dir.path().join(".codex/hooks.json")).unwrap())
				.unwrap();
		assert!(config.get("hooks").is_none());
	}

	#[test]
	fn agent_hook_detects_registration_drift_and_refuses_unmanaged_script_replacement() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let mut stdout = Vec::new();
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Claude, None, &mut stdout).unwrap();
		let adopted =
			super::install_for_agent(&args, crate::AgentClient::Claude, None, &mut stdout).unwrap();
		assert!(!adopted.owned);
		let different_profile = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: Some("architecture".to_string()),
			scope: ".".into(),
			max_violations: 10,
		};
		let error = super::install_for_agent(
			&different_profile,
			crate::AgentClient::Claude,
			Some(&installed.path),
			&mut stdout,
		)
		.unwrap_err();
		assert!(error.to_string().contains("uninstall the hooks component"));

		let settings_path = dir.path().join(".claude/settings.json");
		let mut settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
		settings["hooks"]["PostToolUse"][0]["matcher"] = serde_json::json!("Write");
		std::fs::write(
			&settings_path,
			serde_json::to_vec_pretty(&settings).unwrap(),
		)
		.unwrap();
		assert!(
			super::agent_hook_fingerprint(crate::AgentClient::Claude, &installed.path).is_err()
		);

		let invalid_root = dir.path().join("invalid-config");
		std::fs::create_dir_all(invalid_root.join(".codex")).unwrap();
		std::fs::write(
			invalid_root.join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		std::fs::write(invalid_root.join(".codex/hooks.json"), r#"{"hooks":[]}"#).unwrap();
		let invalid_args = crate::HookInstallArgs {
			root: invalid_root.clone(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		assert!(
			super::install_for_agent(&invalid_args, crate::AgentClient::Codex, None, &mut stdout)
				.is_err()
		);
		assert!(
			!invalid_root
				.join(".codex/hooks/code-moniker-check.sh")
				.exists()
		);

		let unmanaged_root = dir.path().join("unmanaged");
		std::fs::create_dir_all(unmanaged_root.join(".codex/hooks")).unwrap();
		std::fs::write(
			unmanaged_root.join(".code-moniker.toml"),
			"default_rules = true\n",
		)
		.unwrap();
		let unmanaged_hook = unmanaged_root.join(".codex/hooks/code-moniker-check.sh");
		std::fs::write(&unmanaged_hook, "user hook").unwrap();
		let unmanaged_args = crate::HookInstallArgs {
			root: unmanaged_root,
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let error = super::install_for_agent(
			&unmanaged_args,
			crate::AgentClient::Codex,
			None,
			&mut stdout,
		)
		.unwrap_err();
		assert!(error.to_string().contains("refusing to replace unmanaged"));
		assert_eq!(
			std::fs::read_to_string(unmanaged_hook).unwrap(),
			"user hook"
		);
	}

	#[test]
	fn agent_hook_uninstall_removes_only_its_exact_registration() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let mut stdout = Vec::new();
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Gemini, None, &mut stdout).unwrap();
		let settings_path = dir.path().join(".gemini/settings.json");
		let mut settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
		settings["hooks"]["AfterTool"]
			.as_array_mut()
			.unwrap()
			.push(serde_json::json!({
				"matcher": "write_file",
				"hooks": [{
					"type": "command",
					"name": "code-moniker-other",
					"command": "sh -c 'exec \"$root/.gemini/hooks/code-moniker-other.sh\"'"
				}]
			}));
		std::fs::write(
			&settings_path,
			serde_json::to_vec_pretty(&settings).unwrap(),
		)
		.unwrap();

		super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Gemini,
			&installed.path,
			&installed.fingerprint,
		)
		.unwrap();

		let settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(settings_path).unwrap()).unwrap();
		let entries = settings["hooks"]["AfterTool"].as_array().unwrap();
		assert_eq!(entries.len(), 1);
		assert!(
			entries[0]["hooks"][0]["command"]
				.as_str()
				.unwrap()
				.contains("code-moniker-other.sh")
		);
	}

	#[test]
	fn agent_hook_uninstall_rejects_a_registration_duplicated_after_preflight() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = crate::HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: ".code-moniker.toml".into(),
			profile: None,
			scope: ".".into(),
			max_violations: 10,
		};
		let installed =
			super::install_for_agent(&args, crate::AgentClient::Codex, None, &mut Vec::new())
				.unwrap();
		let settings_path = dir.path().join(".codex/hooks.json");
		let mut settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(&settings_path).unwrap()).unwrap();
		let entries = settings["hooks"]["PostToolUse"].as_array_mut().unwrap();
		entries.push(entries[0].clone());
		std::fs::write(
			&settings_path,
			serde_json::to_vec_pretty(&settings).unwrap(),
		)
		.unwrap();

		let error = super::uninstall_for_agent(
			dir.path(),
			crate::AgentClient::Codex,
			&installed.path,
			&installed.fingerprint,
		)
		.unwrap_err()
		.to_string();

		assert!(error.contains("missing, duplicated, or modified"));
		assert!(installed.path.exists());
		let settings: serde_json::Value =
			serde_json::from_slice(&std::fs::read(settings_path).unwrap()).unwrap();
		assert_eq!(
			settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
			2
		);
	}

	#[test]
	fn codex_hook_uses_code_moniker_codex_hook_format_directly() {
		use std::io::Write as _;
		use std::process::{Command, Stdio};

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::write(dir.path().join("src/touched.ts"), "class lower_bad {}\n").unwrap();
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			"#!/usr/bin/env sh\nif [ \"$1\" = \"agent\" ]; then printf '%s\\n' 'src/touched.ts'; exit 0; fi\nprintf '%s\\n' \"$*\"\nprintf '%s\\n' '{\"decision\":\"block\",\"reason\":\"violation from fake checker\"}'\nexit 0\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let mut child = Command::new(dir.path().join(".codex/hooks/code-moniker-check.sh"))
			.env("CODEX_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"apply_patch"}"#)
			.unwrap();
		let output = child.wait_with_output().unwrap();

		assert_eq!(output.status.code(), Some(0));
		assert!(String::from_utf8(output.stderr).unwrap().is_empty());
		let stdout = String::from_utf8(output.stdout).unwrap();
		assert!(stdout.contains("--format codex-hook"), "{stdout}");
		assert!(stdout.contains("--max-violations 10"), "{stdout}");
		assert!(stdout.contains("--file src/touched.ts"), "{stdout}");
		assert!(stdout.contains("violation from fake checker"), "{stdout}");
	}

	#[test]
	fn codex_hook_limits_default_matcher_to_local_write_tools() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: Some("architecture".to_string()),
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		let matcher = settings["hooks"]["PostToolUse"][0]["matcher"]
			.as_str()
			.unwrap();
		assert_eq!(matcher, "apply_patch|Write|Edit|MultiEdit");
		assert!(!matcher.to_ascii_lowercase().contains("mcp"));
		assert!(!matcher.to_ascii_lowercase().contains("custom"));
	}

	#[test]
	fn codex_hook_preserves_existing_settings_entries() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::create_dir(dir.path().join(".codex")).unwrap();
		std::fs::write(
			dir.path().join(".codex/hooks.json"),
			r#"{
  "model": "gpt-5",
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Read",
        "hooks": [
          {
            "type": "command",
            "command": "echo read"
          }
        ]
      }
    ]
  }
}"#,
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(settings["model"], "gpt-5");
		assert_eq!(
			settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
			2
		);
		assert_eq!(
			settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
			"echo read"
		);
	}

	#[test]
	fn codex_hook_preserves_unmanaged_previous_profile_entry() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::create_dir(dir.path().join(".codex")).unwrap();
		let old_hook = dir
			.path()
			.join(".codex/hooks/code-moniker-architecture.sh")
			.display()
			.to_string();
		std::fs::write(
			dir.path().join(".codex/hooks.json"),
			format!(
				r#"{{
  "hooks": {{
    "PostToolUse": [
      {{
        "matcher": "Read",
        "hooks": [
          {{
            "type": "command",
            "command": "echo read"
          }}
        ]
      }},
      {{
        "matcher": "apply_patch|Write|Edit|MultiEdit",
        "hooks": [
          {{
            "type": "command",
            "command": "{old_hook}"
          }}
        ]
      }}
    ]
  }}
}}"#
			),
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
		assert_eq!(post.len(), 3);
		assert_eq!(post[0]["hooks"][0]["command"], "echo read");
		assert_eq!(post[1]["hooks"][0]["command"], old_hook);
		let command = post[2]["hooks"][0]["command"].as_str().unwrap();
		assert!(command.contains(".codex/hooks/code-moniker-check.sh"));
	}

	#[test]
	fn codex_hook_quotes_shell_arguments_and_uses_profile_script_name() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join("rules $x.toml"),
			r#"
[profiles."fast profile"]
enable = [".*"]
"#,
		)
		.unwrap();
		std::fs::create_dir(dir.path().join("src $x")).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from("rules $x.toml"),
			profile: Some("fast profile".to_string()),
			scope: PathBuf::from("src $x"),
			max_violations: 3,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let script =
			std::fs::read_to_string(dir.path().join(".codex/hooks/code-moniker-fast-profile.sh"))
				.unwrap();
		assert!(script.contains("--rules 'rules $x.toml'"));
		assert!(script.contains("--profile 'fast profile'"));
		assert!(script.contains("--max-violations 3"));
		assert!(script.contains("'src $x'"));
		let hooks: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(
			hooks["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
			"sh -c 'root=\"${CODEX_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.codex/hooks/code-moniker-fast-profile.sh\"'"
		);
	}

	#[test]
	fn codex_hook_requires_requested_profile() {
		let dir = tempdir().unwrap();
		std::fs::write(dir.path().join(".code-moniker.toml"), "").unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: Some("architecture".to_string()),
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Codex;
		let mut stdout = Vec::new();

		let error = install(&args, backend, &mut stdout).unwrap_err();
		assert!(format!("{error:#}").contains("profile `architecture` is not defined"));
	}

	#[test]
	fn claude_hook_installs_project_local_settings_and_hook() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Claude;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let script =
			std::fs::read_to_string(dir.path().join(".claude/hooks/code-moniker-check.sh"))
				.unwrap();
		assert!(script.contains("CLAUDE_PROJECT_DIR"));
		assert!(script.contains("\"$HOME/.cargo/bin/code-moniker\" check"));
		assert!(script.contains("exit 2"));
		assert!(!script.contains("npm"));

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(
			settings["hooks"]["PostToolUse"][0]["matcher"],
			"Edit|Write|MultiEdit"
		);
		assert_eq!(
			settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
			"sh -c 'root=\"${CLAUDE_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.claude/hooks/code-moniker-check.sh\"'"
		);
	}

	#[test]
	fn claude_hook_maps_violations_to_stderr_exit_two() {
		use std::io::Write as _;
		use std::process::{Command, Stdio};

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::write(dir.path().join("src/touched.ts"), "class lower_bad {}\n").unwrap();
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			"#!/usr/bin/env sh\nif [ \"$1\" = \"agent\" ]; then printf '%s\\n' 'src/touched.ts'; exit 0; fi\necho 'violation from fake checker'\nexit 1\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Claude;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let mut child = Command::new(dir.path().join(".claude/hooks/code-moniker-check.sh"))
			.env("CLAUDE_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"Edit"}"#)
			.unwrap();
		let output = child.wait_with_output().unwrap();

		assert_eq!(output.status.code(), Some(2));
		assert!(String::from_utf8(output.stdout).unwrap().is_empty());
		assert_eq!(
			String::from_utf8(output.stderr).unwrap().trim(),
			"violation from fake checker"
		);
	}

	#[test]
	fn claude_hook_keeps_scope_relative_tool_paths_for_check_resolution() {
		use std::io::Write as _;
		use std::process::{Command, Stdio};

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::write(dir.path().join("src/order.ts"), "class lower_bad {}\n").unwrap();
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			"#!/usr/bin/env sh\nif [ \"$1\" = \"agent\" ]; then printf '%s\\n' 'order.ts'; exit 0; fi\nprintf '%s\\n' \"$*\"\nexit 0\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("src"),
			max_violations: 10,
		};
		let backend = HookBackend::Claude;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let mut child = Command::new(dir.path().join(".claude/hooks/code-moniker-check.sh"))
			.env("CLAUDE_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"Edit"}"#)
			.unwrap();
		let output = child.wait_with_output().unwrap();

		assert_eq!(output.status.code(), Some(0));
		let stdout = String::from_utf8(output.stdout).unwrap();
		assert!(
			stdout.contains(
				"check --rules .code-moniker.toml --max-violations 10 src --file order.ts"
			),
			"{stdout}"
		);
	}

	#[test]
	fn claude_hook_accepts_absolute_project_and_scope_relative_paths() {
		use std::io::Write as _;
		use std::process::{Command, Stdio};

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let absolute = dir.path().join("src/absolute.ts");
		std::fs::write(&absolute, "class Absolute {}\n").unwrap();
		std::fs::write(dir.path().join("src/project.ts"), "class Project {}\n").unwrap();
		std::fs::write(dir.path().join("src/scope.ts"), "class Scope {}\n").unwrap();
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			format!(
				"#!/usr/bin/env sh\nif [ \"$1\" = \"agent\" ]; then\nprintf '%s\\n' '{}' 'src/project.ts' 'scope.ts' 'gone.ts'\nexit 0\nfi\nprintf '%s\\n' \"$*\"\nexit 0\n",
				absolute.display()
			),
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("src"),
			max_violations: 10,
		};
		let mut stdout = Vec::new();

		install(&args, HookBackend::Claude, &mut stdout).unwrap();

		let mut child = Command::new(dir.path().join(".claude/hooks/code-moniker-check.sh"))
			.env("CLAUDE_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"Edit"}"#)
			.unwrap();
		let output = child.wait_with_output().unwrap();

		assert_eq!(output.status.code(), Some(0));
		let stdout = String::from_utf8(output.stdout).unwrap();
		assert!(
			stdout.contains(&format!("--file {}", absolute.display())),
			"{stdout}"
		);
		assert!(stdout.contains("--file src/project.ts"), "{stdout}");
		assert!(stdout.contains("--file scope.ts"), "{stdout}");
		assert!(!stdout.contains("gone.ts"), "{stdout}");
	}

	#[test]
	fn gemini_hook_installs_project_local_settings_and_hook() {
		let tmp = tempdir().unwrap();
		let root = tmp.path().join("space project");
		std::fs::create_dir(&root).unwrap();
		write_architecture_profile(&root);
		let args = HookInstallArgs {
			root: root.clone(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Gemini;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let script =
			std::fs::read_to_string(root.join(".gemini/hooks/code-moniker-check.sh")).unwrap();
		assert!(script.contains("GEMINI_PROJECT_DIR"));
		assert!(script.contains("\"$HOME/.cargo/bin/code-moniker\" check"));
		assert!(script.contains("--max-violations 10"));
		assert!(script.contains(r#"{"decision":"allow"}"#));
		assert!(script.contains("exit 2"));

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(root.join(".gemini/settings.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(
			settings["hooks"]["AfterTool"][0]["matcher"],
			"write_file|replace|edit"
		);
		assert_eq!(
			settings["hooks"]["AfterTool"][0]["hooks"][0]["name"],
			"code-moniker-check"
		);
		assert_eq!(
			settings["hooks"]["AfterTool"][0]["hooks"][0]["type"],
			"command"
		);
		assert_eq!(
			settings["hooks"]["AfterTool"][0]["hooks"][0]["command"],
			"sh -c 'root=\"${GEMINI_PROJECT_DIR:-$(pwd)}\"; exec \"$root/.gemini/hooks/code-moniker-check.sh\"'"
		);
	}

	#[test]
	fn gemini_hook_maps_clean_and_violating_runs_to_hook_contract() {
		use std::io::Write as _;
		use std::process::{Command, Stdio};

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::write(dir.path().join("src/touched.ts"), "class lower_bad {}\n").unwrap();
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			r#"#!/usr/bin/env sh
if [ "$1" = "agent" ]; then
	printf '%s\n' 'src/touched.ts'
	exit 0
fi
if [ "${CODE_MONIKER_FAKE_FAIL:-}" = "1" ]; then
	echo 'violation from fake checker'
	exit 1
fi
echo 'clean summary that must not reach hook stdout'
exit 0
"#,
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Gemini;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();
		let script = dir.path().join(".gemini/hooks/code-moniker-check.sh");

		let mut child = Command::new(&script)
			.env("GEMINI_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"replace"}"#)
			.unwrap();
		let clean = child.wait_with_output().unwrap();
		assert_eq!(clean.status.code(), Some(0));
		assert_eq!(
			String::from_utf8(clean.stdout).unwrap().trim(),
			r#"{"decision":"allow"}"#
		);
		assert!(String::from_utf8(clean.stderr).unwrap().is_empty());

		let mut child = Command::new(&script)
			.env("GEMINI_PROJECT_DIR", dir.path())
			.env("HOME", dir.path())
			.env("CODE_MONIKER_FAKE_FAIL", "1")
			.stdin(Stdio::piped())
			.stdout(Stdio::piped())
			.stderr(Stdio::piped())
			.spawn()
			.unwrap();
		child
			.stdin
			.as_mut()
			.unwrap()
			.write_all(br#"{"tool_name":"replace"}"#)
			.unwrap();
		let blocked = child.wait_with_output().unwrap();
		assert_eq!(blocked.status.code(), Some(2));
		assert!(String::from_utf8(blocked.stdout).unwrap().is_empty());
		assert_eq!(
			String::from_utf8(blocked.stderr).unwrap().trim(),
			"violation from fake checker"
		);
	}

	#[test]
	fn claude_hook_preserves_existing_settings_entries() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		std::fs::create_dir(dir.path().join(".claude")).unwrap();
		std::fs::write(
			dir.path().join(".claude/settings.json"),
			r#"{
  "permissions": {
    "allow": ["Bash(cargo test:*)"]
  },
  "hooks": {
    "PostToolUse": [
      {
        "matcher": "Read",
        "hooks": [
          {
            "type": "command",
            "command": "echo read"
          }
        ]
      }
    ]
  }
}"#,
		)
		.unwrap();
		let args = HookInstallArgs {
			root: dir.path().to_path_buf(),
			rules: PathBuf::from(".code-moniker.toml"),
			profile: None,
			scope: PathBuf::from("."),
			max_violations: 10,
		};
		let backend = HookBackend::Claude;
		let mut stdout = Vec::new();

		install(&args, backend, &mut stdout).unwrap();

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".claude/settings.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(settings["permissions"]["allow"][0], "Bash(cargo test:*)");
		assert_eq!(
			settings["hooks"]["PostToolUse"].as_array().unwrap().len(),
			2
		);
		assert_eq!(
			settings["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
			"echo read"
		);
	}
}
