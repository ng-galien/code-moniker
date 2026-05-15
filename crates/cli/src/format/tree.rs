use std::collections::BTreeMap;
use std::io::Write;

use anstyle::{AnsiColor, Style};
use rustc_hash::FxHashMap;

use crate::args::{Charset, ColorChoice, ExtractArgs};
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
	entries.sort_by(|a, b| def_line(a.1).cmp(&def_line(b.1)).then_with(|| a.0.cmp(b.0)));

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

fn collapsed_outline_label<'a>(
	seg: &str,
	node: &'a Node<'a>,
	source: &str,
	opts: &TreeOpts,
) -> (String, &'a Node<'a>) {
	let Some((kind, name)) = split_structural_seg(seg) else {
		return (format_seg_label(seg, node.def, source, opts), node);
	};
	let mut names = vec![name.to_string()];
	let mut current = node;
	while current.refs.is_empty() && current.children.len() == 1 {
		let Some((child_seg, child)) = current.children.iter().next() else {
			break;
		};
		let Some((child_kind, child_name)) = split_structural_seg(child_seg) else {
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
		format_collapsed_structural_label(kind, &names, opts),
		current,
	)
}

fn split_structural_seg(seg: &str) -> Option<(&str, &str)> {
	let (kind, name) = seg.split_once(':')?;
	matches!(kind, "package" | "dir").then_some((kind, name))
}

fn format_collapsed_structural_label(kind: &str, names: &[String], opts: &TreeOpts) -> String {
	let sep = if kind == "package" { "." } else { "/" };
	let name = names.join(sep);
	let p = &opts.palette;
	format!(
		"{kpre}{kind:<7}{kpost} {npre}{name}{npost}",
		kpre = p.kind.render(),
		kpost = p.kind.render_reset(),
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
	format!(
		"{kpre}{kind_disp:<7}{kpost} {npre}{name_only}{npost}{args_colored}{rpre}{lines}{rpost}",
		kpre = p.kind.render(),
		kpost = p.kind.render_reset(),
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

struct TreeOpts {
	glyph: Glyphs,
	palette: Palette,
}

impl TreeOpts {
	fn from_args(args: &ExtractArgs) -> Self {
		let glyph = match args.charset {
			Charset::Utf8 => Glyphs::utf8(),
			Charset::Ascii => Glyphs::ascii(),
		};
		let palette = if resolve_color(args.color) {
			Palette::ansi()
		} else {
			Palette::none()
		};
		Self { glyph, palette }
	}
}

struct Glyphs {
	tee: &'static str,
	last: &'static str,
	skip_mid: &'static str,
	skip_last: &'static str,
	arrow: &'static str,
}

impl Glyphs {
	fn utf8() -> Self {
		Self {
			tee: "├──",
			last: "└──",
			skip_mid: "│   ",
			skip_last: "    ",
			arrow: "→",
		}
	}
	fn ascii() -> Self {
		Self {
			tee: "+--",
			last: "+--",
			skip_mid: "|   ",
			skip_last: "    ",
			arrow: "->",
		}
	}
}

struct Palette {
	kind: Style,
	name: Style,
	range: Style,
	arrow: Style,
	ref_kind: Style,
	dim: Style,
	punct: Style,
	arg_name: Style,
	arg_type: Style,
}

impl Palette {
	fn none() -> Self {
		Self {
			kind: Style::new(),
			name: Style::new(),
			range: Style::new(),
			arrow: Style::new(),
			ref_kind: Style::new(),
			dim: Style::new(),
			punct: Style::new(),
			arg_name: Style::new(),
			arg_type: Style::new(),
		}
	}
	fn ansi() -> Self {
		Self {
			kind: Style::new().fg_color(Some(AnsiColor::Cyan.into())),
			name: Style::new().bold(),
			range: Style::new().fg_color(Some(AnsiColor::Green.into())),
			arrow: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
			ref_kind: Style::new().fg_color(Some(AnsiColor::Magenta.into())),
			dim: Style::new()
				.fg_color(Some(AnsiColor::BrightBlack.into()))
				.dimmed(),
			punct: Style::new().fg_color(Some(AnsiColor::BrightBlack.into())),
			arg_name: Style::new().fg_color(Some(AnsiColor::Yellow.into())),
			arg_type: Style::new().fg_color(Some(AnsiColor::Blue.into())),
		}
	}
}

fn resolve_color(arg: ColorChoice) -> bool {
	use std::io::IsTerminal;
	if std::env::var_os("NO_COLOR").is_some() {
		return false;
	}
	if std::env::var_os("CLICOLOR_FORCE").is_some_and(|v| v != "0") {
		return true;
	}
	match arg {
		ColorChoice::Always => true,
		ColorChoice::Never => false,
		ColorChoice::Auto => {
			if std::env::var("TERM").is_ok_and(|t| t == "dumb") {
				return false;
			}
			if std::env::var("CLICOLOR").is_ok_and(|v| v == "0") {
				return false;
			}
			std::io::stdout().is_terminal()
		}
	}
}

pub fn write_file_header<W: Write>(
	w: &mut W,
	path: &std::path::Path,
	args: &ExtractArgs,
) -> std::io::Result<()> {
	let opts = TreeOpts::from_args(args);
	let style = opts.palette.name;
	writeln!(
		w,
		"\n{}── {} ──{}",
		style.render(),
		path.display(),
		style.render_reset()
	)
}

pub struct FileEntry<'a> {
	pub rel_path: String,
	pub matches: MatchSet<'a>,
	pub source: &'a str,
}

pub fn write_files_tree<W: Write>(
	w: &mut W,
	files: &[FileEntry<'_>],
	args: &ExtractArgs,
	scheme: &str,
) -> std::io::Result<()> {
	let opts = TreeOpts::from_args(args);
	let mut trie: FileTrie = FileTrie::default();
	for (i, f) in files.iter().enumerate() {
		let segs: Vec<&str> = f.rel_path.split('/').filter(|s| !s.is_empty()).collect();
		trie.insert(&segs, i);
	}
	render_file_trie(w, &trie, "", files, args, scheme, &opts)
}

type FileTrie = LeafTrie<usize>;

fn render_file_trie<W: Write>(
	w: &mut W,
	node: &FileTrie,
	prefix: &str,
	files: &[FileEntry<'_>],
	args: &ExtractArgs,
	scheme: &str,
	opts: &TreeOpts,
) -> std::io::Result<()> {
	let total = node.children.len();
	for (i, (name, child)) in node.children.iter().enumerate() {
		let last = i + 1 == total;
		let branch = if last {
			opts.glyph.last
		} else {
			opts.glyph.tee
		};
		let cont = if last {
			opts.glyph.skip_last
		} else {
			opts.glyph.skip_mid
		};
		let (label_name, rendered_child) = collapsed_leaf_label(name, child);
		let is_dir = rendered_child.leaf.is_none();
		let suffix = if is_dir { "/" } else { "" };
		writeln!(
			w,
			"{prefix}{branch} {hpre}{label_name}{suffix}{hpost}",
			hpre = opts.palette.name.render(),
			hpost = opts.palette.name.render_reset(),
		)?;
		let sub_prefix = format!("{prefix}{cont}");
		if let Some(idx) = rendered_child.leaf {
			let f = &files[idx];
			write_tree_with_prefix(w, &f.matches, f.source, args, scheme, &sub_prefix)?;
		} else {
			render_file_trie(w, rendered_child, &sub_prefix, files, args, scheme, opts)?;
		}
	}
	Ok(())
}

fn collapsed_leaf_label<'a, T>(name: &str, node: &'a LeafTrie<T>) -> (String, &'a LeafTrie<T>) {
	let mut names = vec![name.to_string()];
	let mut current = node;
	while current.leaf.is_none() && current.children.len() == 1 {
		let Some((child_name, child)) = current.children.iter().next() else {
			break;
		};
		names.push(child_name.clone());
		current = child;
	}
	(names.join("/"), current)
}

pub fn render_dir_tree<W: Write>(
	w: &mut W,
	entries: &[(String, String)],
	args: &ExtractArgs,
) -> std::io::Result<()> {
	let opts = TreeOpts::from_args(args);
	let mut root: PathNode = PathNode::default();
	for (path, label) in entries {
		let segs: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
		root.insert(&segs, label.clone());
	}
	render_path_node(w, &root, "", &opts)
}

type PathNode = LeafTrie<String>;

struct LeafTrie<T> {
	leaf: Option<T>,
	children: BTreeMap<String, LeafTrie<T>>,
}

impl<T> Default for LeafTrie<T> {
	fn default() -> Self {
		Self {
			leaf: None,
			children: BTreeMap::new(),
		}
	}
}

impl<T> LeafTrie<T> {
	fn insert(&mut self, segs: &[&str], val: T) {
		let Some((head, rest)) = segs.split_first() else {
			self.leaf = Some(val);
			return;
		};
		self.children
			.entry((*head).to_string())
			.or_default()
			.insert(rest, val);
	}
}

fn render_path_node<W: Write>(
	w: &mut W,
	node: &PathNode,
	prefix: &str,
	opts: &TreeOpts,
) -> std::io::Result<()> {
	let total = node.children.len();
	for (i, (seg, child)) in node.children.iter().enumerate() {
		let last = i + 1 == total;
		let (label_seg, rendered_child) = collapsed_leaf_label(seg, child);
		let branch = if last {
			opts.glyph.last
		} else {
			opts.glyph.tee
		};
		let cont = if last {
			opts.glyph.skip_last
		} else {
			opts.glyph.skip_mid
		};
		let label = match &rendered_child.leaf {
			Some(l) => format!(
				"{npre}{label_seg}{npost} {dpre}{l}{dpost}",
				npre = opts.palette.name.render(),
				npost = opts.palette.name.render_reset(),
				dpre = opts.palette.dim.render(),
				dpost = opts.palette.dim.render_reset(),
			),
			None => format!(
				"{kpre}{label_seg}/{kpost}",
				kpre = opts.palette.kind.render(),
				kpost = opts.palette.kind.render_reset(),
			),
		};
		writeln!(w, "{prefix}{branch} {label}")?;
		let next_prefix = format!("{prefix}{cont}");
		render_path_node(w, rendered_child, &next_prefix, opts)?;
	}
	Ok(())
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::args::OutputFormat;
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
	fn file_tree_collapses_linear_directory_chain() {
		let g = graph_class_method_and_local();
		let matches = MatchSet {
			defs: g.defs().collect(),
			refs: vec![],
		};
		let files = [FileEntry {
			rel_path: "src/main/java/Foo.java".to_string(),
			matches,
			source: "",
		}];
		let mut buf = Vec::new();
		write_files_tree(&mut buf, &files, &base_args(), "code+moniker://").unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("src/main/java/Foo.java"), "{s}");
		assert!(!s.contains("src/\n"), "{s}");
	}

	#[test]
	fn directory_summary_tree_collapses_linear_directory_chain() {
		let entries = [(
			"src/main/java/Foo.java".to_string(),
			"files=1 defs=1 refs=0".to_string(),
		)];
		let mut buf = Vec::new();
		render_dir_tree(&mut buf, &entries, &base_args()).unwrap();
		let s = String::from_utf8(buf).unwrap();
		assert!(s.contains("src/main/java/Foo.java"), "{s}");
		assert!(!s.contains("src/\n"), "{s}");
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
	fn no_color_env_disables_color_even_with_always() {
		unsafe { std::env::set_var("NO_COLOR", "1") };
		assert!(!resolve_color(ColorChoice::Always));
		unsafe { std::env::remove_var("NO_COLOR") };
	}
}
