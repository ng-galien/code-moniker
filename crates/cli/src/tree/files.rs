// code-moniker: ignore-file[smell-long-parameter-list]
// TODO(smell): introduce a file-tree rendering context that groups args, output, path prefix, and style settings before enabling this guardrail here.
use std::collections::BTreeMap;
use std::io::Write;

use super::outline::write_tree_with_prefix;
use super::style::TreeOpts;
use crate::args::ExtractArgs;
use crate::predicate::MatchSet;

pub(crate) struct FileEntry<'a> {
	pub(crate) rel_path: String,
	pub(crate) matches: MatchSet<'a>,
	pub(crate) source: &'a str,
}

pub(crate) fn write_files_tree<W: Write>(
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

#[cfg(test)]
mod tests {
	use code_moniker_core::core::code_graph::CodeGraph;
	use code_moniker_core::core::moniker::MonikerBuilder;

	use super::*;
	use crate::args::OutputFormat;

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
		g.add_def(bar, b"method", &foo, Some((2, 2))).unwrap();

		g
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
}
