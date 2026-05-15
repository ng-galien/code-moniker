use std::path::PathBuf;

use clap::builder::{PossibleValuesParser, TypedValueParser};
use clap::{Args as ClapArgs, Parser, Subcommand, ValueEnum};

use crate::predicate::Predicate;
use code_moniker_core::core::moniker::Moniker;
use code_moniker_core::core::shape::Shape;
use code_moniker_core::core::uri::{UriConfig, from_uri};

const ASCII_LOGO: &str = "
    ◆ code+moniker://
    └─◆ lang:ts
      └─◆ class:Util
";

#[derive(Debug, Parser)]
#[command(name = "code-moniker", before_help = ASCII_LOGO, version)]
pub struct Cli {
	#[command(subcommand)]
	pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
	#[command(about = "Extract a moniker graph from a file or directory.")]
	Extract(ExtractArgs),
	#[command(about = "Report extraction metrics for a file or directory.")]
	Stats(StatsArgs),
	#[command(about = "Lint a path against .code-moniker.toml rules.")]
	Check(CheckArgs),
	#[command(about = "Install live agent harness configuration.")]
	Harness(HarnessArgs),
	#[command(about = "List supported languages, or kinds of one.")]
	Langs(LangsArgs),
	#[command(about = "Show the shape vocabulary.")]
	Shapes(ShapesArgs),
	#[command(
		about = "Extract declared dependencies from a build manifest (auto-detected by filename) or every manifest under a directory."
	)]
	Manifest(ManifestArgs),
}

#[derive(Debug, ClapArgs)]
pub struct HarnessArgs {
	#[command(subcommand)]
	pub command: HarnessCommand,
}

#[derive(Debug, Subcommand)]
pub enum HarnessCommand {
	#[command(about = "Install a project-local Codex PostToolUse hook.")]
	Codex(CodexHarnessArgs),
	#[command(about = "Install a project-local Claude Code PostToolUse hook.")]
	Claude(CodexHarnessArgs),
}

#[derive(Debug, ClapArgs)]
pub struct CodexHarnessArgs {
	#[arg(value_name = "ROOT", default_value = ".")]
	pub root: PathBuf,

	#[arg(
		long,
		value_name = "PATH",
		default_value = ".code-moniker.toml",
		help = "project rules file, resolved from ROOT unless absolute"
	)]
	pub rules: PathBuf,

	#[arg(
		long,
		value_name = "NAME",
		default_value = "architecture",
		help = "profile that the live harness must run"
	)]
	pub profile: String,

	#[arg(
		long,
		value_name = "PATH",
		default_value = "src",
		help = "project scope checked by the hook, resolved from ROOT"
	)]
	pub scope: PathBuf,
}

#[derive(Debug, ClapArgs)]
pub struct ShapesArgs {
	#[arg(long, value_enum, default_value_t = LangsFormat::Text)]
	pub format: LangsFormat,
}

#[derive(Debug, ClapArgs)]
pub struct LangsArgs {
	#[arg(
		value_name = "LANG",
		help = "language tag (e.g. rs, ts, java, python, go, cs, sql); omit to list every tag"
	)]
	pub lang: Option<String>,

	#[arg(long, value_enum, default_value_t = LangsFormat::Text)]
	pub format: LangsFormat,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum LangsFormat {
	Text,
	Json,
}

#[derive(Debug, ClapArgs)]
pub struct CheckArgs {
	#[arg(value_name = "PATH")]
	pub path: PathBuf,

	#[arg(
		long,
		value_name = "PATH",
		default_value = ".code-moniker.toml",
		help = "user TOML overlay; missing file falls back to embedded defaults"
	)]
	pub rules: PathBuf,

	#[arg(long, value_enum, default_value_t = CheckFormat::Text)]
	pub format: CheckFormat,

	#[arg(
		long,
		help = "print per-rule observability, including implication antecedent hit counts"
	)]
	pub report: bool,

	#[arg(
		long,
		value_name = "NAME",
		help = "filter rules through a named profile from .code-moniker.toml"
	)]
	pub profile: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum CheckFormat {
	Text,
	Json,
}

#[derive(Debug, ClapArgs)]
pub struct StatsArgs {
	#[arg(value_name = "PATH")]
	pub path: PathBuf,

	#[arg(long, value_enum, default_value_t = StatsFormat::Tsv)]
	pub format: StatsFormat,

	#[arg(
		long,
		value_enum,
		default_value_t = ColorChoice::Auto,
		help = "ANSI color for --format tree: auto = on if stdout is a TTY (honors NO_COLOR / CLICOLOR / CLICOLOR_FORCE)"
	)]
	pub color: ColorChoice,

	#[arg(
		long,
		value_enum,
		default_value_t = Charset::Utf8,
		help = "glyph set for --format tree"
	)]
	pub charset: Charset,

	#[arg(
		long,
		value_name = "NAME",
		help = "project component of the anchor moniker; defaults to '.'"
	)]
	pub project: Option<String>,

	#[arg(
		long,
		value_name = "DIR",
		env = "CODE_MONIKER_CACHE_DIR",
		help = "enable on-disk cache of extracted graphs at DIR (empty = disabled)"
	)]
	pub cache: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum StatsFormat {
	Tsv,
	Json,
	#[cfg(feature = "pretty")]
	Tree,
}

#[derive(Debug, ClapArgs)]
pub struct ExtractArgs {
	#[arg(value_name = "PATH")]
	pub path: PathBuf,

	#[arg(
		long = "where",
		value_name = "OP URI",
		help = "predicate `<op> <uri>` where op ∈ {=, <, <=, >, >=, @>, <@, ?=}; repeatable, AND-combined"
	)]
	pub where_: Vec<String>,

	#[arg(
		long,
		value_name = "NAME",
		value_delimiter = ',',
		help = "concrete kind (e.g. class, fn, calls); repeatable or comma-separated; OR within --kind, AND with --shape. Discover values per language with `code-moniker langs <TAG>`."
	)]
	pub kind: Vec<String>,

	#[arg(
		long,
		value_name = "SHAPE",
		value_delimiter = ',',
		value_parser = shape_parser(),
		help = "kind family; repeatable or comma-separated; OR within --shape, AND with --kind. See `code-moniker shapes`."
	)]
	pub shape: Vec<Shape>,

	#[arg(long, value_enum, default_value_t = OutputFormat::Tsv)]
	pub format: OutputFormat,

	#[arg(
		long,
		value_enum,
		default_value_t = ColorChoice::Auto,
		help = "ANSI color for --format tree: auto = on if stdout is a TTY (honors NO_COLOR / CLICOLOR / CLICOLOR_FORCE)"
	)]
	pub color: ColorChoice,

	#[arg(
		long,
		value_enum,
		default_value_t = Charset::Utf8,
		help = "glyph set for --format tree"
	)]
	pub charset: Charset,

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

	#[arg(
		long,
		value_name = "NAME",
		help = "project component of the anchor moniker; defaults to '.'"
	)]
	pub project: Option<String>,

	#[arg(
		long,
		value_name = "DIR",
		env = "CODE_MONIKER_CACHE_DIR",
		help = "enable on-disk cache of extracted graphs at DIR (empty = disabled)"
	)]
	pub cache: Option<PathBuf>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum OutputFormat {
	Tsv,
	Json,
	#[cfg(feature = "pretty")]
	Tree,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ColorChoice {
	Auto,
	Always,
	Never,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum Charset {
	Utf8,
	Ascii,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum OutputMode {
	Default,
	Count,
	Quiet,
}

#[derive(Debug, ClapArgs)]
pub struct ManifestArgs {
	#[arg(
		value_name = "PATH",
		help = "manifest file (Cargo.toml / package.json / pom.xml / pyproject.toml / go.mod / *.csproj) or a directory to walk for any of those"
	)]
	pub path: PathBuf,

	#[arg(long, value_enum, default_value_t = ManifestFormat::Tsv)]
	pub format: ManifestFormat,

	#[arg(long, conflicts_with = "quiet", help = "print only the row count")]
	pub count: bool,
	#[arg(
		long,
		conflicts_with = "count",
		help = "suppress output, exit code only"
	)]
	pub quiet: bool,

	#[arg(
		long,
		value_name = "SCHEME",
		help = "URI scheme for package_moniker; defaults to code+moniker://"
	)]
	pub scheme: Option<String>,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, ValueEnum)]
pub enum ManifestFormat {
	Tsv,
	Json,
	#[cfg(feature = "pretty")]
	Tree,
}

impl ManifestArgs {
	pub fn mode(&self) -> OutputMode {
		if self.count {
			OutputMode::Count
		} else if self.quiet {
			OutputMode::Quiet
		} else {
			OutputMode::Default
		}
	}
}

impl ExtractArgs {
	#[cfg(test)]
	pub(crate) fn for_tests() -> Self {
		ExtractArgs {
			path: "a.ts".into(),
			where_: Vec::new(),
			kind: vec![],
			shape: vec![],
			format: OutputFormat::Tsv,
			color: ColorChoice::Never,
			charset: Charset::Utf8,
			count: false,
			quiet: false,
			with_text: false,
			scheme: None,
			project: None,
			cache: None,
		}
	}

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

fn shape_parser() -> impl TypedValueParser<Value = Shape> {
	PossibleValuesParser::new(Shape::ALL.iter().map(|s| s.as_str())).map(|s| {
		s.parse::<Shape>()
			.expect("PossibleValuesParser pre-validated")
	})
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

	fn extract(argv: &[&str]) -> ExtractArgs {
		let mut full = vec!["extract"];
		full.extend_from_slice(argv);
		let cli = parse(&full).unwrap();
		match cli.command {
			Command::Extract(a) => a,
			other => panic!("expected Extract, got {other:?}"),
		}
	}

	#[test]
	fn no_args_requires_subcommand() {
		assert!(
			parse(&[]).is_err(),
			"empty argv must error — subcommand required"
		);
	}

	#[test]
	fn minimal_invocation() {
		let a = extract(&["a.ts"]);
		assert_eq!(a.path, PathBuf::from("a.ts"));
		assert_eq!(a.format, OutputFormat::Tsv);
		assert_eq!(a.mode(), OutputMode::Default);
		assert!(a.kind.is_empty());
		assert!(!a.with_text);
	}

	#[test]
	fn quiet_and_count_are_mutually_exclusive() {
		assert!(parse(&["extract", "a.ts", "--count", "--quiet"]).is_err());
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
		assert!(parse(&["extract", "a.ts", "--format", "xml"]).is_err());
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
			Command::Check(c) => assert_eq!(c.path, PathBuf::from("a.ts")),
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
			Command::Check(c) => {
				assert_eq!(c.rules, PathBuf::from("my-rules.toml"));
				assert_eq!(c.format, CheckFormat::Json);
				assert!(!c.report);
			}
			other => panic!("expected Check, got {other:?}"),
		}
	}

	#[test]
	fn check_subcommand_accepts_report() {
		let cli = parse(&["check", "a.ts", "--report"]).unwrap();
		match cli.command {
			Command::Check(c) => assert!(c.report),
			other => panic!("expected Check, got {other:?}"),
		}
	}
}
