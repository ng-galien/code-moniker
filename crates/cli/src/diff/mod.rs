//! CLI adapter for the semantic change review. Owns argument mapping and
//! the text/JSON rendering of `SemanticReview` facts; the engine lives in
//! `code-moniker-workspace::changes::semantic`.

mod render_json;
mod render_text;
mod scan;

use std::io::Write;
use std::path::Path;

use code_moniker_workspace::changes::semantic::model::{SemanticKind, SymbolChange};
use code_moniker_workspace::changes::semantic::review::FileFacts;

use crate::Exit;
use crate::args::{DiffArgs, DiffFormat};

pub fn run<W1: Write, W2: Write>(args: &DiffArgs, stdout: &mut W1, stderr: &mut W2) -> Exit {
	let review = match scan::semantic_review(args) {
		Ok(review) => review,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			return Exit::UsageError;
		}
	};
	let rendered = match args.format {
		DiffFormat::Text => render_text::write_review(stdout, &review, args.refs),
		DiffFormat::Json => render_json::write_review(stdout, &review),
	};
	match rendered {
		Ok(()) => Exit::Match,
		Err(error) => {
			let _ = writeln!(stderr, "code-moniker: {error:#}");
			Exit::UsageError
		}
	}
}

pub(in crate::diff) fn change_primary_path(change: &SymbolChange) -> &Path {
	change
		.new
		.as_ref()
		.or(change.old.as_ref())
		.map(|side| side.file_path.as_path())
		.expect("a change has at least one side")
}

pub(in crate::diff) fn facts_primary_path(facts: &FileFacts) -> Option<&Path> {
	facts
		.rollup
		.new_path
		.as_deref()
		.or(facts.rollup.old_path.as_deref())
}

pub(in crate::diff) fn is_pure_move(change: &SymbolChange) -> bool {
	change.kind == SemanticKind::Moved
		&& !change.facets.body_changed
		&& !change.facets.signature_changed
		&& !change.facets.visibility_changed
		&& !change.facets.header_changed
}
