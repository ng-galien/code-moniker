use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};

use crate::Exit;
use crate::args::{RulesArgs, RulesCommand, RulesFileArgs};

pub fn run<W1: Write, W2: Write>(args: &RulesArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let result = match &args.command {
		RulesCommand::Init(args) => init(args, stdout),
		RulesCommand::Disable(args) => set_default_rules(args, false, stdout),
		RulesCommand::Enable(args) => set_default_rules(args, true, stdout),
	};
	match result {
		Ok(()) => Exit::Match,
		Err(e) => {
			let _ = writeln!(stderr, "code-moniker: {e:#}");
			Exit::UsageError
		}
	}
}

fn init<W: Write>(args: &RulesFileArgs, stdout: &mut W) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let path = resolve_from_root(&root, &args.rules);
	if path.exists() {
		bail!("`{}` already exists", path.display());
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)
			.with_context(|| format!("cannot create `{}`", parent.display()))?;
	}
	let detected = detect_project(&root);
	let content = initial_config(&detected);
	fs::write(&path, content).with_context(|| format!("cannot write `{}`", path.display()))?;
	writeln!(
		stdout,
		"Created {} for {} project rules.",
		path.display(),
		detected.label()
	)?;
	Ok(())
}

fn set_default_rules<W: Write>(
	args: &RulesFileArgs,
	enabled: bool,
	stdout: &mut W,
) -> anyhow::Result<()> {
	let root = args
		.root
		.canonicalize()
		.with_context(|| format!("cannot resolve project root `{}`", args.root.display()))?;
	let path = resolve_from_root(&root, &args.rules);
	let raw = if path.exists() {
		fs::read_to_string(&path).with_context(|| format!("cannot read `{}`", path.display()))?
	} else {
		String::new()
	};
	if !raw.trim().is_empty() {
		parse_toml(&raw, &path)?;
	}
	if let Some(parent) = path.parent() {
		fs::create_dir_all(parent)
			.with_context(|| format!("cannot create `{}`", parent.display()))?;
	}
	let next = set_top_level_default_rules(&raw, enabled)?;
	parse_toml(&next, &path)?;
	fs::write(&path, next).with_context(|| format!("cannot write `{}`", path.display()))?;
	let state = if enabled { "enabled" } else { "disabled" };
	writeln!(
		stdout,
		"Embedded default rules {state} in {}.",
		path.display()
	)?;
	Ok(())
}

fn resolve_from_root(root: &Path, path: &Path) -> PathBuf {
	if path.is_absolute() {
		path.to_path_buf()
	} else {
		root.join(path)
	}
}

fn parse_toml(raw: &str, path: &Path) -> anyhow::Result<toml::Value> {
	raw.parse::<toml::Value>()
		.with_context(|| format!("`{}` is not valid TOML", path.display()))
}

fn set_top_level_default_rules(raw: &str, enabled: bool) -> anyhow::Result<String> {
	let flag = format!("default_rules = {enabled}");
	if raw.trim().is_empty() {
		return Ok(format!("{flag}\n"));
	}

	let mut lines: Vec<String> = raw.lines().map(str::to_string).collect();
	let first_table = lines
		.iter()
		.position(|line| line.trim_start().starts_with('['))
		.unwrap_or(lines.len());

	for line in &mut lines[..first_table] {
		let trimmed = line.trim_start();
		if let Some(rest) = trimmed.strip_prefix("default_rules")
			&& rest.trim_start().starts_with('=')
		{
			let indent = &line[..line.len() - trimmed.len()];
			*line = format!("{indent}{flag}");
			return Ok(finish_lines(lines, raw.ends_with('\n')));
		}
	}

	lines.insert(first_table, flag);
	Ok(finish_lines(lines, true))
}

fn finish_lines(lines: Vec<String>, trailing_newline: bool) -> String {
	let mut out = lines.join("\n");
	if trailing_newline {
		out.push('\n');
	}
	out
}

#[derive(Default)]
struct DetectedProject {
	java: bool,
	ts: bool,
	rust: bool,
	python: bool,
	go: bool,
	cs: bool,
}

impl DetectedProject {
	fn label(&self) -> &'static str {
		let count = [self.java, self.ts, self.rust, self.python, self.go, self.cs]
			.into_iter()
			.filter(|detected| *detected)
			.count();
		match count {
			0 => "generic",
			1 if self.java => "java",
			1 if self.ts => "typescript",
			1 if self.rust => "rust",
			1 if self.python => "python",
			1 if self.go => "go",
			1 if self.cs => "csharp",
			_ => "multi-language",
		}
	}
}

fn detect_project(root: &Path) -> DetectedProject {
	let mut detected = DetectedProject {
		java: root.join("pom.xml").exists()
			|| root.join("build.gradle").exists()
			|| root.join("build.gradle.kts").exists(),
		ts: root.join("package.json").exists() || root.join("tsconfig.json").exists(),
		rust: root.join("Cargo.toml").exists(),
		python: root.join("pyproject.toml").exists(),
		go: root.join("go.mod").exists(),
		cs: false,
	};
	detected.cs = fs::read_dir(root).is_ok_and(|entries| {
		entries.filter_map(Result::ok).any(|entry| {
			entry
				.path()
				.extension()
				.and_then(|ext| ext.to_str())
				.is_some_and(|ext| ext.eq_ignore_ascii_case("csproj"))
		})
	});
	detected
}

fn initial_config(detected: &DetectedProject) -> String {
	let mut out = String::from(
		"# code-moniker project rules\n\
		 # This file is loaded automatically by `code-moniker check`.\n\n\
		 default_rules = true\n\n\
		 [aliases]\n",
	);
	let mut wrote = false;
	if detected.java {
		wrote = true;
		out.push_str(
			"java_main = \"moniker ~ '**/srcset:main/**'\"\n\
			 java_test = \"moniker ~ '**/srcset:test/**'\"\n",
		);
	}
	if detected.ts {
		wrote = true;
		out.push_str(
			"ts_src = \"moniker ~ '**/dir:src/**'\"\n\
			 ts_test = \"moniker ~ '**/dir:test/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.rust {
		wrote = true;
		out.push_str(
			"rust_src = \"moniker ~ '**/dir:src/**'\"\n\
			 rust_tests = \"moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.python {
		wrote = true;
		out.push_str(
			"python_package = \"moniker ~ '**/dir:src/**'\"\n\
			 python_tests = \"moniker ~ '**/dir:test/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if detected.go {
		wrote = true;
		out.push_str("go_package = \"moniker ~ '**/lang:go/**'\"\n");
	}
	if detected.cs {
		wrote = true;
		out.push_str(
			"cs_src = \"moniker ~ '**/lang:cs/**'\"\n\
			 cs_tests = \"moniker ~ '**/dir:Tests/**' OR moniker ~ '**/dir:tests/**'\"\n",
		);
	}
	if !wrote {
		out.push_str("src = \"moniker ~ '**/dir:src/**'\"\n");
	}
	out.push('\n');
	out.push_str(
		"# Add project-specific rules here. Example:\n\
		 # [[refs.where]]\n\
		 # id = \"domain-no-infra\"\n\
		 # expr = \"source ~ '**/dir:domain/**' => NOT target ~ '**/dir:infrastructure/**'\"\n",
	);
	out
}

#[cfg(test)]
mod tests {
	use clap::Parser;
	use tempfile::tempdir;

	use crate::args::Cli;
	use crate::{Exit, run};

	#[test]
	fn rules_init_creates_canonical_file_with_detected_aliases() {
		let dir = tempdir().unwrap();
		std::fs::write(dir.path().join("pom.xml"), "<project/>").unwrap();
		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"init",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();

		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);

		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = true"));
		assert!(config.contains("java_main = \"moniker ~ '**/srcset:main/**'\""));
		assert!(config.contains("java_test = \"moniker ~ '**/srcset:test/**'\""));
		assert!(!config.contains("code-moniker.toml"));
	}

	#[test]
	fn rules_disable_and_enable_toggle_default_rules() {
		let dir = tempdir().unwrap();
		std::fs::write(
			dir.path().join(".code-moniker.toml"),
			"# local rules\n\n[aliases]\nfoo = \"name = Foo\"\n",
		)
		.unwrap();

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"disable",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = false\n"));
		assert!(config.contains("[aliases]"));

		let cli = Cli::parse_from([
			"code-moniker",
			"rules",
			"enable",
			dir.path().to_str().unwrap(),
		]);
		let mut stdout = Vec::new();
		let mut stderr = Vec::new();
		assert_eq!(run(&cli, &mut stdout, &mut stderr), Exit::Match);
		let config = std::fs::read_to_string(dir.path().join(".code-moniker.toml")).unwrap();
		assert!(config.contains("default_rules = true\n"));
	}
}
