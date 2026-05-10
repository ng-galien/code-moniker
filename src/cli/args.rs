use std::path::PathBuf;

use clap::{Parser, ValueEnum};

use crate::cli::predicate::Predicate;
use crate::core::moniker::Moniker;
use crate::core::uri::{UriConfig, from_uri};

#[derive(Debug, Parser)]
#[command(
	name = "pg-moniker",
	about = "Single-file moniker / code_graph extraction; see docs/CLI.md",
	version
)]
pub struct Args {
	pub file: PathBuf,

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

	fn parse(argv: &[&str]) -> Result<Args, clap::Error> {
		let mut full = vec!["pg-moniker"];
		full.extend_from_slice(argv);
		Args::try_parse_from(full)
	}

	#[test]
	fn requires_file_argument() {
		assert!(parse(&[]).is_err());
	}

	#[test]
	fn minimal_invocation() {
		let a = parse(&["a.ts"]).unwrap();
		assert_eq!(a.file, PathBuf::from("a.ts"));
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
		assert_eq!(
			parse(&["a.ts", "--count"]).unwrap().mode(),
			OutputMode::Count
		);
	}

	#[test]
	fn quiet_mode_detected() {
		assert_eq!(
			parse(&["a.ts", "--quiet"]).unwrap().mode(),
			OutputMode::Quiet
		);
	}

	#[test]
	fn format_json_recognised() {
		assert_eq!(
			parse(&["a.ts", "--format", "json"]).unwrap().format,
			OutputFormat::Json
		);
	}

	#[test]
	fn unknown_format_rejected() {
		assert!(parse(&["a.ts", "--format", "xml"]).is_err());
	}

	#[test]
	fn kind_is_repeatable() {
		let a = parse(&["a.ts", "--kind", "class", "--kind", "method"]).unwrap();
		assert_eq!(a.kind, vec!["class".to_string(), "method".to_string()]);
	}

	#[test]
	fn with_text_flag() {
		assert!(parse(&["a.ts", "--with-text"]).unwrap().with_text);
	}

	#[test]
	fn predicate_uri_parses() {
		let a = parse(&["a.ts", "--descendant-of", "ts+moniker://./class:Foo"]).unwrap();
		let preds = a.compiled_predicates("ts+moniker://").expect("uri ok");
		assert_eq!(preds.len(), 1);
		match &preds[0] {
			Predicate::DescendantOf(_) => {}
			other => panic!("unexpected: {other:?}"),
		}
	}

	#[test]
	fn predicate_uri_malformed_is_usage_error() {
		let a = parse(&["a.ts", "--eq", "not a uri"]).unwrap();
		let err = a.compiled_predicates("ts+moniker://").unwrap_err();
		let msg = format!("{err:#}");
		assert!(msg.contains("--eq"), "expected flag in error: {msg}");
	}

	#[test]
	fn multiple_predicates_all_compile() {
		let a = parse(&[
			"a.ts",
			"--descendant-of",
			"ts+moniker://./class:Foo",
			"--gt",
			"ts+moniker://./class:Bar",
		])
		.unwrap();
		let preds = a.compiled_predicates("ts+moniker://").expect("uri ok");
		assert_eq!(preds.len(), 2);
	}

	#[test]
	fn explicit_scheme_overrides_default() {
		let a = parse(&[
			"a.ts",
			"--scheme",
			"my-scheme://",
			"--eq",
			"my-scheme://./class:Foo",
		])
		.unwrap();
		assert!(a.compiled_predicates("ts+moniker://").is_ok());
	}
}
