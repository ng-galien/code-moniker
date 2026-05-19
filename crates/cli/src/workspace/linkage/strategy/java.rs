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
		let method_name = bare_callable_name(method_name);
		let arity = reference_arity(ctx.reference);
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
		let method_name = bare_callable_name(method_name);
		let arity = reference_arity(ctx.reference);
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
	if let Some(owner) = method_owner_target(reference) {
		let defs = java_type_defs_for_target(ctx, &owner);
		if !defs.is_empty() {
			return defs;
		}
	}
	Vec::new()
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
		let method_name = bare_callable_name(method_name);
		let arity = reference_arity(reference);
		for type_def in receiver_type_defs(ctx, reference, depth + 1) {
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

fn method_owner_target(reference: &RefRecord) -> Option<Moniker> {
	if reference.kind != kinds::METHOD_CALL {
		return None;
	}
	let view = reference.target.as_view();
	let segments: Vec<_> = view.segments().collect();
	let last = segments.last()?;
	if last.kind != kinds::METHOD {
		return None;
	}
	let owner = reference.target.parent()?;
	let owner_last = owner.as_view().segments().last()?;
	if is_java_type_kind(owner_last.kind) || owner_last.kind == kinds::PATH {
		return Some(owner);
	}
	None
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

fn java_method_return_type_target(index: &SessionIndex, method: DefLocation) -> Option<Moniker> {
	index.files[method.file]
		.graph
		.refs()
		.filter(|reference| reference.source == method.def && reference.kind == kinds::USES_TYPE)
		.min_by_key(|reference| {
			reference
				.position
				.map(|position| position.0)
				.unwrap_or(u32::MAX)
		})
		.map(|reference| reference.target.clone())
}

fn reference_arity(reference: &RefRecord) -> Option<usize> {
	let name = last_segment_name(&reference.target)?;
	callable_arity(name)
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
	let mut count = 1usize;
	let mut angle = 0usize;
	let mut paren = 0usize;
	let mut bracket = 0usize;
	for byte in args {
		match *byte {
			b'<' => angle += 1,
			b'>' => angle = angle.saturating_sub(1),
			b'(' => paren += 1,
			b')' => paren = paren.saturating_sub(1),
			b'[' => bracket += 1,
			b']' => bracket = bracket.saturating_sub(1),
			b',' if angle == 0 && paren == 0 && bracket == 0 => count += 1,
			_ => {}
		}
	}
	Some(count)
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
				.filter_map(|method| java_method_return_type_target(ctx.index, method))
				.any(|target| java_external_target(&target))
		});
	}
	method_owner_target(reference).is_some_and(|owner| java_external_target(&owner))
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

#[cfg(test)]
mod tests {
	use super::callable_arity;

	#[test]
	fn callable_arity_ignores_commas_inside_generic_types() {
		assert_eq!(callable_arity(b"save(map:Map<String,String>)"), Some(1));
		assert_eq!(
			callable_arity(b"put(id:String,map:Map<String,List<Integer>>)"),
			Some(2)
		);
		assert_eq!(callable_arity(b"empty()"), Some(0));
		assert_eq!(callable_arity(b"placeholder(_,_)"), Some(2));
	}
}
