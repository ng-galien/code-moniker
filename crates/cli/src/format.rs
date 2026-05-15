use std::io::Write;

#[cfg(feature = "pretty")]
pub mod tree;
use std::path::Path;

use serde::Serialize;

use crate::args::{ExtractArgs, MonikerFormat};
use crate::color::resolve_color;
use crate::extract;
use crate::lines::line_range;
use crate::predicate::{MatchSet, RefMatch};
use crate::render_uri;
use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::kinds::KIND_COMMENT;
use code_moniker_core::core::uri::UriConfig;
use code_moniker_core::lang::Lang;

pub fn write_tsv<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	let cfg = UriConfig { scheme };
	for d in &matches.defs {
		let uri = render_moniker(&d.moniker, &cfg, args.moniker_format, false);
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
		let target = render_moniker(&r.record.target, &cfg, args.moniker_format, false);
		let src_uri = render_moniker(r.source, &cfg, args.moniker_format, false);
		writeln!(
			w,
			"ref\t{target}\t{kind}\t{pos}\t{lines}\tsource={src_uri}\t{alias}\t{conf}\t{rcv}",
			kind = utf8_or_dash(&r.record.kind),
			pos = pos_or_dash(r.record.position),
			lines = lines_or_dash(r.record.position, source),
			alias = utf8_or_dash(&r.record.alias),
			conf = utf8_or_dash(&r.record.confidence),
			rcv = utf8_or_dash(&r.record.receiver_hint),
		)?;
	}
	Ok(())
}

pub fn write_text<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	let cfg = UriConfig { scheme };
	let color = resolve_color(args.color);
	for d in &matches.defs {
		writeln!(
			w,
			"{}",
			render_moniker(&d.moniker, &cfg, args.moniker_format, color)
		)?;
	}
	for r in &matches.refs {
		writeln!(
			w,
			"{}",
			render_moniker(&r.record.target, &cfg, args.moniker_format, color)
		)?;
	}
	Ok(())
}

fn render_moniker(
	m: &code_moniker_core::core::moniker::Moniker,
	cfg: &UriConfig<'_>,
	format: MonikerFormat,
	color: bool,
) -> String {
	match format {
		MonikerFormat::Uri => render_uri(m, cfg),
		MonikerFormat::Compact => {
			render_compact_moniker(m, color).unwrap_or_else(|| render_uri(m, cfg))
		}
	}
}

fn render_compact_moniker(
	m: &code_moniker_core::core::moniker::Moniker,
	color: bool,
) -> Option<String> {
	let view = m.as_view();
	let mut lang: Option<String> = None;
	let mut packages: Vec<String> = Vec::new();
	let mut dirs: Vec<String> = Vec::new();
	let mut module: Option<String> = None;
	let mut rest: Vec<(String, String)> = Vec::new();
	for seg in view.segments() {
		let kind = std::str::from_utf8(seg.kind).ok()?.to_string();
		let name = std::str::from_utf8(seg.name).ok()?.to_string();
		match kind.as_str() {
			"lang" => lang = Some(name),
			"package" => packages.push(name),
			"dir" => dirs.push(name),
			"module" => module = Some(name),
			_ => rest.push((kind, name)),
		}
	}
	let head = lang.unwrap_or_else(|| {
		std::str::from_utf8(view.project())
			.unwrap_or(".")
			.to_string()
	});
	if packages.is_empty() && dirs.is_empty() && module.is_none() && rest.is_empty() {
		return Some(paint(color, "36", &head));
	}
	let mut out = String::new();
	out.push_str(&paint(color, "36", &head));
	out.push(':');
	let mut wrote_scope = false;
	if !packages.is_empty() {
		out.push_str(&paint(color, "2", &packages.join(".")));
		wrote_scope = true;
	} else if !dirs.is_empty() {
		out.push_str(&paint(color, "2", &dirs.join("/")));
		wrote_scope = true;
	}
	let has_module = module.is_some();
	if let Some(module) = module {
		if wrote_scope {
			out.push('/');
		}
		out.push_str(&paint(color, "34;1", &module));
	}
	for (idx, (kind, name)) in rest.iter().enumerate() {
		if idx == 0 && has_module {
			out.push('.');
		} else if wrote_scope || has_module || idx > 0 {
			out.push('/');
		}
		out.push_str(&paint(color, "35", kind));
		out.push(':');
		out.push_str(&paint(color, "32;1", name));
	}
	Some(out)
}

fn paint(color: bool, code: &str, text: &str) -> String {
	if color {
		format!("\x1b[{code}m{text}\x1b[0m")
	} else {
		text.to_string()
	}
}

pub fn write_json<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &ExtractArgs,
	lang: Lang,
	path: &Path,
	scheme: &str,
) -> anyhow::Result<()> {
	let out = JsonOutput {
		uri: extract::file_uri(path),
		lang: lang.tag(),
		matches: build_matches(matches, source, args, scheme),
	};
	serde_json::to_writer_pretty(&mut *w, &out)?;
	w.write_all(b"\n")?;
	Ok(())
}

pub fn build_matches_value(
	matches: &MatchSet<'_>,
	source: &str,
	args: &ExtractArgs,
	scheme: &str,
) -> serde_json::Value {
	serde_json::to_value(build_matches(matches, source, args, scheme))
		.expect("Matches<'_> is always serializable")
}

fn build_matches<'a>(
	matches: &'a MatchSet<'_>,
	source: &'a str,
	args: &ExtractArgs,
	scheme: &'a str,
) -> Matches<'a> {
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
	Matches { defs, refs }
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
	source: String,
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
	fn from(r: &'a RefMatch<'a>, cfg: &UriConfig<'_>, source: &str) -> Self {
		Self {
			source: render_uri(r.source, cfg),
			target: render_uri(&r.record.target, cfg),
			kind: std::str::from_utf8(&r.record.kind).unwrap_or(""),
			position: r.record.position.map(|(l, c)| [l, c]),
			lines: r.record.position.map(|(s, e)| {
				let (a, b) = line_range(source, s, e);
				[a, b]
			}),
			alias: nullable(&r.record.alias),
			confidence: nullable(&r.record.confidence),
			receiver_hint: nullable(&r.record.receiver_hint),
			binding: nullable(&r.record.binding),
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
		.replace('\r', "\\r")
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::predicate::MatchSet;
	use code_moniker_core::core::code_graph::CodeGraph;
	use code_moniker_core::core::moniker::{Moniker, MonikerBuilder};

	fn args() -> ExtractArgs {
		ExtractArgs::for_tests()
	}

	fn args_with_uri_monikers() -> ExtractArgs {
		let mut args = args();
		args.moniker_format = MonikerFormat::Uri;
		args
	}

	fn java_service_moniker() -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"java");
		b.segment(b"package", b"app");
		b.segment(b"package", b"user");
		b.segment(b"module", b"UserService");
		b.segment(b"class", b"UserService");
		b.build()
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
		write_tsv(&mut buf, &matches, "", &args(), "code+moniker://").unwrap();
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
		write_tsv(&mut buf, &matches, source, &args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("\tL2-L4\t"), "missing line range column: {s}");
	}

	#[test]
	fn tsv_renders_compact_monikers_by_default() {
		let (g, foo, _) = build_graph_with_class_and_method();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tsv(&mut buf, &matches, "", &args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(
			s.contains("\tapp:class:Foo\t"),
			"missing compact moniker in: {s}"
		);
		assert!(
			!s.contains("code+moniker://"),
			"default TSV should omit URI scheme: {s}"
		);
	}

	#[test]
	fn compact_moniker_collapses_lang_package_and_module() {
		let m = java_service_moniker();
		let rendered = render_compact_moniker(&m, false).unwrap();
		assert_eq!(rendered, "java:app.user/UserService.class:UserService");
	}

	#[test]
	fn text_colorizes_compact_monikers_when_forced() {
		unsafe { std::env::remove_var("NO_COLOR") };
		let m = java_service_moniker();
		let root = MonikerBuilder::new().project(b".").build();
		let mut g = CodeGraph::new(root.clone(), b"module");
		g.add_def(m, b"class", &root, None).unwrap();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut args = args();
		args.color = crate::args::ColorChoice::Always;
		let mut buf = Vec::new();
		write_text(&mut buf, &matches, &args, "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("\x1b["), "expected ANSI color escapes: {s}");
		assert!(s.contains("java"), "{s}");
	}

	#[test]
	fn color_always_wins_over_no_color_for_text() {
		unsafe { std::env::set_var("NO_COLOR", "1") };
		assert!(resolve_color(crate::args::ColorChoice::Always));
		unsafe { std::env::remove_var("NO_COLOR") };
	}

	#[test]
	fn color_never_wins_over_clicolor_force_for_text() {
		unsafe { std::env::set_var("CLICOLOR_FORCE", "1") };
		assert!(!resolve_color(crate::args::ColorChoice::Never));
		unsafe { std::env::remove_var("CLICOLOR_FORCE") };
	}

	#[test]
	fn tsv_can_render_moniker_uri_with_supplied_scheme() {
		let (g, foo, _) = build_graph_with_class_and_method();
		let foo_def = g.defs().find(|d| d.moniker == foo).unwrap();
		let matches = MatchSet {
			defs: vec![foo_def],
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tsv(
			&mut buf,
			&matches,
			"",
			&args_with_uri_monikers(),
			"code+moniker://",
		)
		.unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(
			s.contains("code+moniker://app/class:Foo"),
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
			"code+moniker://",
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
			"code+moniker://",
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
			"code+moniker://",
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
		assert_eq!(escape_tsv("a\rb"), "a\\rb");
		assert_eq!(escape_tsv("a\\b"), "a\\\\b");
	}
}
