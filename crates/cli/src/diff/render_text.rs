use std::io::Write;

use code_moniker_workspace::changes::semantic::model::{RefChange, SemanticKind, SymbolChange};
use code_moniker_workspace::changes::semantic::review::{FileFacts, SemanticReview};

use super::{change_primary_path, facts_primary_path, is_pure_move};

pub fn write_review(
	out: &mut impl Write,
	review: &SemanticReview,
	detail_refs: bool,
) -> anyhow::Result<()> {
	writeln!(out, "scope {}", review.scope)?;
	writeln!(out, "{}", summary_line(review))?;
	for facts in &review.files {
		writeln!(out)?;
		writeln!(out, "{}", file_heading(facts))?;
		if !facts.analyzable {
			continue;
		}
		write_file_symbols(out, review, facts)?;
		write_file_refs(out, review, facts, detail_refs)?;
		write_residual(out, facts)?;
	}
	for diagnostic in &review.diagnostics {
		writeln!(out, "diagnostic: {diagnostic}")?;
	}
	Ok(())
}

fn summary_line(review: &SemanticReview) -> String {
	let analyzable = review.files.iter().filter(|facts| facts.analyzable).count();
	let retargeted = review
		.ref_changes
		.iter()
		.filter(|change| change.kind.is_retarget())
		.count();
	let residual = review
		.files
		.iter()
		.filter(|facts| !facts.coverage.explained())
		.count();
	format!(
		"files {} ({} analyzable) | symbol facts {} | ref facts {} ({} retargeted) | residual files {}",
		review.files.len(),
		analyzable,
		review.symbol_changes.len(),
		review.ref_changes.len(),
		retargeted,
		residual
	)
}

fn file_heading(facts: &FileFacts) -> String {
	let path = match (&facts.rollup.old_path, &facts.rollup.new_path) {
		(Some(old), Some(new)) if old != new => {
			format!("{} -> {}", old.display(), new.display())
		}
		(_, Some(new)) => new.display().to_string(),
		(Some(old), None) => old.display().to_string(),
		(None, None) => "<unknown>".to_string(),
	};
	let suffix = if facts.analyzable {
		""
	} else {
		" (not analyzable)"
	};
	format!("{path}  {}{suffix}", facts.rollup.disposition.label())
}

fn write_file_symbols(
	out: &mut impl Write,
	review: &SemanticReview,
	facts: &FileFacts,
) -> anyhow::Result<()> {
	let Some(primary) = facts_primary_path(facts) else {
		return Ok(());
	};
	let mut pure_moves = 0usize;
	for change in &review.symbol_changes {
		if change_primary_path(change) != primary {
			continue;
		}
		if is_pure_move(change) {
			pure_moves += 1;
			continue;
		}
		writeln!(out, "  {}", symbol_line(change))?;
	}
	if pure_moves > 0 {
		writeln!(out, "  = {pure_moves} symbol(s) moved, no other facts")?;
	}
	Ok(())
}

fn symbol_line(change: &SymbolChange) -> String {
	let side = change
		.new
		.as_ref()
		.or(change.old.as_ref())
		.expect("a change has at least one side");
	let subject = match (change.kind, &change.old, &change.new) {
		(SemanticKind::Renamed | SemanticKind::SignatureChanged, Some(old), Some(new)) => {
			format!("{} {} -> {}", old.kind, old.name, new.name)
		}
		_ => format!("{} {}", side.kind, side.name),
	};
	let mut line = format!(
		"{} {subject}  {}",
		kind_marker(change.kind),
		change.kind.label()
	);
	line.push_str(&facet_suffix(change));
	if change.confidence.label() != "certain" {
		line.push_str(&format!(" [{}]", change.confidence.label()));
	}
	line
}

fn kind_marker(kind: SemanticKind) -> char {
	match kind {
		SemanticKind::Added => '+',
		SemanticKind::Removed => '-',
		SemanticKind::BodyModified => '~',
		SemanticKind::SignatureChanged => '#',
		SemanticKind::Renamed => '>',
		SemanticKind::Moved => '=',
		SemanticKind::AttributeChanged => '!',
	}
}

fn facet_suffix(change: &SymbolChange) -> String {
	let mut extras = Vec::new();
	if change.facets.body_changed && change.kind != SemanticKind::BodyModified {
		extras.push("body");
	}
	if change.facets.signature_changed && change.kind != SemanticKind::SignatureChanged {
		extras.push("signature");
	}
	if change.facets.visibility_changed {
		extras.push("visibility");
	}
	if change.facets.header_changed {
		extras.push("header");
	}
	if extras.is_empty() {
		String::new()
	} else {
		format!(" +{}", extras.join(" +"))
	}
}

fn write_file_refs(
	out: &mut impl Write,
	review: &SemanticReview,
	facts: &FileFacts,
	detail_refs: bool,
) -> anyhow::Result<()> {
	let Some(primary) = facts_primary_path(facts) else {
		return Ok(());
	};
	let file_refs: Vec<&RefChange> = review
		.ref_changes
		.iter()
		.filter(|change| change.file_path == primary)
		.collect();
	if file_refs.is_empty() {
		return Ok(());
	}
	if detail_refs {
		for change in &file_refs {
			writeln!(out, "  {}", ref_line(change))?;
		}
		return Ok(());
	}
	let retargeted = file_refs
		.iter()
		.filter(|change| change.kind.is_retarget())
		.count();
	let other = file_refs.len() - retargeted;
	let mut parts = Vec::new();
	if retargeted > 0 {
		parts.push(format!("{retargeted} retargeted"));
	}
	if other > 0 {
		parts.push(format!("{other} added/removed"));
	}
	writeln!(out, "  refs: {} (--refs for detail)", parts.join(", "))?;
	Ok(())
}

fn ref_line(change: &RefChange) -> String {
	let lines = change
		.new_line_range
		.or(change.old_line_range)
		.map(|(start, _)| format!(" L{start}"))
		.unwrap_or_default();
	format!("{} {}{lines}", change.kind.label(), change.ref_kind)
}

fn write_residual(out: &mut impl Write, facts: &FileFacts) -> anyhow::Result<()> {
	if facts.coverage.explained() {
		return Ok(());
	}
	writeln!(
		out,
		"  residual: old {} | new {}",
		spans_text(&facts.coverage.old_residual),
		spans_text(&facts.coverage.new_residual)
	)?;
	Ok(())
}

fn spans_text(spans: &[(u32, u32)]) -> String {
	if spans.is_empty() {
		return "-".to_string();
	}
	spans
		.iter()
		.map(|(start, end)| {
			if start == end {
				format!("{start}")
			} else {
				format!("{start}-{end}")
			}
		})
		.collect::<Vec<_>>()
		.join(",")
}
