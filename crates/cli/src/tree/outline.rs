use std::collections::BTreeMap;
use std::io::Write;

use anstyle::Style;
use rustc_hash::FxHashMap;

use super::strategy::TreeStrategy;
use super::style::{Palette, TreeOpts};
use crate::args::ExtractArgs;
use crate::lines::line_range;
use crate::predicate::{MatchSet, RefMatch};
use crate::render_uri;
use code_moniker_core::core::code_graph::DefRecord;
use code_moniker_core::core::kinds::{KIND_COMMENT, KIND_LOCAL, KIND_PARAM};
use code_moniker_core::core::uri::UriConfig;

const NOISE_KINDS: &[&[u8]] = &[KIND_LOCAL, KIND_PARAM, KIND_COMMENT];

pub fn write_tree<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	write_tree_with_prefix(w, matches, source, args, scheme, "")
}

pub fn write_tree_with_prefix<W: Write>(
	w: &mut W,
	matches: &MatchSet<'_>,
	source: &str,
	args: &ExtractArgs,
	scheme: &str,
	prefix: &str,
) -> std::io::Result<()> {
	let cfg = UriConfig { scheme };
	let opts = TreeOpts::from_args(args);
	let user_filtered = !args.kind.is_empty();

	let kept_defs: Vec<&DefRecord> = matches
		.defs
		.iter()
		.copied()
		.filter(|d| user_filtered || !is_noise(&d.kind))
		.collect();

	let kept_refs: Vec<&RefMatch<'_>> = if user_filtered {
		matches.refs.iter().collect()
	} else {
		Vec::new()
	};

	if kept_defs.is_empty() && kept_refs.is_empty() {
		return Ok(());
	}

	let def_uris: Vec<String> = kept_defs
		.iter()
		.map(|d| render_uri(&d.moniker, &cfg))
		.collect();
	let mut refs_by_src: FxHashMap<String, Vec<&RefMatch<'_>>> = FxHashMap::default();
	for r in &kept_refs {
		refs_by_src
			.entry(render_uri(r.source, &cfg))
			.or_default()
			.push(r);
	}

	let split: Vec<Vec<&str>> = def_uris
		.iter()
		.map(|u| strip_fs_prefix(u.split('/').collect()))
		.collect();

	let mut root: Node = Node::default();
	for (i, d) in kept_defs.iter().enumerate() {
		let segs = &split[i];
		root.insert(segs, NodePayload::Def(d));
		if let Some(rs) = refs_by_src.get(&def_uris[i]) {
			for r in rs {
				root.insert(segs, NodePayload::Ref(r));
			}
		}
	}

	render(w, &root, prefix, true, &opts, &cfg, source)
}

#[derive(Default)]
struct Node<'a> {
	def: Option<&'a DefRecord>,
	refs: Vec<&'a RefMatch<'a>>,
	children: BTreeMap<String, Node<'a>>,
}

enum NodePayload<'a> {
	Def(&'a DefRecord),
	Ref(&'a RefMatch<'a>),
}

impl<'a> Node<'a> {
	fn insert(&mut self, segs: &[&str], payload: NodePayload<'a>) {
		let Some((head, rest)) = segs.split_first() else {
			match payload {
				NodePayload::Def(d) => self.def = Some(d),
				NodePayload::Ref(r) => self.refs.push(r),
			}
			return;
		};
		let entry = self.children.entry((*head).to_string()).or_default();
		entry.insert(rest, payload);
	}
}

fn render<W: Write>(
	w: &mut W,
	node: &Node<'_>,
	prefix: &str,
	is_top: bool,
	opts: &TreeOpts,
	cfg: &UriConfig<'_>,
	source: &str,
) -> std::io::Result<()> {
	let mut entries: Vec<(&String, &Node<'_>)> = node.children.iter().collect();
	entries.sort_by_key(|entry| tree_sort_key(entry.0, entry.1));

	let total = entries.len() + node.refs.len();
	let mut i = 0usize;

	for (seg, child) in &entries {
		let last = i + 1 == total;
		let (branch, cont) = branch_glyphs(is_top, last, opts);
		let (label, rendered_child) = collapsed_outline_label(seg, child, source, opts);
		writeln!(w, "{prefix}{branch}{label}")?;
		let next_prefix = format!("{prefix}{cont}");
		render(w, rendered_child, &next_prefix, false, opts, cfg, source)?;
		i += 1;
	}

	for r in &node.refs {
		let last = i + 1 == total;
		let (branch, _) = branch_glyphs(is_top, last, opts);
		let label = format_ref_label(r, cfg, opts);
		writeln!(w, "{prefix}{branch}{label}")?;
		i += 1;
	}
	Ok(())
}

#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct TreeSortKey {
	order: u16,
	line: u32,
	label: String,
}

fn tree_sort_key(seg: &str, node: &Node<'_>) -> TreeSortKey {
	let kind = node
		.def
		.and_then(|def| std::str::from_utf8(&def.kind).ok())
		.or_else(|| seg.split_once(':').map(|(kind, _)| kind))
		.unwrap_or("");
	let strategy = node
		.def
		.map(|def| TreeStrategy::from_moniker(&def.moniker))
		.unwrap_or_else(TreeStrategy::unknown);
	TreeSortKey {
		order: strategy.definition_order(kind),
		line: def_line(node),
		label: seg.to_string(),
	}
}

fn collapsed_outline_label<'a>(
	seg: &str,
	node: &'a Node<'a>,
	source: &str,
	opts: &TreeOpts,
) -> (String, &'a Node<'a>) {
	let strategy = node
		.def
		.map(|def| TreeStrategy::from_moniker(&def.moniker))
		.unwrap_or_else(TreeStrategy::unknown);
	let Some((kind, name)) = split_structural_seg(seg, strategy) else {
		return (format_seg_label(seg, node.def, source, opts), node);
	};
	let mut names = vec![name.to_string()];
	let mut current = node;
	while current.refs.is_empty() && current.children.len() == 1 {
		let Some((child_seg, child)) = current.children.iter().next() else {
			break;
		};
		let Some((child_kind, child_name)) = split_structural_seg(child_seg, strategy) else {
			break;
		};
		if child_kind != kind {
			break;
		}
		names.push(child_name.to_string());
		current = child;
	}
	if names.len() == 1 {
		return (format_seg_label(seg, node.def, source, opts), node);
	}
	(
		format_collapsed_structural_label(kind, &names, strategy, opts),
		current,
	)
}

fn split_structural_seg(seg: &str, strategy: TreeStrategy) -> Option<(&str, &str)> {
	let (kind, name) = seg.split_once(':')?;
	strategy
		.collapse_separator(kind)
		.is_some()
		.then_some((kind, name))
}

fn format_collapsed_structural_label(
	kind: &str,
	names: &[String],
	strategy: TreeStrategy,
	opts: &TreeOpts,
) -> String {
	let sep = strategy.collapse_separator(kind).unwrap_or("/");
	let name = names.join(sep);
	let p = &opts.palette;
	let kind_style = p.kind_style(strategy.definition_shape(kind));
	format!(
		"{kpre}{kind:<7}{kpost} {npre}{name}{npost}",
		kpre = kind_style.render(),
		kpost = kind_style.render_reset(),
		npre = p.name.render(),
		npost = p.name.render_reset(),
	)
}

fn branch_glyphs(is_top: bool, last: bool, opts: &TreeOpts) -> (String, String) {
	if is_top {
		("".to_string(), "".to_string())
	} else if last {
		(
			format!("{} ", opts.glyph.last),
			opts.glyph.skip_last.to_string(),
		)
	} else {
		(
			format!("{} ", opts.glyph.tee),
			opts.glyph.skip_mid.to_string(),
		)
	}
}

fn def_line(node: &Node<'_>) -> u32 {
	node.def
		.and_then(|d| d.position)
		.map(|(s, _)| s)
		.unwrap_or(u32::MAX)
}

fn format_seg_label(seg: &str, def: Option<&DefRecord>, source: &str, opts: &TreeOpts) -> String {
	let (kind_part, name_part) = seg.split_once(':').unwrap_or(("", seg));
	let (name_only, args_part) = match name_part.find('(') {
		Some(i) => (&name_part[..i], &name_part[i..]),
		None => (name_part, ""),
	};
	let kind_disp = def
		.map(|d| std::str::from_utf8(&d.kind).unwrap_or(kind_part))
		.unwrap_or(kind_part);
	let strategy = def
		.map(|d| TreeStrategy::from_moniker(&d.moniker))
		.unwrap_or_else(TreeStrategy::unknown);
	let lines = def
		.and_then(|d| d.position)
		.map(|(s, e)| {
			let (a, b) = line_range(source, s, e);
			if a == b {
				format!("  L{a}")
			} else {
				format!("  L{a}-L{b}")
			}
		})
		.unwrap_or_default();

	let p = &opts.palette;
	let args_colored = colorize_args(args_part, p);
	let kind_style = p.kind_style(strategy.definition_shape(kind_disp));
	format!(
		"{kpre}{kind_disp:<7}{kpost} {npre}{name_only}{npost}{args_colored}{rpre}{lines}{rpost}",
		kpre = kind_style.render(),
		kpost = kind_style.render_reset(),
		npre = p.name.render(),
		npost = p.name.render_reset(),
		rpre = p.range.render(),
		rpost = p.range.render_reset(),
	)
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ArgTok {
	Punct,
	Name,
	Type,
	Plain,
}

fn classify(c: char, paren_depth: usize, in_name: bool) -> ArgTok {
	match c {
		'(' | ')' => ArgTok::Punct,
		',' | ':' if paren_depth > 0 => ArgTok::Punct,
		_ if paren_depth > 0 && in_name => ArgTok::Name,
		_ if paren_depth > 0 => ArgTok::Type,
		_ => ArgTok::Plain,
	}
}

fn colorize_args(args: &str, p: &Palette) -> String {
	if args.is_empty() {
		return String::new();
	}
	let mut out = String::with_capacity(args.len() + 32);
	let mut cur_tok: Option<ArgTok> = None;
	let mut in_name = true;
	let mut paren_depth = 0usize;
	for c in args.chars() {
		let tok = classify(c, paren_depth, in_name);
		if cur_tok != Some(tok) {
			if let Some(prev) = cur_tok {
				write_close(&mut out, p, prev);
			}
			write_open(&mut out, p, tok);
			cur_tok = Some(tok);
		}
		out.push(c);
		match c {
			'(' => {
				paren_depth += 1;
				in_name = true;
			}
			')' => paren_depth = paren_depth.saturating_sub(1),
			',' if paren_depth > 0 => in_name = true,
			':' if paren_depth > 0 && in_name => in_name = false,
			_ => {}
		}
	}
	if let Some(prev) = cur_tok {
		write_close(&mut out, p, prev);
	}
	out
}

fn style_for(p: &Palette, tok: ArgTok) -> Style {
	match tok {
		ArgTok::Punct => p.punct,
		ArgTok::Name => p.arg_name,
		ArgTok::Type => p.arg_type,
		ArgTok::Plain => Style::new(),
	}
}

fn write_open(out: &mut String, p: &Palette, tok: ArgTok) {
	let s = style_for(p, tok);
	let ansi = s.render().to_string();
	out.push_str(&ansi);
}

fn write_close(out: &mut String, p: &Palette, tok: ArgTok) {
	let s = style_for(p, tok);
	let ansi = s.render_reset().to_string();
	out.push_str(&ansi);
}

fn format_ref_label(r: &RefMatch<'_>, cfg: &UriConfig<'_>, opts: &TreeOpts) -> String {
	let kind = std::str::from_utf8(&r.record.kind).unwrap_or("?");
	let target = render_uri(&r.record.target, cfg);
	let last_seg = target.rsplit('/').next().unwrap_or(&target);
	let target_name = last_seg.split_once(':').map_or(last_seg, |s| s.1);
	let p = &opts.palette;
	format!(
		"{apre}{arrow} {apost}{rkpre}{kind:<10}{rkpost} {dpre}{target_name}{dpost}",
		apre = p.arrow.render(),
		arrow = opts.glyph.arrow,
		apost = p.arrow.render_reset(),
		rkpre = p.ref_kind.render(),
		rkpost = p.ref_kind.render_reset(),
		dpre = p.dim.render(),
		dpost = p.dim.render_reset(),
	)
}

fn strip_fs_prefix(segs: Vec<&str>) -> Vec<&str> {
	let i = segs
		.iter()
		.position(|s| {
			if s.is_empty() || *s == "." || s.starts_with("code+moniker:") {
				return false;
			}
			let kind = s.split_once(':').map(|(k, _)| k).unwrap_or("");
			!matches!(kind, "lang" | "dir")
		})
		.unwrap_or(segs.len());
	segs.into_iter().skip(i).collect()
}

fn is_noise(kind: &[u8]) -> bool {
	NOISE_KINDS.contains(&kind)
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::args::{Charset, ColorChoice, OutputFormat};
	use crate::color::resolve_color;
	use code_moniker_core::core::code_graph::CodeGraph;
	use code_moniker_core::core::moniker::MonikerBuilder;

	fn base_args() -> ExtractArgs {
		let mut a = ExtractArgs::for_tests();
		a.format = OutputFormat::Tree;
		a
	}

	fn graph_class_method_and_local() -> CodeGraph {
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");

		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		let foo = b.build();
		g.add_def(foo.clone(), b"class", &root, Some((1, 0)))
			.unwrap();

		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		b.segment(b"method", b"bar");
		let bar = b.build();
		g.add_def(bar.clone(), b"method", &foo, Some((2, 2)))
			.unwrap();

		let mut b = MonikerBuilder::new();
		b.project(b"app");
		b.segment(b"class", b"Foo");
		b.segment(b"method", b"bar");
		b.segment(b"local", b"x");
		let local_x = b.build();
		g.add_def(local_x, b"local", &bar, Some((3, 3))).unwrap();

		g
	}

	fn graph_java_packaged_class() -> CodeGraph {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");

		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"java");
		b.segment(b"package", b"org");
		b.segment(b"package", b"apache");
		b.segment(b"package", b"bookkeeper");
		b.segment(b"module", b"Ledger");
		b.segment(b"class", b"Ledger");
		let ledger = b.build();
		g.add_def(ledger, b"class", &root, Some((0, 0))).unwrap();

		g
	}

	fn graph_java_field_method_source_order_reversed() -> CodeGraph {
		let mut b = MonikerBuilder::new();
		b.project(b".");
		let root = b.build();
		let mut g = CodeGraph::new(root.clone(), b"module");

		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"java");
		b.segment(b"package", b"app");
		b.segment(b"module", b"User");
		b.segment(b"class", b"User");
		let class = b.build();
		g.add_def(class.clone(), b"class", &root, Some((0, 0)))
			.unwrap();

		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"java");
		b.segment(b"package", b"app");
		b.segment(b"module", b"User");
		b.segment(b"class", b"User");
		b.segment(b"method", b"compute()");
		let method = b.build();
		g.add_def(method, b"method", &class, Some((1, 1))).unwrap();

		let mut b = MonikerBuilder::new();
		b.project(b".");
		b.segment(b"lang", b"java");
		b.segment(b"package", b"app");
		b.segment(b"module", b"User");
		b.segment(b"class", b"User");
		b.segment(b"field", b"total");
		let field = b.build();
		g.add_def(field, b"field", &class, Some((10, 10))).unwrap();

		g
	}

	#[test]
	fn structural_only_by_default_hides_locals() {
		let g = graph_class_method_and_local();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &base_args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("Foo"), "class missing: {s}");
		assert!(s.contains("bar"), "method missing: {s}");
		assert!(
			!s.contains("local"),
			"local should be hidden by default: {s}"
		);
		assert!(
			!s.contains("code+moniker"),
			"URI header should not appear: {s}"
		);
	}

	#[test]
	fn tree_collapses_linear_package_chain() {
		let g = graph_java_packaged_class();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &base_args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("package org.apache.bookkeeper"), "{s}");
		assert_eq!(s.matches("package").count(), 1, "{s}");
		assert!(s.contains("module"), "{s}");
		assert!(s.contains("class"), "{s}");
	}

	#[test]
	fn tree_orders_defs_by_language_kind_contract() {
		let g = graph_java_field_method_source_order_reversed();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &base_args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		let field = s
			.find("field")
			.unwrap_or_else(|| panic!("missing field: {s}"));
		let method = s
			.find("method")
			.unwrap_or_else(|| panic!("missing method: {s}"));

		assert!(
			field < method,
			"Java field should be ordered before method by the language kind contract: {s}"
		);
	}

	#[test]
	fn explicit_kind_local_reveals_them() {
		let g = graph_class_method_and_local();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut args = base_args();
		args.kind = vec!["local".into()];
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &args, "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(
			s.contains("local"),
			"user-requested local kind should appear: {s}"
		);
	}

	#[test]
	fn ascii_charset_avoids_unicode_glyphs() {
		let g = graph_class_method_and_local();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut args = base_args();
		args.charset = Charset::Ascii;
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &args, "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.is_ascii(), "ascii mode produced non-ASCII: {s:?}");
	}

	#[test]
	fn always_color_emits_ansi_escapes() {
		let g = graph_class_method_and_local();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let mut args = base_args();
		args.color = ColorChoice::Always;
		unsafe { std::env::remove_var("NO_COLOR") };
		let mut buf = Vec::new();
		write_tree(&mut buf, &matches, "", &args, "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(
			s.contains("\x1b["),
			"no ANSI escape in always-color output: {s:?}"
		);
	}

	#[test]
	fn color_always_wins_over_no_color_for_tree() {
		unsafe { std::env::set_var("NO_COLOR", "1") };
		assert!(resolve_color(ColorChoice::Always));
		unsafe { std::env::remove_var("NO_COLOR") };
	}
}
