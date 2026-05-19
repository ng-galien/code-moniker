use code_moniker_core::core::code_graph::RefRecord;
use code_moniker_core::core::moniker::{Moniker, MonikerBuilder, Segment};
use code_moniker_core::lang::{Lang, kinds};

use super::{CandidateDef, CandidateKeys, LinkageQuery, LinkageStrategy, UnresolvedClassification};
use crate::workspace::index::IndexedFile;
use crate::workspace::linkage::{
	DefLocation, LinkKey, SessionIndex, bare_callable_name, moniker_matches_without_project,
};

pub(super) struct JavaLinkageStrategy;

impl LinkageStrategy for JavaLinkageStrategy {
	fn allow_generic_candidates(&self, ctx: &LinkageQuery<'_>) -> bool {
		!(ctx.reference.kind == kinds::METHOD_CALL
			&& matches!(
				ctx.reference.receiver_hint.as_slice(),
				kinds::HINT_CALL | kinds::HINT_MEMBER
			))
	}

	fn candidate_keys(&self, ctx: &LinkageQuery<'_>, out: &mut CandidateKeys) {
		if let Some(key) = java_import_type_key(&ctx.reference.target, true) {
			out.exact.push(key);
		}
		if let Some(key) = java_import_type_key(&ctx.reference.target, false) {
			out.projectless.push(key);
		}
	}

	fn candidate_defs(&self, ctx: &LinkageQuery<'_>, out: &mut Vec<CandidateDef>) {
		if is_java_type_ref(ctx.reference) {
			self.same_package_type_defs(ctx, out);
		}
		if ctx.reference.kind == kinds::READS {
			self.enclosing_field_defs(ctx, out);
		}
		if ctx.reference.kind == kinds::METHOD_CALL {
			self.receiver_member_defs(ctx, out);
		}
	}

	fn def_matches(&self, ctx: &LinkageQuery<'_>, def: &DefLocation) -> bool {
		if is_java_type_ref(ctx.reference) && self.same_package_type_def_matches(ctx, def) {
			return true;
		}
		if ctx.reference.kind == kinds::READS && self.enclosing_field_def_matches(ctx, def) {
			return true;
		}
		if ctx.reference.kind == kinds::METHOD_CALL && self.receiver_member_def_matches(ctx, def) {
			return true;
		}
		false
	}

	fn classify_unresolved(&self, ctx: &LinkageQuery<'_>) -> UnresolvedClassification {
		if java_external_target(&ctx.reference.target) {
			return UnresolvedClassification::External;
		}
		if java_external_member_ref(ctx) {
			return UnresolvedClassification::External;
		}
		if java_suppressed_member_ref(ctx.reference) {
			return UnresolvedClassification::Suppressed;
		}
		UnresolvedClassification::Actionable
	}
}

impl JavaLinkageStrategy {
	fn same_package_type_defs(&self, ctx: &LinkageQuery<'_>, out: &mut Vec<CandidateDef>) {
		let Some((package, name)) = same_package_type_target(ctx) else {
			return;
		};
		for (file_idx, file) in ctx.index.files.iter().enumerate() {
			if file.lang != Lang::Java || !java_file_package_matches(file, &package) {
				continue;
			}
			for (def_idx, def) in file.graph.defs().enumerate() {
				if !is_java_top_level_type(&def.kind) {
					continue;
				}
				let Some(last) = def.moniker.as_view().segments().last() else {
					continue;
				};
				if last.name == name {
					out.push(CandidateDef {
						loc: DefLocation {
							file: file_idx,
							def: def_idx,
						},
					});
				}
			}
		}
	}

	fn same_package_type_def_matches(&self, ctx: &LinkageQuery<'_>, def: &DefLocation) -> bool {
		let Some((package, name)) = same_package_type_target(ctx) else {
			return false;
		};
		let file = &ctx.index.files[def.file];
		if file.lang != Lang::Java || !java_file_package_matches(file, &package) {
			return false;
		}
		let def = ctx.index.def(def);
		if !is_java_top_level_type(&def.kind) {
			return false;
		}
		def.moniker
			.as_view()
			.segments()
			.last()
			.is_some_and(|segment| segment.name == name)
	}

	fn enclosing_field_defs(&self, ctx: &LinkageQuery<'_>, out: &mut Vec<CandidateDef>) {
		let Some(name) = last_segment_name(&ctx.reference.target) else {
			return;
		};
		for loc in java_enclosing_field_defs(ctx, name) {
			out.push(CandidateDef { loc });
		}
	}

	fn enclosing_field_def_matches(&self, ctx: &LinkageQuery<'_>, def: &DefLocation) -> bool {
		let Some(name) = last_segment_name(&ctx.reference.target) else {
			return false;
		};
		java_enclosing_field_defs(ctx, name)
			.into_iter()
			.any(|loc| loc == *def)
	}

	fn receiver_member_defs(&self, ctx: &LinkageQuery<'_>, out: &mut Vec<CandidateDef>) {
		let Some(method_name) = last_segment_name(&ctx.reference.target) else {
			return;
		};
		let arity = call_arity(ctx.source_file, ctx.reference);
		for type_def in receiver_type_defs(ctx, ctx.reference, 0) {
			for loc in java_type_member_methods(ctx.index, type_def, method_name, arity) {
				out.push(CandidateDef { loc });
			}
		}
	}

	fn receiver_member_def_matches(&self, ctx: &LinkageQuery<'_>, def: &DefLocation) -> bool {
		let Some(method_name) = last_segment_name(&ctx.reference.target) else {
			return false;
		};
		let arity = call_arity(ctx.source_file, ctx.reference);
		receiver_type_defs(ctx, ctx.reference, 0)
			.into_iter()
			.any(|type_def| {
				java_type_member_methods(ctx.index, type_def, method_name, arity)
					.into_iter()
					.any(|loc| loc == *def)
			})
	}
}

fn java_import_type_key(moniker: &Moniker, include_project: bool) -> Option<LinkKey> {
	let view = moniker.as_view();
	let segments: Vec<_> = view.segments().collect();
	let last = segments.last()?;
	if last.kind != kinds::PATH {
		return None;
	}
	let parent = segments.get(segments.len().checked_sub(2)?)?;
	if parent.kind != kinds::MODULE || parent.name != last.name {
		return None;
	}
	Some(LinkKey::from_parts(
		include_project.then(|| view.project().to_vec()),
		segments[..segments.len() - 1]
			.iter()
			.map(|segment| (segment.kind.to_vec(), segment.name.to_vec()))
			.collect(),
		bare_callable_name(last.name).to_vec(),
	))
}

fn is_java_type_ref(reference: &RefRecord) -> bool {
	matches!(
		reference.kind.as_slice(),
		kinds::USES_TYPE
			| kinds::IMPLEMENTS
			| kinds::EXTENDS
			| kinds::INSTANTIATES
			| kinds::ANNOTATES
			| kinds::IMPORTS_SYMBOL
	)
}

fn same_package_type_target<'a>(ctx: &'a LinkageQuery<'_>) -> Option<(Vec<Vec<u8>>, &'a [u8])> {
	same_package_type_target_for_moniker(ctx.source_file, &ctx.reference.target)
}

fn same_package_type_target_for_moniker<'a>(
	source_file: &IndexedFile,
	target: &'a Moniker,
) -> Option<(Vec<Vec<u8>>, &'a [u8])> {
	let target_segments: Vec<_> = target.as_view().segments().collect();
	let source_segments: Vec<_> = source_file.graph.root().as_view().segments().collect();
	let target_module_idx = target_segments
		.iter()
		.position(|segment| segment.kind == kinds::MODULE)?;
	let source_module_idx = source_segments
		.iter()
		.position(|segment| segment.kind == kinds::MODULE)?;
	if target_segments[..target_module_idx] != source_segments[..source_module_idx] {
		return None;
	}
	let last = target_segments.last()?;
	if !is_java_type_kind(last.kind) {
		return None;
	}
	Some((
		source_segments[..source_module_idx]
			.iter()
			.filter(|segment| segment.kind == kinds::PACKAGE)
			.map(|segment| segment.name.to_vec())
			.collect(),
		last.name,
	))
}

fn java_file_package_matches(file: &IndexedFile, package: &[Vec<u8>]) -> bool {
	let segments: Vec<_> = file.graph.root().as_view().segments().collect();
	let root_module_idx = match segments
		.iter()
		.position(|segment| segment.kind == kinds::MODULE)
	{
		Some(idx) => idx,
		None => return false,
	};
	let file_package = segments[..root_module_idx]
		.iter()
		.filter(|segment| segment.kind == kinds::PACKAGE)
		.map(|segment| segment.name)
		.collect::<Vec<_>>();
	file_package.len() == package.len()
		&& file_package
			.iter()
			.zip(package)
			.all(|(left, right)| *left == right.as_slice())
}

fn is_java_top_level_type(kind: &[u8]) -> bool {
	is_java_type_kind(kind)
}

fn is_java_type_kind(kind: &[u8]) -> bool {
	matches!(
		kind,
		kinds::CLASS | kinds::INTERFACE | kinds::RECORD | kinds::ENUM | kinds::ANNOTATION_TYPE
	)
}

fn last_segment_name(moniker: &Moniker) -> Option<&[u8]> {
	moniker
		.as_view()
		.segments()
		.last()
		.map(|segment| segment.name)
}

fn java_enclosing_field_defs(ctx: &LinkageQuery<'_>, name: &[u8]) -> Vec<DefLocation> {
	let source = ctx.source_file.graph.def_at(ctx.reference.source);
	let Some(owner) = enclosing_java_type(&source.moniker) else {
		return Vec::new();
	};
	ctx.source_file
		.graph
		.defs()
		.enumerate()
		.filter_map(|(def_idx, def)| {
			if def.kind != kinds::FIELD || !owner.is_ancestor_of(&def.moniker) {
				return None;
			}
			def.moniker
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == name)
				.then_some(DefLocation {
					file: ctx.source_file_idx,
					def: def_idx,
				})
		})
		.collect()
}

fn receiver_type_defs(
	ctx: &LinkageQuery<'_>,
	reference: &RefRecord,
	depth: usize,
) -> Vec<DefLocation> {
	if depth > 8 {
		return Vec::new();
	}
	let receiver = reference.receiver_hint.as_slice();
	if !receiver.is_empty()
		&& !matches!(receiver, kinds::HINT_CALL | kinds::HINT_MEMBER)
		&& let Some(receiver_def) = receiver_symbol_def(ctx, reference, receiver)
		&& let Some(type_ref) = type_ref_for_symbol_def(ctx.index, receiver_def)
	{
		let defs = java_type_defs_for_target(ctx, &type_ref.target);
		if !defs.is_empty() {
			return defs;
		}
	}
	let mut defs = span_call_return_type_defs(ctx, reference, depth);
	if immediate_receiver_call(ctx, reference).is_none() {
		defs.extend(span_created_type_defs(ctx, reference));
	}
	defs.sort_by_key(|loc| (loc.file, loc.def));
	defs.dedup();
	defs
}

fn receiver_symbol_def(
	ctx: &LinkageQuery<'_>,
	reference: &RefRecord,
	receiver: &[u8],
) -> Option<DefLocation> {
	let source = ctx.source_file.graph.def_at(reference.source);
	let reference_start = reference.position.map(|position| position.0);
	let local_or_param = ctx
		.source_file
		.graph
		.defs()
		.enumerate()
		.filter_map(|(idx, def)| {
			if !matches!(def.kind.as_slice(), kinds::LOCAL | kinds::PARAM) {
				return None;
			}
			if def.parent != Some(reference.source) {
				return None;
			}
			if reference_start.is_some_and(|start| {
				def.kind == kinds::LOCAL && def.position.is_some_and(|position| position.0 > start)
			}) {
				return None;
			}
			if def.kind == kinds::LOCAL
				&& !local_scope_contains_reference(
					ctx.source_file,
					def.position,
					reference.position,
				) {
				return None;
			}
			def.moniker
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == receiver)
				.then_some((
					def.position.map(|position| position.0).unwrap_or(0),
					DefLocation {
						file: ctx.source_file_idx,
						def: idx,
					},
				))
		})
		.max_by_key(|(start, _)| *start)
		.map(|(_, loc)| loc);
	if local_or_param.is_some() {
		return local_or_param;
	}

	let owner = enclosing_java_type(&source.moniker)?;
	ctx.source_file
		.graph
		.defs()
		.enumerate()
		.find_map(|(idx, def)| {
			if def.kind != kinds::FIELD || !owner.is_ancestor_of(&def.moniker) {
				return None;
			}
			def.moniker
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| segment.name == receiver)
				.then_some(DefLocation {
					file: ctx.source_file_idx,
					def: idx,
				})
		})
}

fn type_ref_for_symbol_def(index: &SessionIndex, symbol: DefLocation) -> Option<&RefRecord> {
	let symbol_file = &index.files[symbol.file];
	let symbol_def = index.def(&symbol);
	let symbol_span = symbol_def.position?;
	if symbol_def.kind == kinds::METHOD
		&& let Some(type_ref) = symbol_file
			.graph
			.refs()
			.filter(|reference| {
				reference.source == symbol.def
					&& reference.kind == kinds::USES_TYPE
					&& reference.position.is_some_and(|position| {
						symbol_span.0 <= position.0 && position.1 <= symbol_span.1
					})
			})
			.min_by_key(|reference| {
				reference
					.position
					.map(|position| position.0)
					.unwrap_or(u32::MAX)
			}) {
		return Some(type_ref);
	}
	let source_idx = symbol_def.parent?;
	if let Some(type_ref) = symbol_file
		.graph
		.refs()
		.filter(|reference| {
			reference.source == source_idx
				&& reference.kind == kinds::USES_TYPE
				&& reference
					.position
					.is_some_and(|position| position.1 <= symbol_span.0)
		})
		.max_by_key(|reference| reference.position.map(|position| position.1).unwrap_or(0))
	{
		return Some(type_ref);
	}
	symbol_file
		.graph
		.refs()
		.filter(|reference| {
			reference.source == source_idx
				&& reference.kind == kinds::USES_TYPE
				&& reference.position.is_some_and(|position| {
					symbol_span.0 <= position.0 && position.1 <= symbol_span.1
				})
		})
		.max_by_key(|reference| reference.position.map(|position| position.1).unwrap_or(0))
}

fn local_scope_contains_reference(
	file: &IndexedFile,
	local_position: Option<(u32, u32)>,
	reference_position: Option<(u32, u32)>,
) -> bool {
	let Some((local_start, _)) = local_position else {
		return true;
	};
	let Some((reference_start, _)) = reference_position else {
		return true;
	};
	if reference_start < local_start {
		return false;
	}
	if let Some(loop_scope) = for_initializer_scope_containing(&file.source, local_start as usize) {
		return reference_start as usize <= loop_scope.1;
	}
	let Some(block) = innermost_brace_block_containing(&file.source, local_start as usize) else {
		return true;
	};
	reference_start as usize <= block.1
}

fn for_initializer_scope_containing(source: &str, offset: usize) -> Option<(usize, usize)> {
	for keyword in keyword_positions_before(source, "for", offset)
		.into_iter()
		.rev()
	{
		let open = source[keyword + 3..]
			.char_indices()
			.find(|(_, ch)| !ch.is_whitespace())
			.and_then(|(idx, ch)| (ch == '(').then_some(keyword + 3 + idx))?;
		let close = matching_close_paren(source, open)?;
		if offset < open || offset > close {
			continue;
		}
		let body_start = source[close + 1..]
			.char_indices()
			.find(|(_, ch)| !ch.is_whitespace())
			.and_then(|(idx, ch)| (ch == '{').then_some(close + 1 + idx))?;
		let body_end = matching_close_brace(source, body_start)?;
		return Some((keyword, body_end));
	}
	None
}

fn keyword_positions_before(source: &str, keyword: &str, before: usize) -> Vec<usize> {
	let mut out = Vec::new();
	let mut search_start = 0;
	while let Some(relative) = source[search_start..before].find(keyword) {
		let start = search_start + relative;
		let end = start + keyword.len();
		let before_ok = start == 0
			|| !source.as_bytes()[start - 1].is_ascii_alphanumeric()
				&& source.as_bytes()[start - 1] != b'_';
		let after_ok = end >= source.len()
			|| !source.as_bytes()[end].is_ascii_alphanumeric() && source.as_bytes()[end] != b'_';
		if before_ok && after_ok {
			out.push(start);
		}
		search_start = end;
	}
	out
}

fn innermost_brace_block_containing(source: &str, offset: usize) -> Option<(usize, usize)> {
	let bytes = source.as_bytes();
	let mut stack = Vec::new();
	let mut blocks = Vec::new();
	for (idx, byte) in bytes.iter().enumerate() {
		match *byte {
			b'{' => stack.push(idx),
			b'}' => {
				if let Some(start) = stack.pop() {
					blocks.push((start, idx));
				}
			}
			_ => {}
		}
	}
	blocks
		.into_iter()
		.filter(|(start, end)| *start <= offset && offset <= *end)
		.min_by_key(|(start, end)| end - start)
}

fn matching_close_brace(source: &str, open: usize) -> Option<usize> {
	let mut depth = 0usize;
	for (idx, ch) in source.char_indices().skip_while(|(idx, _)| *idx < open) {
		match ch {
			'{' => depth += 1,
			'}' => {
				depth = depth.saturating_sub(1);
				if depth == 0 {
					return Some(idx);
				}
			}
			_ => {}
		}
	}
	None
}

fn span_created_type_defs(ctx: &LinkageQuery<'_>, reference: &RefRecord) -> Vec<DefLocation> {
	let Some(call_span) = reference.position else {
		return Vec::new();
	};
	let mut out = Vec::new();
	for reference in ctx.source_file.graph.refs().filter(|candidate| {
		candidate.kind == kinds::INSTANTIATES
			&& candidate
				.position
				.is_some_and(|position| position.0 == call_span.0 && position.1 <= call_span.1)
	}) {
		out.extend(java_type_defs_for_target(ctx, &reference.target));
	}
	out.sort_by_key(|loc| (loc.file, loc.def));
	out.dedup();
	out
}

fn span_call_return_type_defs(
	ctx: &LinkageQuery<'_>,
	reference: &RefRecord,
	depth: usize,
) -> Vec<DefLocation> {
	if reference.receiver_hint != kinds::HINT_CALL {
		return Vec::new();
	}
	if reference.position.is_none() {
		return Vec::new();
	}
	let mut out = Vec::new();
	if let Some(inner) = immediate_receiver_call(ctx, reference) {
		for method in method_defs_for_call(ctx, inner, depth + 1) {
			if let Some(type_ref) = type_ref_for_symbol_def(ctx.index, method) {
				out.extend(java_type_defs_for_target(ctx, &type_ref.target));
			}
		}
	}
	out.sort_by_key(|loc| (loc.file, loc.def));
	out.dedup();
	out
}

fn immediate_receiver_call<'a>(
	ctx: &'a LinkageQuery<'_>,
	reference: &RefRecord,
) -> Option<&'a RefRecord> {
	let call_span = reference.position?;
	ctx.source_file
		.graph
		.refs()
		.filter(|candidate| {
			candidate.kind == kinds::METHOD_CALL
				&& candidate
					.position
					.is_some_and(|position| position.0 == call_span.0 && position.1 < call_span.1)
		})
		.max_by_key(|candidate| candidate.position.map(|position| position.1).unwrap_or(0))
}

fn method_defs_for_call(
	ctx: &LinkageQuery<'_>,
	reference: &RefRecord,
	depth: usize,
) -> Vec<DefLocation> {
	let mut out = Vec::new();
	if let Some(locs) = ctx.index.defs_by_moniker.get(&reference.target) {
		out.extend(
			locs.iter()
				.copied()
				.filter(|loc| ctx.index.def(loc).kind == kinds::METHOD),
		);
	}
	if out.is_empty()
		&& let Some(method_name) = last_segment_name(&reference.target)
	{
		let arity = call_arity(ctx.source_file, reference);
		for type_def in receiver_type_defs(ctx, reference, depth) {
			out.extend(java_type_member_methods(
				ctx.index,
				type_def,
				method_name,
				arity,
			));
		}
	}
	out.sort_by_key(|loc| (loc.file, loc.def));
	out.dedup();
	out
}

fn java_type_defs_for_target(ctx: &LinkageQuery<'_>, target: &Moniker) -> Vec<DefLocation> {
	let mut out = Vec::new();
	for (file_idx, file) in ctx.index.files.iter().enumerate() {
		if file.lang != Lang::Java {
			continue;
		}
		for (def_idx, def) in file.graph.defs().enumerate() {
			if !is_java_top_level_type(&def.kind) {
				continue;
			}
			if target.bind_match(&def.moniker)
				|| moniker_matches_without_project(target, &def.moniker)
			{
				out.push(DefLocation {
					file: file_idx,
					def: def_idx,
				});
				continue;
			}
			if same_package_type_target_for_moniker(ctx.source_file, target).is_some_and(
				|(package, name)| {
					java_file_package_matches(file, &package)
						&& def
							.moniker
							.as_view()
							.segments()
							.last()
							.is_some_and(|segment| segment.name == name)
				},
			) {
				out.push(DefLocation {
					file: file_idx,
					def: def_idx,
				});
			}
		}
	}
	out
}

fn java_type_member_methods(
	index: &SessionIndex,
	type_def: DefLocation,
	method_name: &[u8],
	arity: Option<usize>,
) -> Vec<DefLocation> {
	let owner = &index.def(&type_def).moniker;
	index.files[type_def.file]
		.graph
		.defs()
		.enumerate()
		.filter_map(|(def_idx, def)| {
			if def.kind != kinds::METHOD || !owner.is_ancestor_of(&def.moniker) {
				return None;
			}
			def.moniker
				.as_view()
				.segments()
				.last()
				.is_some_and(|segment| {
					bare_callable_name(segment.name) == method_name
						&& arity.is_none_or(|arity| callable_arity(segment.name) == Some(arity))
				})
				.then_some(DefLocation {
					file: type_def.file,
					def: def_idx,
				})
		})
		.collect()
}

fn call_arity(file: &IndexedFile, reference: &RefRecord) -> Option<usize> {
	let (start, end) = reference.position?;
	let call = file.source.get(start as usize..end as usize)?;
	let open = call.rfind('(')?;
	let close = matching_close_paren(call, open)?;
	Some(count_java_arguments(&call[open + 1..close]))
}

fn matching_close_paren(source: &str, open: usize) -> Option<usize> {
	let mut depth = 0usize;
	let mut in_string = false;
	let mut escaped = false;
	for (idx, ch) in source.char_indices().skip_while(|(idx, _)| *idx < open) {
		if in_string {
			if escaped {
				escaped = false;
			} else if ch == '\\' {
				escaped = true;
			} else if ch == '"' {
				in_string = false;
			}
			continue;
		}
		match ch {
			'"' => in_string = true,
			'(' => depth += 1,
			')' => {
				depth = depth.saturating_sub(1);
				if depth == 0 {
					return Some(idx);
				}
			}
			_ => {}
		}
	}
	None
}

fn count_java_arguments(args: &str) -> usize {
	if args.trim().is_empty() {
		return 0;
	}
	let mut count = 1usize;
	let mut paren = 0usize;
	let mut bracket = 0usize;
	let mut brace = 0usize;
	let mut in_string = false;
	let mut escaped = false;
	for ch in args.chars() {
		if in_string {
			if escaped {
				escaped = false;
			} else if ch == '\\' {
				escaped = true;
			} else if ch == '"' {
				in_string = false;
			}
			continue;
		}
		match ch {
			'"' => in_string = true,
			'(' => paren += 1,
			')' => paren = paren.saturating_sub(1),
			'[' => bracket += 1,
			']' => bracket = bracket.saturating_sub(1),
			'{' => brace += 1,
			'}' => brace = brace.saturating_sub(1),
			',' if paren == 0 && bracket == 0 && brace == 0 => count += 1,
			_ => {}
		}
	}
	count
}

fn callable_arity(name: &[u8]) -> Option<usize> {
	let open = name.iter().position(|b| *b == b'(')?;
	let close = name.iter().rposition(|b| *b == b')')?;
	if close <= open {
		return None;
	}
	let args = &name[open + 1..close];
	if args.is_empty() {
		return Some(0);
	}
	Some(args.iter().filter(|b| **b == b':').count())
}

fn enclosing_java_type(moniker: &Moniker) -> Option<Moniker> {
	let view = moniker.as_view();
	let segments: Vec<_> = view.segments().collect();
	let idx = segments
		.iter()
		.rposition(|segment| is_java_type_kind(segment.kind))?;
	let mut b = MonikerBuilder::new();
	b.project(view.project());
	for segment in &segments[..=idx] {
		b.segment(segment.kind, segment.name);
	}
	Some(b.build())
}

fn java_external_target(target: &Moniker) -> bool {
	let segments: Vec<_> = target.as_view().segments().collect();
	if segments
		.iter()
		.any(|segment| segment.kind == kinds::EXTERNAL_PKG)
	{
		return true;
	}
	let Some(last) = segments.last() else {
		return false;
	};
	if last.name == b"Override" && matches!(last.kind, kinds::ANNOTATION_TYPE | kinds::PATH) {
		return true;
	}
	segments
		.windows(3)
		.any(|window| java_lang_package_prefix(window[0], window[1], window[2]))
}

fn java_lang_package_prefix(first: Segment<'_>, second: Segment<'_>, third: Segment<'_>) -> bool {
	first.kind == kinds::LANG
		&& first.name == b"java"
		&& second.kind == kinds::PACKAGE
		&& second.name == b"java"
		&& third.kind == kinds::PACKAGE
		&& third.name == b"lang"
}

fn java_external_member_ref(ctx: &LinkageQuery<'_>) -> bool {
	if !matches!(
		ctx.reference.kind.as_slice(),
		kinds::METHOD_CALL | kinds::READS
	) {
		return false;
	}
	java_external_member_reference(ctx, ctx.reference)
}

fn java_external_member_reference(ctx: &LinkageQuery<'_>, reference: &RefRecord) -> bool {
	let receiver = reference.receiver_hint.as_slice();
	if receiver == kinds::HINT_CALL {
		return immediate_receiver_call(ctx, reference).is_some_and(|inner| {
			if java_external_member_reference(ctx, inner) {
				return true;
			}
			method_defs_for_call(ctx, inner, 0)
				.into_iter()
				.filter_map(|method| type_ref_for_symbol_def(ctx.index, method))
				.any(|type_ref| java_external_target(&type_ref.target))
		});
	}
	if receiver.is_empty() || matches!(receiver, kinds::HINT_CALL | kinds::HINT_MEMBER) {
		return false;
	}
	let Some(receiver_def) = receiver_symbol_def(ctx, reference, receiver) else {
		return false;
	};
	let Some(type_ref) = type_ref_for_symbol_def(ctx.index, receiver_def) else {
		return false;
	};
	java_external_target(&type_ref.target)
}

fn java_suppressed_member_ref(reference: &RefRecord) -> bool {
	if !matches!(
		reference.kind.as_slice(),
		kinds::METHOD_CALL | kinds::CALLS | kinds::READS
	) {
		return false;
	}
	let receiver = reference.receiver_hint.as_slice();
	if matches!(
		receiver,
		b"Math" | b"String" | b"System" | b"Long" | b"Integer" | b"Double" | b"Boolean"
	) {
		return true;
	}
	let Some(last) = reference.target.as_view().segments().last() else {
		return false;
	};
	matches!(
		last.name,
		b"System"
			| b"Math" | b"String"
			| b"Long" | b"Integer"
			| b"Double"
			| b"Boolean"
			| b"out" | b"println"
	) || ((receiver.is_empty() || matches!(receiver, kinds::HINT_CALL | kinds::HINT_MEMBER))
		&& matches!(
			last.name,
			b"abs"
				| b"format" | b"toString"
				| b"equalsIgnoreCase"
				| b"startsWith"
				| b"trim" | b"toLowerCase"
		))
}
