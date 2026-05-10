use std::path::PathBuf;

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};

use crate::cli::predicate::Predicate;
use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Parser)]
#[command(
	name = "pg-moniker",
	about = "Single-file moniker / code_graph extraction; see docs/CLI.md",
	version
)]
pub struct Cli {
	#[command(subcommand)]
	pub command: Option<Command>,

	#[command(flatten)]
	pub extract: Args,
}

#[derive(Debug, Subcommand)]
pub enum Command {
	/// Run lint rules against a single file (live linter for agent harnesses).
	Check(CheckArgs),
}

#[derive(Debug, ClapArgs)]
pub struct CheckArgs {
	pub file: PathBuf,

	#[arg(
		long,
		value_name = "PATH",
		default_value = ".pg-moniker.toml",
		help = "user TOML overlay; missing file falls back to embedded defaults"
	)]
	pub rules: PathBuf,

	#[arg(long, value_enum, default_value_t = CheckFormat::Text)]
	pub format: CheckFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum CheckFormat {
	Text,
	Json,
}

#[derive(Debug, ClapArgs)]
pub struct Args {
	pub file: Option<PathBuf>,

	#[arg(long, value_name = "URI", help = "element moniker = <uri>")]
	pub eq: Option<String>,

	#[arg(long, value_name = "URI")]
	pub lt: Option<String>,
	#[arg(long, value_name = "URI")]
	pub le: Option<String>,
	#[arg(long, value_name = "URI")]
	pub gt: Option<String>,
	#[arg(long, value_name = "URI")]
	pub ge: Option<String>,

	#[arg(long = "ancestor-of", value_name = "URI")]
	pub ancestor_of: Option<String>,
	#[arg(long = "descendant-of", value_name = "URI")]
	pub descendant_of: Option<String>,
	#[arg(long, value_name = "URI", help = "asymmetric bind_match (?= operator)")]
	pub bind: Option<String>,

	#[arg(long, value_name = "NAME", help = "kind filter (repeatable, OR)")]
	pub kind: Vec<String>,

	#[arg(long, value_enum, default_value_t = OutputFormat::Tsv)]
	pub format: OutputFormat,

	#[arg(long, conflicts_with = "quiet", help = "print only the match count")]
	pub count: bool,
	#[arg(
		long,
		conflicts_with = "count",
		help = "suppress output, exit code only"
	)]
	pub quiet: bool,

	#[arg(long = "with-text", help = "include comment text (re-reads source)")]
	pub with_text: bool,

	#[arg(
		long,
		value_name = "SCHEME",
		help = "URI scheme; defaults to <lang>+moniker:// based on the file extension"
	)]
	pub scheme: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
	Tsv,
	Json,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OutputMode {
	Default,
	Count,
	Quiet,
}

impl Args {
	pub fn mode(&self) -> OutputMode {
		if self.count {
			OutputMode::Count
		} else if self.quiet {
			OutputMode::Quiet
		} else {
			OutputMode::Default
		}
	}

	pub fn compiled_predicates(&self, default_scheme: &str) -> anyhow::Result<Vec<Predicate>> {
		let scheme = self.scheme.as_deref().unwrap_or(default_scheme);
		let cfg = UriConfig { scheme };
		let parse = |flag: &str, raw: &str| -> anyhow::Result<Moniker> {
			from_uri(raw, &cfg).map_err(|e| anyhow::anyhow!("--{flag} `{raw}`: {e}"))
		};
		let mut out = Vec::new();
		if let Some(s) = &self.eq {
			out.push(Predicate::Eq(parse("eq", s)?));
		}
		if let Some(s) = &self.lt {
			out.push(Predicate::Lt(parse("lt", s)?));
		}
		if let Some(s) = &self.le {
			out.push(Predicate::Le(parse("le", s)?));
		}
		if let Some(s) = &self.gt {
			out.push(Predicate::Gt(parse("gt", s)?));
		}
		if let Some(s) = &self.ge {
			out.push(Predicate::Ge(parse("ge", s)?));
		}
		if let Some(s) = &self.ancestor_of {
			out.push(Predicate::AncestorOf(parse("ancestor-of", s)?));
		}
		if let Some(s) = &self.descendant_of {
			out.push(Predicate::DescendantOf(parse("descendant-of", s)?));
		}
		if let Some(s) = &self.bind {
			out.push(Predicate::Bind(parse("bind", s)?));
		}
		Ok(out)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
		let mut full = vec!["pg-moniker"];
		full.extend_from_slice(argv);
		Cli::try_parse_from(full)
	}

	fn extract(argv: &[&str]) -> Args {
		let cli = parse(argv).unwrap();
		assert!(cli.command.is_none());
		cli.extract
	}

	#[test]
	fn no_args_parses_but_carries_no_file() {
		let cli = parse(&[]).expect("clap accepts empty argv");
		assert!(cli.command.is_none());
		assert!(cli.extract.file.is_none());
	}

	#[test]
	fn minimal_invocation() {
		let a = extract(&["a.ts"]);
		assert_eq!(a.file.as_deref(), Some(std::path::Path::new("a.ts")));
		assert_eq!(a.format, OutputFormat::Tsv);
		assert_eq!(a.mode(), OutputMode::Default);
		assert!(a.kind.is_empty());
		assert!(!a.with_text);
	}

	#[test]
	fn quiet_and_count_are_mutually_exclusive() {
		assert!(parse(&["a.ts", "--count", "--quiet"]).is_err());
	}

	#[test]
	fn count_mode_detected() {
		assert_eq!(extract(&["a.ts", "--count"]).mode(), OutputMode::Count);
	}

	#[test]
	fn quiet_mode_detected() {
		assert_eq!(extract(&["a.ts", "--quiet"]).mode(), OutputMode::Quiet);
	}

	#[test]
	fn format_json_recognised() {
		assert_eq!(
			extract(&["a.ts", "--format", "json"]).format,
			OutputFormat::Json
		);
	}

	#[test]
	fn unknown_format_rejected() {
		assert!(parse(&["a.ts", "--format", "xml"]).is_err());
	}

	#[test]
	fn kind_is_repeatable() {
		let a = extract(&["a.ts", "--kind", "class", "--kind", "method"]);
		assert_eq!(a.kind, vec!["class".to_string(), "method".to_string()]);
	}

	#[test]
	fn with_text_flag() {
		assert!(extract(&["a.ts", "--with-text"]).with_text);
	}

	#[test]
	fn predicate_uri_parses() {
		let a = extract(&["a.ts", "--descendant-of", "ts+moniker://./class:Foo"]);
		let preds = a.compiled_predicates("ts+moniker://").expect("uri ok");
		assert_eq!(preds.len(), 1);
		match &preds[0] {
			Predicate::DescendantOf(_) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn predicate_uri_malformed_is_usage_error() {
		let a = extract(&["a.ts", "--eq", "not a uri"]);
		let err = a.compiled_predicates("ts+moniker://").unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("--eq"), "expected flag in error: {msg}");
	}

	#[test]
	fn check_subcommand_routes_to_command() {
		let cli = parse(&["check", "a.ts"]).unwrap();
		match cli.command {
			Some(Command::Check(c)) => assert_eq!(c.file, PathBuf::from("a.ts")),
			other => panic!("expected Check, got {other:?}"),
		}
	}

	#[test]
	fn check_subcommand_accepts_rules_and_format() {
		let cli = parse(&[
			"check",
			"a.ts",
			"--rules",
			"my-rules.toml",
			"--format",
			"json",
		])
		.unwrap();
		match cli.command {
			Some(Command::Check(c)) => {
				assert_eq!(c.rules, PathBuf::from("my-rules.toml"));
				assert_eq!(c.format, CheckFormat::Json);
			}
			other => panic!("expected Check, got {other:?}"),
		}
	}
}
