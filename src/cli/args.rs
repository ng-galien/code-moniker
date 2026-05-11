use std::path::PathBuf;

use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};

use crate::cli::predicate::Predicate;
use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Parser)]
#[command(
	name = "code-moniker",
	about = "Single-file moniker / code_graph extraction; see docs/cli-extract.md",
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
		default_value = ".code-moniker.toml",
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

	#[arg(
		long = "where",
		value_name = "OP URI",
		help = "predicate `<op> <uri>` where op ∈ {=, <, <=, >, >=, @>, <@, ?=}; repeatable, AND-combined"
	)]
	pub where_: Vec<String>,

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
		help = "URI scheme; defaults to code+moniker://"
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
		let mut out = Vec::with_capacity(self.where_.len());
		for raw in &self.where_ {
			out.push(parse_where(raw, &cfg)?);
		}
		Ok(out)
	}
}

/// CLI predicate ops are the moniker subset of `expr::TWO_CHAR_OPS` — regex
/// ops (`=~`, `!~`) and inequality (`!=`) don't map to a `Predicate` variant.
const CLI_TWO_CHAR_OPS: &[&str] = &["<=", ">=", "<@", "@>", "?="];

fn parse_where(raw: &str, cfg: &UriConfig<'_>) -> anyhow::Result<Predicate> {
	let raw = raw.trim();
	let bail = || {
		anyhow::anyhow!("--where `{raw}`: expected `<op> <uri>` (op ∈ =, <=, >=, <, >, @>, <@, ?=)")
	};
	for op in CLI_TWO_CHAR_OPS {
		if let Some(rest) = raw.strip_prefix(op) {
			return finish_where(op, rest.trim(), cfg, raw);
		}
	}
	for &op in &["<", ">", "="] {
		if let Some(rest) = raw.strip_prefix(op) {
			return finish_where(op, rest.trim(), cfg, raw);
		}
	}
	Err(bail())
}

fn finish_where(op: &str, uri: &str, cfg: &UriConfig<'_>, raw: &str) -> anyhow::Result<Predicate> {
	if uri.is_empty() {
		return Err(anyhow::anyhow!("--where `{raw}`: missing URI after `{op}`"));
	}
	let m: Moniker = from_uri(uri, cfg).map_err(|e| anyhow::anyhow!("--where `{raw}`: {e}"))?;
	Ok(match op {
		"=" => Predicate::Eq(m),
		"<" => Predicate::Lt(m),
		"<=" => Predicate::Le(m),
		">" => Predicate::Gt(m),
		">=" => Predicate::Ge(m),
		"@>" => Predicate::AncestorOf(m),
		"<@" => Predicate::DescendantOf(m),
		"?=" => Predicate::Bind(m),
		_ => unreachable!("op set is whitelisted via CLI_TWO_CHAR_OPS / single-char fallthrough"),
	})
}

#[cfg(test)]
mod tests {
	use super::*;

	fn parse(argv: &[&str]) -> Result<Cli, clap::Error> {
		let mut full = vec!["code-moniker"];
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
	fn where_descendant_parses() {
		let a = extract(&["a.ts", "--where", "<@ code+moniker://./class:Foo"]);
		let preds = a.compiled_predicates("code+moniker://").expect("ok");
		assert_eq!(preds.len(), 1);
		assert!(matches!(preds[0], Predicate::DescendantOf(_)));
	}

	#[test]
	fn where_multiple_predicates_compose_with_and() {
		let a = extract(&[
			"a.ts",
			"--where",
			"@> code+moniker://./class:Foo",
			"--where",
			"= code+moniker://./class:Foo/method:bar",
		]);
		let preds = a.compiled_predicates("code+moniker://").expect("ok");
		assert_eq!(preds.len(), 2);
		assert!(matches!(preds[0], Predicate::AncestorOf(_)));
		assert!(matches!(preds[1], Predicate::Eq(_)));
	}

	#[test]
	fn where_each_operator_supported() {
		for op in &["=", "<", "<=", ">", ">=", "@>", "<@", "?="] {
			let a = extract(&[
				"a.ts",
				"--where",
				&format!("{op} code+moniker://./class:Foo"),
			]);
			let preds = a.compiled_predicates("code+moniker://").expect(op);
			assert_eq!(preds.len(), 1, "op {op} failed");
		}
	}

	#[test]
	fn where_malformed_is_usage_error() {
		let a = extract(&["a.ts", "--where", "garbage uri"]);
		let err = a.compiled_predicates("code+moniker://").unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("--where"), "{msg}");
	}

	#[test]
	fn where_missing_uri_is_usage_error() {
		let a = extract(&["a.ts", "--where", "@>"]);
		let err = a.compiled_predicates("code+moniker://").unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("missing URI"), "{msg}");
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
