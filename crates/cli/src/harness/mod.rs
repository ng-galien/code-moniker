use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

use crate::Exit;
use crate::args::{
	CodexHarnessArgs, HarnessArgs, HarnessCommand, HarnessToolBackend, HarnessToolFilesArgs,
};

const CODEX_MATCHER: &str = "apply_patch|Write|Edit|MultiEdit";
const CLAUDE_MATCHER: &str = "Edit|Write|MultiEdit";
const GEMINI_MATCHER: &str = "write_file|replace|edit";

#[derive(Copy, Clone)]
enum HarnessBackend {
	Codex,
	Claude,
	Gemini,
}

pub fn run<W1: Write, W2: Write>(args: &HarnessArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let result = match &args.command {
		HarnessCommand::Codex(args) => install(args, HarnessBackend::Codex, stdout),
		HarnessCommand::Claude(args) => install(args, HarnessBackend::Claude, stdout),
		HarnessCommand::Gemini(args) => install(args, HarnessBackend::Gemini, stdout),
		HarnessCommand::ToolFiles(args) => write_tool_files(args, stdout),
	};
	match result {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn install<W: Write>(
	args: &CodexHarnessArgs,
	backend: HarnessBackend,
	stdout: &mut W,
) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let rules = resolve_from_root(&root, &args.rules);
	let scope = normalize_relative(&args.scope);
	let cfg = code_moniker_check::load_with_overrides(Some(&rules))?;
	if let Some(profile) = &args.profile
		&& !cfg.profiles.contains_key(profile)
	{
		bail!(
			"profile `{profile}` is not defined in `{}`; add [profiles.{profile}] before installing the live harness",
			rules.display()
		);
	}

	let project_dir = root.join(backend_project_dir(backend));
	let hooks_dir = project_dir.join("hooks");
	fs::create_dir_all(&hooks_dir)
		.with_context(|| format!("cannot create `{}`", hooks_dir.display()))?;

	let hook_file = hook_file_name(args.profile.as_deref());
	let hook_path = hooks_dir.join(&hook_file);
	fs::write(
		&hook_path,
		hook_script(
			args.profile.as_deref(),
			&args.rules,
			&scope,
			args.max_violations,
			backend,
		),
	)
	.with_context(|| format!("cannot write `{}`", hook_path.display()))?;
	make_executable(&hook_path)?;

	let config_path = project_dir.join(backend_config_file(backend));
	let config = read_json_object(&config_path)?;
	let hook_command = hook_path.display().to_string();
	let settings_command = backend_settings_command(backend, &hook_command);
	let config = upsert_tool_hook(
		config,
		ToolHookUpsert {
			path: &config_path,
			command: &settings_command,
			event: backend_hook_event(backend),
			matcher: backend_matcher(backend),
			name: backend_hook_name(backend, &settings_command),
			project_dir: backend_project_dir(backend),
		},
	)?;
	fs::write(&config_path, serde_json::to_vec_pretty(&config)?)
		.with_context(|| format!("cannot write `{}`", config_path.display()))?;
	fs::write(
		project_dir.join("code-moniker-performance.md"),
		performance_report(args.profile.as_deref(), &scope, backend),
	)
	.with_context(|| {
		format!(
			"cannot write {} hook performance template",
			backend_name(backend)
		)
	})?;

	match args.profile.as_deref() {
		Some(profile) => writeln!(
			stdout,
			"Installed {} live harness for profile `{profile}` on `{}`.",
			backend_name(backend),
			scope.display()
		)?,
		None => writeln!(
			stdout,
			"Installed {} live harness on `{}`.",
			backend_name(backend),
			scope.display()
		)?,
	}
	writeln!(stdout, "Hook: {}", hook_path.display())?;
	writeln!(
		stdout,
		"{} config: {}",
		backend_name(backend),
		config_path.display()
	)?;
	Ok(())
}

fn backend_name(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => "Codex",
		HarnessBackend::Claude => "Claude",
		HarnessBackend::Gemini => "Gemini CLI",
	}
}

fn backend_project_dir(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => ".codex",
		HarnessBackend::Claude => ".claude",
		HarnessBackend::Gemini => ".gemini",
	}
}

fn backend_config_file(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => "hooks.json",
		HarnessBackend::Claude | HarnessBackend::Gemini => "settings.json",
	}
}

fn backend_env_var(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => "CODEX_PROJECT_DIR",
		HarnessBackend::Claude => "CLAUDE_PROJECT_DIR",
		HarnessBackend::Gemini => "GEMINI_PROJECT_DIR",
	}
}

fn backend_hook_event(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex | HarnessBackend::Claude => "PostToolUse",
		HarnessBackend::Gemini => "AfterTool",
	}
}

fn backend_matcher(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => CODEX_MATCHER,
		HarnessBackend::Claude => CLAUDE_MATCHER,
		HarnessBackend::Gemini => GEMINI_MATCHER,
	}
}

fn backend_hook_name(backend: HarnessBackend, command: &str) -> Option<String> {
	match backend {
		HarnessBackend::Codex | HarnessBackend::Claude => None,
		HarnessBackend::Gemini => Some(hook_name_from_command(command)),
	}
}

fn backend_settings_command(backend: HarnessBackend, hook_command: &str) -> String {
	format!(
		"sh -c 'root=\"${{{}:-$(pwd)}}\"; exec \"$root/{}\"'",
		backend_env_var(backend),
		relative_hook_path(backend, hook_command)
	)
}

fn backend_check_format_arg(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => " --format codex-hook",
		HarnessBackend::Claude | HarnessBackend::Gemini => "",
	}
}

fn backend_tool_backend_arg(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Codex => "codex",
		HarnessBackend::Claude => "claude",
		HarnessBackend::Gemini => "gemini",
	}
}

fn relative_hook_path(backend: HarnessBackend, hook_command: &str) -> String {
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

fn hook_file_name(profile: Option<&str>) -> String {
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
	backend: HarnessBackend,
) -> String {
	let root_expr = format!(
		r#"root="${{{}:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}}""#,
		backend_env_var(backend)
	);
	let files_setup = hook_files_setup(scope, backend);
	let no_files = hook_no_files(backend);
	let command = hook_check_command(profile, rules, max_violations, backend);
	match backend {
		HarnessBackend::Codex => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

{files_setup}
{no_files}

exec {command}
"#
		),
		HarnessBackend::Claude => format!(
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
		HarnessBackend::Gemini => format!(
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

fn hook_files_setup(scope: &Path, backend: HarnessBackend) -> String {
	let set_scope = format!("set -- {}", sh_quote(&scope.display().to_string()));
	format!(
		r#"input_file=$(mktemp "${{TMPDIR:-/tmp}}/code-moniker-hook.XXXXXX")
trap 'rm -f "$input_file"' EXIT HUP INT TERM
cat > "$input_file"
files=$("$HOME/.cargo/bin/code-moniker" harness tool-files {} "$input_file" 2>/dev/null) || {{
	printf '%s\n' 'code-moniker hook could not inspect tool input' >&2
	exit 2
}}

{set_scope}
while IFS= read -r file; do
	[ -n "$file" ] || continue
	set -- "$@" --file "$file"
done <<CODE_MONIKER_FILES
$files
CODE_MONIKER_FILES
"#,
		backend_tool_backend_arg(backend)
	)
}

fn hook_no_files(backend: HarnessBackend) -> &'static str {
	match backend {
		HarnessBackend::Gemini => {
			r#"if [ "$#" -eq 1 ]; then
	printf '%s\n' '{"decision":"allow"}'
	exit 0
fi"#
		}
		HarnessBackend::Codex | HarnessBackend::Claude => {
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
	backend: HarnessBackend,
) -> String {
	let profile_arg = profile
		.map(|profile| format!(" --profile {}", sh_quote(profile)))
		.unwrap_or_default();
	format!(
		r#""$HOME/.cargo/bin/code-moniker" check --rules {}{}{} --max-violations {} "$@""#,
		sh_quote(&rules.display().to_string()),
		profile_arg,
		backend_check_format_arg(backend),
		max_violations,
	)
}

fn write_tool_files<W: Write>(args: &HarnessToolFilesArgs, stdout: &mut W) -> anyhow::Result<()> {
	let raw = fs::read_to_string(&args.input)
		.with_context(|| format!("cannot read `{}`", args.input.display()))?;
	for file in touched_files_from_hook_input(args.backend, &raw) {
		writeln!(stdout, "{file}")?;
	}
	Ok(())
}

fn touched_files_from_hook_input(backend: HarnessToolBackend, raw: &str) -> Vec<String> {
	let mut files = Vec::new();
	if let Ok(value) = serde_json::from_str::<Value>(raw) {
		collect_json_file_paths(&value, &mut files);
		if backend == HarnessToolBackend::Codex {
			collect_codex_apply_patch_paths(&value, &mut files);
		}
	}
	if backend == HarnessToolBackend::Codex {
		collect_apply_patch_paths(raw, &mut files);
	}
	dedup_strings(files)
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

fn read_json_object(path: &Path) -> anyhow::Result<Value> {
	if !path.exists() {
		return Ok(Value::Object(Map::new()));
	}
	let raw =
		fs::read_to_string(path).with_context(|| format!("cannot read `{}`", path.display()))?;
	let value: Value = serde_json::from_str(&raw)
		.with_context(|| format!("`{}` is not valid JSON", path.display()))?;
	if value.is_object() {
		Ok(value)
	} else {
		bail!("`{}` must contain a JSON object", path.display())
	}
}

struct ToolHookUpsert<'a> {
	path: &'a Path,
	command: &'a str,
	event: &'a str,
	matcher: &'a str,
	name: Option<String>,
	project_dir: &'a str,
}

fn upsert_tool_hook(mut settings: Value, request: ToolHookUpsert<'_>) -> anyhow::Result<Value> {
	let root = settings.as_object_mut().expect("settings object");
	let hooks = root
		.entry("hooks")
		.or_insert_with(|| Value::Object(Map::new()))
		.as_object_mut()
		.with_context(|| {
			format!(
				"`{}` field `hooks` must be a JSON object",
				request.path.display()
			)
		})?;
	let event_hooks = hooks
		.entry(request.event)
		.or_insert_with(|| Value::Array(Vec::new()))
		.as_array_mut()
		.with_context(|| {
			format!(
				"`{}` field `hooks.{}` must be a JSON array",
				request.path.display(),
				request.event,
			)
		})?;

	event_hooks
		.retain(|entry| !entry_contains_generated_harness_command(entry, request.project_dir));
	let mut hook = json!({
		"type": "command",
		"command": request.command
	});
	if let Some(hook_name) = request.name
		&& let Some(hook) = hook.as_object_mut()
	{
		hook.insert("name".to_string(), Value::String(hook_name));
	}
	event_hooks.push(json!({
		"matcher": request.matcher,
		"hooks": [hook]
	}));
	Ok(settings)
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

fn entry_contains_generated_harness_command(entry: &Value, project_dir: &str) -> bool {
	let marker = format!("/{project_dir}/hooks/code-moniker-");
	entry
		.get("hooks")
		.and_then(Value::as_array)
		.is_some_and(|hooks| {
			hooks.iter().any(|hook| {
				hook.get("command")
					.and_then(Value::as_str)
					.is_some_and(|command| command.contains(&marker) && command.contains(".sh"))
			})
		})
}

fn performance_report(profile: Option<&str>, scope: &Path, backend: HarnessBackend) -> String {
	let profile_arg = profile
		.map(|profile| format!(" --profile {}", sh_quote(profile)))
		.unwrap_or_default();
	let scope_arg = sh_quote(&scope.display().to_string());
	format!(
		"# code-moniker {} hook overhead\n\n| Date | Machine | Scope | Command | p50 | p95 | Notes |\n| ---- | ------- | ----- | ------- | --- | --- | ----- |\n| YYYY-MM-DD | dev laptop | {} | `code-moniker check{} {} --file <touched-file>` |  |  |  |\n",
		backend_name(backend),
		scope.display(),
		profile_arg,
		scope_arg
	)
}

#[cfg(unix)]
fn make_executable(path: &Path) -> anyhow::Result<()> {
	use std::os::unix::fs::PermissionsExt;
	let mut perms = fs::metadata(path)?.permissions();
	perms.set_mode(perms.mode() | 0o755);
	fs::set_permissions(path, perms)?;
	Ok(())
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> anyhow::Result<()> {
	Ok(())
}

#[cfg(test)]
mod tests {
	use clap::Parser;
	use tempfile::tempdir;

	use crate::args::Cli;
	use crate::{Exit, run};

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
			super::touched_files_from_hook_input(crate::HarnessToolBackend::Claude, claude),
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
			super::touched_files_from_hook_input(crate::HarnessToolBackend::Gemini, gemini),
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
			super::touched_files_from_hook_input(crate::HarnessToolBackend::Codex, codex),
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
						"command": "*** Begin Patch\n*** Update File: crates/cli/src/harness.rs\n*** End Patch\n"
					}
				}
			]
		}"#;

		assert_eq!(
			super::touched_files_from_hook_input(crate::HarnessToolBackend::Codex, codex),
			vec!["crates/cli/src/harness.rs"]
		);
	}

	#[test]
	fn codex_harness_installs_direct_code_moniker_hook() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn codex_harness_uses_code_moniker_codex_hook_format_directly() {
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
			"#!/usr/bin/env sh\nif [ \"$1\" = \"harness\" ]; then printf '%s\\n' 'src/touched.ts'; exit 0; fi\nprintf '%s\\n' \"$*\"\nprintf '%s\\n' '{\"decision\":\"block\",\"reason\":\"violation from fake checker\"}'\nexit 0\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn codex_harness_limits_default_matcher_to_local_write_tools() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
			"--profile",
			"architecture",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn codex_harness_preserves_existing_settings_entries() {
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
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn codex_harness_replaces_previous_generated_harness_entry() {
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
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

		let settings: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		let post = settings["hooks"]["PostToolUse"].as_array().unwrap();
		assert_eq!(post.len(), 2);
		assert_eq!(post[0]["hooks"][0]["command"], "echo read");
		let command = post[1]["hooks"][0]["command"].as_str().unwrap();
		assert!(command.contains(".codex/hooks/code-moniker-check.sh"));
		assert!(!command.contains("code-moniker-architecture.sh"));
	}

	#[test]
	fn codex_harness_quotes_shell_arguments_and_uses_profile_script_name() {
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
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
			"--rules",
			"rules $x.toml",
			"--profile",
			"fast profile",
			"--scope",
			"src $x",
			"--max-violations",
			"3",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

		let script =
			std::fs::read_to_string(dir.path().join(".codex/hooks/code-moniker-fast-profile.sh"))
				.unwrap();
		assert!(script.contains("--rules 'rules $x.toml'"));
		assert!(script.contains("--profile 'fast profile'"));
		assert!(script.contains("--max-violations 3"));
		assert!(script.contains("'src $x'"));
		let performance =
			std::fs::read_to_string(dir.path().join(".codex/code-moniker-performance.md")).unwrap();
		assert!(
			performance.contains(
				"`code-moniker check --profile 'fast profile' 'src $x' --file <touched-file>`"
			),
			"{performance}"
		);
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
	fn codex_harness_requires_requested_profile() {
		let dir = tempdir().unwrap();
		std::fs::write(dir.path().join(".code-moniker.toml"), "").unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"codex",
			dir.path().to_str().unwrap(),
			"--profile",
			"architecture",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::UsageError);
		let stderr = String::from_utf8(stderr).unwrap();
		assert!(stderr.contains("profile `architecture` is not defined"));
	}

	#[test]
	fn claude_harness_installs_project_local_settings_and_hook() {
		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"claude",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn claude_harness_maps_violations_to_stderr_exit_two() {
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
			"#!/usr/bin/env sh\nif [ \"$1\" = \"harness\" ]; then printf '%s\\n' 'src/touched.ts'; exit 0; fi\necho 'violation from fake checker'\nexit 1\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"claude",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn claude_harness_keeps_scope_relative_tool_paths_for_check_resolution() {
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
			"#!/usr/bin/env sh\nif [ \"$1\" = \"harness\" ]; then printf '%s\\n' 'order.ts'; exit 0; fi\nprintf '%s\\n' \"$*\"\nexit 0\n",
		)
		.unwrap();
		super::make_executable(&fake).unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"claude",
			dir.path().to_str().unwrap(),
			"--scope",
			"src",
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn gemini_harness_installs_project_local_settings_and_hook() {
		let tmp = tempdir().unwrap();
		let root = tmp.path().join("space project");
		std::fs::create_dir(&root).unwrap();
		write_architecture_profile(&root);
		let cli = Cli::parse_from(["code-moniker", "harness", "gemini", root.to_str().unwrap()]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
	fn gemini_harness_maps_clean_and_violating_runs_to_hook_contract() {
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
if [ "$1" = "harness" ]; then
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
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"gemini",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
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
	fn claude_harness_preserves_existing_settings_entries() {
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
		let cli = Cli::parse_from([
			"code-moniker",
			"harness",
			"claude",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

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
