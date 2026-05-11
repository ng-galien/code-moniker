use std::io::Write;
use std::path::Path;

use serde::Serialize;

use crate::cli::args::Args;
use crate::cli::extract;
use crate::cli::lines::line_range;
use crate::cli::predicate::MatchSet;
use crate::cli::render_uri;
use crate::core::code_graph::{DefRecord, RefRecord};
use crate::core::kinds::KIND_COMMENT;
use crate::core::uri::UriConfig;
use crate::lang::Lang;

pub fn write_tsv<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &Args,
	scheme: &str,
) -> std::io::Result<()> {
	let cfg = UriConfig { scheme };
	for d in &matches.defs {
		let uri = render_uri(&d.moniker, &cfg);
		write!(
			w,
			"def\t{uri}\t{kind}\t{pos}\t{lines}\t{vis}\t{sig}\t{origin}",
			kind = utf8_or_dash(&d.kind),
			pos = pos_or_dash(d.position),
			lines = lines_or_dash(d.position, source),
			vis = utf8_or_dash(&d.visibility),
			sig = utf8_or_dash(&d.signature),
			origin = utf8_or_dash(&d.origin),
		)?;
		if args.with_text && d.kind == KIND_COMMENT {
			let text = comment_slice(source, d);
			write!(w, "\t{}", escape_tsv(text))?;
		}
		writeln!(w)?;
	}
	for r in &matches.refs {
		let target = render_uri(&r.target, &cfg);
		writeln!(
			w,
			"ref\t{target}\t{kind}\t{pos}\t{lines}\tsource_idx={src}\t{alias}\t{conf}\t{rcv}",
			kind = utf8_or_dash(&r.kind),
			pos = pos_or_dash(r.position),
			lines = lines_or_dash(r.position, source),
			src = r.source,
			alias = utf8_or_dash(&r.alias),
			conf = utf8_or_dash(&r.confidence),
			rcv = utf8_or_dash(&r.receiver_hint),
		)?;
	}
	Ok(())
}

pub fn write_json<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &Args,
	lang: Lang,
	path: &Path,
	scheme: &str,
) -> anyhow::Result<()> {
	let cfg = UriConfig { scheme };
	let defs: Vec<DefView> = matches
		.defs
		.iter()
		.map(|d| DefView::from(d, &cfg, args.with_text, source))
		.collect();
	let refs: Vec<RefView> = matches
		.refs
		.iter()
		.map(|r| RefView::from(r, &cfg, source))
		.collect();
	let out = JsonOutput {
		uri: extract::file_uri(path),
		lang: lang.tag(),
		matches: Matches { defs, refs },
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

#[derive(Serialize)]
struct JsonOutput<'a> {
	uri: String,
	lang: &'a str,
	matches: Matches<'a>,
}

#[derive(Serialize)]
struct Matches<'a> {
	defs: Vec<DefView<'a>>,
	refs: Vec<RefView<'a>>,
}

#[derive(Serialize)]
struct DefView<'a> {
	moniker: String,
	kind: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	position: Option<[u32; 2]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	lines: Option<[u32; 2]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	visibility: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	signature: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	binding: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	origin: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	text: Option<String>,
}

impl<'a> DefView<'a> {
	fn from(d: &'a DefRecord, cfg: &UriConfig<'_>, with_text: bool, source: &str) -> Self {
		let text = if with_text && d.kind == KIND_COMMENT {
			Some(comment_slice(source, d).to_string())
		} else {
			None
		};
		Self {
			moniker: render_uri(&d.moniker, cfg),
			kind: std::str::from_utf8(&d.kind).unwrap_or(""),
			position: d.position.map(|(l, c)| [l, c]),
			lines: d.position.map(|(s, e)| {
				let (a, b) = line_range(source, s, e);
				[a, b]
			}),
			visibility: nullable(&d.visibility),
			signature: nullable(&d.signature),
			binding: nullable(&d.binding),
			origin: nullable(&d.origin),
			text,
		}
	}
}

#[derive(Serialize)]
struct RefView<'a> {
	source_idx: usize,
	target: String,
	kind: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	position: Option<[u32; 2]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	lines: Option<[u32; 2]>,
	#[serde(skip_serializing_if = "Option::is_none")]
	alias: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	confidence: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	receiver_hint: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	binding: Option<&'a str>,
}

impl<'a> RefView<'a> {
	fn from(r: &'a RefRecord, cfg: &UriConfig<'_>, source: &str) -> Self {
		Self {
			source_idx: r.source,
			target: render_uri(&r.target, cfg),
			kind: std::str::from_utf8(&r.kind).unwrap_or(""),
			position: r.position.map(|(l, c)| [l, c]),
			lines: r.position.map(|(s, e)| {
				let (a, b) = line_range(source, s, e);
				[a, b]
			}),
			alias: nullable(&r.alias),
			confidence: nullable(&r.confidence),
			receiver_hint: nullable(&r.receiver_hint),
			binding: nullable(&r.binding),
		}
	}
}

fn nullable(b: &[u8]) -> Option<&str> {
	if b.is_empty() {
		None
	} else {
		std::str::from_utf8(b).ok()
	}
}

fn utf8_or_dash(b: &[u8]) -> &str {
	if b.is_empty() {
		"-"
	} else {
		std::str::from_utf8(b).unwrap_or("-")
	}
}

fn pos_or_dash(p: Option<(u32, u32)>) -> String {
	match p {
		Some((start, end)) => format!("{start}..{end}"),
		None => "-".to_string(),
	}
}

fn lines_or_dash(p: Option<(u32, u32)>, source: &str) -> String {
	match p {
		Some((start, end)) => {
			let (a, b) = line_range(source, start, end);
			format!("L{a}-L{b}")
		}
		None => "-".to_string(),
	}
}

fn comment_slice<'a>(source: &'a str, d: &DefRecord) -> &'a str {
	let Some((start, end)) = d.position else {
		return "";
	};
	let bytes = source.as_bytes();
	let s = (start as usize).min(bytes.len());
	let e = (end as usize).min(bytes.len()).max(s);
	std::str::from_utf8(&bytes[s..e]).unwrap_or("")
}

fn escape_tsv(s: &str) -> String {
	s.replace('\\', "\\\\")
		.replace('\t', "\\t")
		.replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::cli::args::OutputFormat;
	use crate::cli::predicate::MatchSet;
	use crate::core::code_graph::CodeGraph;
	use crate::core::moniker::{Moniker, MonikerBuilder};

	fn args() -> Args {
		Args {
			file: Some("a.ts".into()),
			where_: Vec::new(),
			kind: vec![],
			format: OutputFormat::Tsv,
			count: false,
			quiet: false,
			with_text: false,
			scheme: None,
		}
	}

	fn build_graph_with_class_and_method() -> (CodeGraph, Moniker, Moniker) {
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		b.segment(b"method", b"bar");
		let bar = b.build();
		g.add_def(foo.clone(), b"class", &root, Some((1, 0)))
			.unwrap();
		g.add_def(bar.clone(), b"method", &foo, Some((2, 2)))
			.unwrap();
		(g, foo, bar)
	}

	#[test]
	fn tsv_emits_one_line_per_def() {
		let (g, _, _) = build_graph_with_class_and_method();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tsv(&mut buf, &matches, "", &args(), "ts+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert_eq!(s.lines().count(), 3);
		for line in s.lines() {
			assert!(line.starts_with("def\t"));
			assert_eq!(line.matches('\t').count(), 7, "tsv columns: {line}");
		}
	}

	#[test]
	fn tsv_renders_line_range_when_position_is_known() {
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"function", b"foo");
		let foo = b.build();
		let source = "line1\nfn foo() {\n  body\n}\nline5\n";
		g.add_def(foo.clone(), b"function", &root, Some((6, 25)))
			.unwrap();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tsv(&mut buf, &matches, source, &args(), "ts+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("\tL2-L4\t"), "missing line range column: {s}");
	}

	#[test]
	fn tsv_renders_moniker_uri_with_supplied_scheme() {
		let (g, foo, _) = build_graph_with_class_and_method();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tsv(&mut buf, &matches, "", &args(), "ts+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(
			s.contains("ts+moniker://app/class:Foo"),
			"missing canonical URI in: {s}"
		);
	}

	#[test]
	fn json_top_level_shape() {
		let (g, _, _) = build_graph_with_class_and_method();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_json(
			&mut buf,
			&matches,
			"",
			&args(),
			Lang::Ts,
			Path::new("a.ts"),
			"ts+moniker://",
		)
		.unwrap();
		let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
		assert_eq!(v["lang"].as_str(), Some("ts"));
		assert!(v["uri"].as_str().unwrap().starts_with("file://"));
		assert!(v["matches"]["defs"].is_array());
		assert!(v["matches"]["refs"].is_array());
		assert_eq!(v["matches"]["defs"].as_array().unwrap().len(), 3);
	}

	#[test]
	fn json_includes_line_range_alongside_byte_position() {
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		let foo = b.build();
		let source = "line1\nclass Foo {\n  body\n}\nline5\n";
		g.add_def(foo.clone(), b"class", &root, Some((6, 26)))
			.unwrap();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_json(
			&mut buf,
			&matches,
			source,
			&args(),
			Lang::Ts,
			Path::new("a.ts"),
			"ts+moniker://",
		)
		.unwrap();
		let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
		let def = &v["matches"]["defs"][0];
		assert_eq!(def["position"], serde_json::json!([6, 26]));
		assert_eq!(def["lines"], serde_json::json!([2, 4]));
	}

	#[test]
	fn json_skips_empty_attribute_fields() {
		let (g, foo, _) = build_graph_with_class_and_method();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_json(
			&mut buf,
			&matches,
			"",
			&args(),
			Lang::Ts,
			Path::new("a.ts"),
			"ts+moniker://",
		)
		.unwrap();
		let v: serde_json::Value = serde_json::from_slice(&buf).unwrap();
		let def = &v["matches"]["defs"][0];
		assert!(
			def.get("text").is_none(),
			"no text field without --with-text"
		);
	}

	#[test]
	fn comment_text_extraction_uses_byte_range() {
		let src = "line0\n// hello\nline2\n";
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"comment", b"6");
		let m = b.build();
		let d = DefRecord {
			moniker: m,
			kind: b"comment".to_vec(),
			parent: Some(0),
			position: Some((6, 14)),
			visibility: vec![],
			signature: vec![],
			binding: vec![],
			origin: vec![],
		};
		assert_eq!(comment_slice(src, &d), "// hello");
	}

	#[test]
	fn tsv_escape_handles_tabs_and_newlines() {
		assert_eq!(escape_tsv("a\tb"), "a\\tb");
		assert_eq!(escape_tsv("a\nb"), "a\\nb");
		assert_eq!(escape_tsv("a\\b"), "a\\\\b");
	}
}
