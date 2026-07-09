use std::io::Write;

use code_moniker_core::core::uri::{UriConfig, to_uri};
use code_moniker_workspace::changes::semantic::model::{RefChange, SymbolChange, SymbolSide};
use code_moniker_workspace::changes::semantic::review::{FileFacts, SemanticReview};
use serde::Serialize;

use crate::DEFAULT_SCHEME;

#[derive(Serialize)]
struct ReviewOut<'a> {
	schema: &'static str,
	scope: &'a str,
	summary: SummaryOut,
	files: Vec<FileOut>,
	symbol_changes: Vec<SymbolChangeOut<'a>>,
	ref_changes: Vec<RefChangeOut>,
	diagnostics: &'a [String],
}

#[derive(Serialize)]
struct SummaryOut {
	files: usize,
	analyzable_files: usize,
	symbol_changes: usize,
	ref_changes: usize,
	retargeted_refs: usize,
	residual_files: usize,
}

#[derive(Serialize)]
struct FileOut {
	old_path: Option<String>,
	new_path: Option<String>,
	disposition: &'static str,
	analyzable: bool,
	symbol_changes: usize,
	moved_symbols: usize,
	coverage: CoverageOut,
}

#[derive(Serialize)]
struct CoverageOut {
	explained: bool,
	old_residual: Vec<(u32, u32)>,
	new_residual: Vec<(u32, u32)>,
}

#[derive(Serialize)]
struct SymbolChangeOut<'a> {
	kind: &'static str,
	confidence: &'static str,
	facets: FacetsOut,
	old: Option<SideOut<'a>>,
	new: Option<SideOut<'a>>,
}

#[derive(Serialize)]
struct FacetsOut {
	body_changed: bool,
	signature_changed: bool,
	visibility_changed: bool,
	header_changed: bool,
	file_moved: bool,
}

#[derive(Serialize)]
struct SideOut<'a> {
	identity: String,
	file: String,
	kind: &'a str,
	name: &'a str,
	visibility: &'a str,
	#[serde(skip_serializing_if = "Option::is_none")]
	signature: Option<&'a str>,
	#[serde(skip_serializing_if = "Option::is_none")]
	lines: Option<(u32, u32)>,
}

#[derive(Serialize)]
struct RefChangeOut {
	kind: &'static str,
	file: String,
	ref_kind: String,
	#[serde(skip_serializing_if = "Option::is_none")]
	old_target: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	new_target: Option<String>,
	#[serde(skip_serializing_if = "Option::is_none")]
	old_lines: Option<(u32, u32)>,
	#[serde(skip_serializing_if = "Option::is_none")]
	new_lines: Option<(u32, u32)>,
}

pub fn write_review(out: &mut impl Write, review: &SemanticReview) -> anyhow::Result<()> {
	let payload = ReviewOut {
		schema: "code-moniker.diff/1",
		scope: &review.scope,
		summary: summary_out(review),
		files: review.files.iter().map(file_out).collect(),
		symbol_changes: review.symbol_changes.iter().map(symbol_out).collect(),
		ref_changes: review.ref_changes.iter().map(ref_out).collect(),
		diagnostics: &review.diagnostics,
	};
	serde_json::to_writer_pretty(&mut *out, &payload)?;
	writeln!(out)?;
	Ok(())
}

fn summary_out(review: &SemanticReview) -> SummaryOut {
	SummaryOut {
		files: review.files.len(),
		analyzable_files: review.files.iter().filter(|facts| facts.analyzable).count(),
		symbol_changes: review.symbol_changes.len(),
		ref_changes: review.ref_changes.len(),
		retargeted_refs: review
			.ref_changes
			.iter()
			.filter(|change| change.kind.is_retarget())
			.count(),
		residual_files: review
			.files
			.iter()
			.filter(|facts| !facts.coverage.explained())
			.count(),
	}
}

fn file_out(facts: &FileFacts) -> FileOut {
	FileOut {
		old_path: facts
			.rollup
			.old_path
			.as_ref()
			.map(|path| path.display().to_string()),
		new_path: facts
			.rollup
			.new_path
			.as_ref()
			.map(|path| path.display().to_string()),
		disposition: facts.rollup.disposition.label(),
		analyzable: facts.analyzable,
		symbol_changes: facts.rollup.symbol_changes,
		moved_symbols: facts.rollup.moved_symbols,
		coverage: CoverageOut {
			explained: facts.coverage.explained(),
			old_residual: facts.coverage.old_residual.clone(),
			new_residual: facts.coverage.new_residual.clone(),
		},
	}
}

fn symbol_out(change: &SymbolChange) -> SymbolChangeOut<'_> {
	SymbolChangeOut {
		kind: change.kind.label(),
		confidence: change.confidence.label(),
		facets: FacetsOut {
			body_changed: change.facets.body_changed,
			signature_changed: change.facets.signature_changed,
			visibility_changed: change.facets.visibility_changed,
			header_changed: change.facets.header_changed,
			file_moved: change.facets.file_moved,
		},
		old: change.old.as_ref().map(side_out),
		new: change.new.as_ref().map(side_out),
	}
}

fn side_out(side: &SymbolSide) -> SideOut<'_> {
	SideOut {
		identity: to_uri(
			&side.moniker,
			&UriConfig {
				scheme: DEFAULT_SCHEME,
			},
		),
		file: side.file_path.display().to_string(),
		kind: &side.kind,
		name: &side.name,
		visibility: &side.visibility,
		signature: (!side.signature.is_empty()).then_some(side.signature.as_str()),
		lines: side.line_range,
	}
}

fn ref_out(change: &RefChange) -> RefChangeOut {
	let config = UriConfig {
		scheme: DEFAULT_SCHEME,
	};
	RefChangeOut {
		kind: change.kind.label(),
		file: change.file_path.display().to_string(),
		ref_kind: change.ref_kind.clone(),
		old_target: change
			.old_target
			.as_ref()
			.map(|target| to_uri(target, &config)),
		new_target: change
			.new_target
			.as_ref()
			.map(|target| to_uri(target, &config)),
		old_lines: change.old_line_range,
		new_lines: change.new_line_range,
	}
}
