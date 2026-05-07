//! Refs extraction: imports today; calls, extends/implements,
//! type uses to come.

use tree_sitter::Node;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::{Moniker, MonikerBuilder};

use super::canonicalize::{append_path_segments, node_position, strip_known_extension};
use super::walker::Walker;

impl<'src> Walker<'src> {
	pub(super) fn handle_import(&self, node: Node<'_>, parent: &Moniker, graph: &mut CodeGraph) {
		let Some(src_node) = node.child_by_field_name("source") else { return };
		let raw = src_node.utf8_text(self.source_bytes).unwrap_or("");
		let path = raw.trim_matches(|c| c == '"' || c == '\'');
		let target = self.build_import_target(path);
		let _ = graph.add_ref(
			parent,
			target,
			self.kinds.import_label,
			Some(node_position(node)),
		);
	}

	/// Resolve a relative import path against the importer's directory.
	///
	/// `./foo` siblings the importer; `../foo` walks up one directory
	/// per occurrence; the trailing extension is stripped. Bare imports
	/// (no `./` or `../` prefix) keep the legacy single-segment form
	/// today — they will get an external-package shape in a later phase.
	fn build_import_target(&self, raw_path: &str) -> Moniker {
		let importer_view = self.module.as_view();

		// Bare specifier: legacy single-segment encoding under project root.
		// Stable for `=` but not yet a real external moniker shape.
		if !raw_path.starts_with("./") && !raw_path.starts_with("../") {
			let mut b = MonikerBuilder::new();
			b.project(importer_view.project());
			b.segment(self.kinds.path, raw_path.as_bytes());
			return b.build();
		}

		// Relative: seed from importer, drop the filename, then walk up
		// per "../" and append remaining segments.
		let mut b = MonikerBuilder::from_view(importer_view);
		let mut depth = (importer_view.segment_count() as usize).saturating_sub(1);
		b.truncate(depth);

		let mut remainder = raw_path;
		while let Some(rest) = remainder.strip_prefix("./") {
			remainder = rest;
		}
		while let Some(rest) = remainder.strip_prefix("../") {
			depth = depth.saturating_sub(1);
			b.truncate(depth);
			remainder = rest;
		}
		let remainder = strip_known_extension(remainder);
		append_path_segments(&mut b, remainder, self.kinds.path);
		b.build()
	}
}
