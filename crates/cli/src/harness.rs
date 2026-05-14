use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

use crate::Exit;
use crate::args::{CodexHarnessArgs, HarnessArgs, HarnessCommand};

const CODEX_MATCHER: &str = "apply_patch|Write|Edit|MultiEdit";
const CLAUDE_MATCHER: &str = "Edit|Write|MultiEdit";

#[derive(Copy, Clone)]
enum HarnessBackend {
	Codex,
	Claude,
}

pub fn run<W1: Write, W2: Write>(args: &HarnessArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let result = match &args.command {
		HarnessCommand::Codex(args) => install(args, HarnessBackend::Codex, stdout),
		HarnessCommand::Claude(args) => install(args, HarnessBackend::Claude, stdout),
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
	let cfg = crate::check::config::load_with_overrides(Some(&rules))?;
	if !cfg.profiles.contains_key(&args.profile) {
		bail!(
			"profile `{}` is not defined in `{}`; add [profiles.{}] before installing the live harness",
			args.profile,
			rules.display(),
			args.profile
		);
	}

	let project_dir = root.join(backend.project_dir());
	let hooks_dir = project_dir.join("hooks");
	fs::create_dir_all(&hooks_dir)
		.with_context(|| format!("cannot create `{}`", hooks_dir.display()))?;

	let hook_file = hook_file_name(&args.profile);
	let hook_path = hooks_dir.join(&hook_file);
	fs::write(
		&hook_path,
		hook_script(&args.profile, &args.rules, &scope, backend),
	)
	.with_context(|| format!("cannot write `{}`", hook_path.display()))?;
	make_executable(&hook_path)?;

	let config_path = project_dir.join(backend.config_file());
	let config = read_json_object(&config_path)?;
	let hook_command = hook_path.display().to_string();
	let config = upsert_post_tool_use_hook(config, &config_path, &hook_command, backend.matcher())?;
	fs::write(&config_path, serde_json::to_vec_pretty(&config)?)
		.with_context(|| format!("cannot write `{}`", config_path.display()))?;
	fs::write(
		project_dir.join("code-moniker-performance.md"),
		performance_report(&args.profile, &scope, backend),
	)
	.with_context(|| format!("cannot write {} hook performance template", backend.name()))?;

	writeln!(
		stdout,
		"Installed {} live harness for profile `{}` on `{}`.",
		backend.name(),
		args.profile,
		scope.display()
	)?;
	writeln!(stdout, "Hook: {}", hook_path.display())?;
	writeln!(
		stdout,
		"{} config: {}",
		backend.name(),
		config_path.display()
	)?;
	Ok(())
}

impl HarnessBackend {
	fn name(self) -> &'static str {
		match self {
			Self::Codex => "Codex",
			Self::Claude => "Claude",
		}
	}

	fn project_dir(self) -> &'static str {
		match self {
			Self::Codex => ".codex",
			Self::Claude => ".claude",
		}
	}

	fn config_file(self) -> &'static str {
		match self {
			Self::Codex => "hooks.json",
			Self::Claude => "settings.json",
		}
	}

	fn env_var(self) -> &'static str {
		match self {
			Self::Codex => "CODEX_PROJECT_DIR",
			Self::Claude => "CLAUDE_PROJECT_DIR",
		}
	}

	fn matcher(self) -> &'static str {
		match self {
			Self::Codex => CODEX_MATCHER,
			Self::Claude => CLAUDE_MATCHER,
		}
	}
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

fn hook_file_name(profile: &str) -> String {
	let slug: String = profile
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

fn hook_script(profile: &str, rules: &Path, scope: &Path, backend: HarnessBackend) -> String {
	let root_expr = format!(
		r#"root="${{{}:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}}""#,
		backend.env_var()
	);
	let command = format!(
		r#""$HOME/.cargo/bin/code-moniker" check --rules {} --profile {} {}"#,
		sh_quote(&rules.display().to_string()),
		sh_quote(profile),
		sh_quote(&scope.display().to_string())
	);
	match backend {
		HarnessBackend::Codex => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

exec {command}
"#
		),
		HarnessBackend::Claude => format!(
			r#"#!/usr/bin/env sh
set -eu

{root_expr}
cd "$root"

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
	}
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

fn upsert_post_tool_use_hook(
	mut settings: Value,
	path: &Path,
	command: &str,
	matcher: &str,
) -> anyhow::Result<Value> {
	let root = settings.as_object_mut().expect("settings object");
	let hooks = root
		.entry("hooks")
		.or_insert_with(|| Value::Object(Map::new()))
		.as_object_mut()
		.with_context(|| format!("`{}` field `hooks` must be a JSON object", path.display()))?;
	let post = hooks
		.entry("PostToolUse")
		.or_insert_with(|| Value::Array(Vec::new()))
		.as_array_mut()
		.with_context(|| {
			format!(
				"`{}` field `hooks.PostToolUse` must be a JSON array",
				path.display()
			)
		})?;

	post.retain(|entry| !entry_contains_command(entry, command));
	post.push(json!({
		"matcher": matcher,
		"hooks": [
			{
				"type": "command",
				"command": command
			}
		]
	}));
	Ok(settings)
}

fn entry_contains_command(entry: &Value, command: &str) -> bool {
	entry
		.get("hooks")
		.and_then(Value::as_array)
		.is_some_and(|hooks| {
			hooks
				.iter()
				.any(|hook| hook.get("command").and_then(Value::as_str) == Some(command))
		})
}

fn performance_report(profile: &str, scope: &Path, backend: HarnessBackend) -> String {
	format!(
		"# code-moniker {} hook overhead\n\n| Date | Machine | Scope | Command | p50 | p95 | Notes |\n| ---- | ------- | ----- | ------- | --- | --- | ----- |\n| YYYY-MM-DD | dev laptop | {} | `code-moniker check --profile {} {}` |  |  |  |\n",
		backend.name(),
		scope.display(),
		profile,
		scope.display()
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
			std::fs::read_to_string(dir.path().join(".codex/hooks/code-moniker-architecture.sh"))
				.unwrap();
		assert!(script.contains("exec \"$HOME/.cargo/bin/code-moniker\" check"));
		assert!(script.contains("$HOME/.cargo/bin/code-moniker"));
		assert!(script.contains("--profile 'architecture'"));
		assert!(script.contains("'src'"));
		assert!(!script.contains("npm"));
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
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

		let script =
			std::fs::read_to_string(dir.path().join(".codex/hooks/code-moniker-fast-profile.sh"))
				.unwrap();
		assert!(script.contains("--rules 'rules $x.toml'"));
		assert!(script.contains("--profile 'fast profile'"));
		assert!(script.contains("'src $x'"));
		let hooks: serde_json::Value = serde_json::from_str(
			&std::fs::read_to_string(dir.path().join(".codex/hooks.json")).unwrap(),
		)
		.unwrap();
		assert_eq!(
			hooks["hooks"]["PostToolUse"][0]["hooks"][0]["command"],
			dir.path()
				.join(".codex/hooks/code-moniker-fast-profile.sh")
				.canonicalize()
				.unwrap()
				.display()
				.to_string()
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

		let script = std::fs::read_to_string(
			dir.path()
				.join(".claude/hooks/code-moniker-architecture.sh"),
		)
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
			dir.path()
				.join(".claude/hooks/code-moniker-architecture.sh")
				.canonicalize()
				.unwrap()
				.display()
				.to_string()
		);
	}

	#[test]
	fn claude_harness_maps_violations_to_stderr_exit_two() {
		use std::process::Command;

		let dir = tempdir().unwrap();
		write_architecture_profile(dir.path());
		let bin_dir = dir.path().join(".cargo/bin");
		std::fs::create_dir_all(&bin_dir).unwrap();
		let fake = bin_dir.join("code-moniker");
		std::fs::write(
			&fake,
			"#!/usr/bin/env sh\necho 'violation from fake checker'\nexit 1\n",
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

		let output = Command::new(
			dir.path()
				.join(".claude/hooks/code-moniker-architecture.sh"),
		)
		.env("CLAUDE_PROJECT_DIR", dir.path())
		.env("HOME", dir.path())
		.output()
		.unwrap();

		assert_eq!(output.status.code(), Some(2));
		assert!(String::from_utf8(output.stdout).unwrap().is_empty());
		assert_eq!(
			String::from_utf8(output.stderr).unwrap().trim(),
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
