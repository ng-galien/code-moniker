use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde_json::{Map, Value, json};

use crate::Exit;
use crate::args::{CodexHarnessArgs, HarnessArgs, HarnessCommand};

const CODEX_MATCHER: &str = "apply_patch|Write|Edit|MultiEdit";

pub fn run<W1: Write, W2: Write>(args: &HarnessArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let result = match &args.command {
		HarnessCommand::Codex(args) => install_codex(args, stdout),
	};
	match result {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn install_codex<W: Write>(args: &CodexHarnessArgs, stdout: &mut W) -> anyhow::Result<()> {
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

	let codex_dir = root.join(".codex");
	let hooks_dir = codex_dir.join("hooks");
	fs::create_dir_all(&hooks_dir)
		.with_context(|| format!("cannot create `{}`", hooks_dir.display()))?;

	let hook_file = hook_file_name(&args.profile);
	let hook_path = hooks_dir.join(&hook_file);
	fs::write(&hook_path, hook_script(&args.profile, &args.rules, &scope))
		.with_context(|| format!("cannot write `{}`", hook_path.display()))?;
	make_executable(&hook_path)?;

	let hooks_path = codex_dir.join("hooks.json");
	let hooks = read_json_object(&hooks_path)?;
	let hook_command = format!("$CODEX_PROJECT_DIR/.codex/hooks/{hook_file}");
	let hooks = upsert_codex_hook(hooks, &hooks_path, &hook_command)?;
	fs::write(&hooks_path, serde_json::to_vec_pretty(&hooks)?)
		.with_context(|| format!("cannot write `{}`", hooks_path.display()))?;
	fs::write(
		codex_dir.join("code-moniker-performance.md"),
		performance_report(&args.profile, &scope),
	)
	.with_context(|| "cannot write Codex hook performance template")?;

	writeln!(
		stdout,
		"Installed Codex live harness for profile `{}` on `{}`.",
		args.profile,
		scope.display()
	)?;
	writeln!(stdout, "Hook: {}", hook_path.display())?;
	writeln!(stdout, "Codex hooks: {}", hooks_path.display())?;
	Ok(())
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

fn hook_script(profile: &str, rules: &Path, scope: &Path) -> String {
	format!(
		r#"#!/usr/bin/env sh
set -eu

root="${{CODEX_PROJECT_DIR:-$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)}}"
cd "$root"

exec code-moniker check --rules {} --profile {} {}
"#,
		sh_quote(&rules.display().to_string()),
		sh_quote(profile),
		sh_quote(&scope.display().to_string())
	)
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

fn upsert_codex_hook(mut settings: Value, path: &Path, command: &str) -> anyhow::Result<Value> {
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
		"matcher": CODEX_MATCHER,
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

fn performance_report(profile: &str, scope: &Path) -> String {
	format!(
		"# code-moniker Codex hook overhead\n\n| Date | Machine | Scope | Command | p50 | p95 | Notes |\n| ---- | ------- | ----- | ------- | --- | --- | ----- |\n| YYYY-MM-DD | dev laptop | {} | `code-moniker check --profile {} {}` |  |  |  |\n",
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
		assert!(script.contains("exec code-moniker check"));
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
			"$CODEX_PROJECT_DIR/.codex/hooks/code-moniker-fast-profile.sh"
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
}
