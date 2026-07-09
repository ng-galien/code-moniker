use code_moniker_core::core::moniker::Moniker;
use rustc_hash::FxHasher;
use std::hash::Hasher;

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TailSegment {
	pub kind: Vec<u8>,
	pub name: Vec<u8>,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct IdentityTail {
	segments: Vec<TailSegment>,
}

impl IdentityTail {
	pub fn parent(&self) -> IdentityTail {
		let mut segments = self.segments.clone();
		segments.pop();
		IdentityTail { segments }
	}

	pub fn last(&self) -> Option<&TailSegment> {
		self.segments.last()
	}

	pub fn segments(&self) -> &[TailSegment] {
		&self.segments
	}

	pub fn starts_with(&self, prefix: &IdentityTail) -> bool {
		self.segments.starts_with(&prefix.segments)
	}

	pub fn rewrite_prefix(&self, from: &IdentityTail, to: &IdentityTail) -> Option<IdentityTail> {
		let rest = self.segments.strip_prefix(from.segments.as_slice())?;
		let mut segments = to.segments.clone();
		segments.extend_from_slice(rest);
		Some(IdentityTail { segments })
	}
}

pub fn identity_tail(moniker: &Moniker, root: &Moniker) -> Option<IdentityTail> {
	let root_view = root.as_view();
	let def_view = moniker.as_view();
	if !root_view.is_ancestor_of(&def_view) {
		return None;
	}
	let segments = def_view
		.segments()
		.skip(root_view.segment_count() as usize)
		.map(|segment| TailSegment {
			kind: segment.kind.to_vec(),
			name: segment.name.to_vec(),
		})
		.collect();
	Some(IdentityTail { segments })
}

pub fn split_callable_name(name: &[u8]) -> (&[u8], Option<&[u8]>) {
	match name.iter().position(|byte| *byte == b'(') {
		Some(open) => (&name[..open], Some(&name[open..])),
		None => (name, None),
	}
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct DefFingerprints {
	pub text: u64,
	pub body: u64,
}

pub struct FingerprintScope<'a> {
	pub source: &'a str,
	pub span: (u32, u32),
	pub name: &'a [u8],
	pub nested_spans: &'a [(u32, u32)],
}

pub fn def_fingerprints(scope: FingerprintScope<'_>) -> DefFingerprints {
	let bytes = span_bytes(scope.source, scope.span);
	let holes = relative_holes(scope.span, scope.nested_spans, bytes.len());
	let text = masked_hash(bytes, scope.name, &holes);
	let body = match find_identifier(bytes, scope.name) {
		Some(at) if at > 0 => {
			let shifted: Vec<(usize, usize)> = holes
				.iter()
				.filter(|(_, end)| *end > at)
				.map(|(start, end)| (start.saturating_sub(at), end - at))
				.collect();
			masked_hash(&bytes[at..], scope.name, &shifted)
		}
		_ => text,
	};
	DefFingerprints { text, body }
}

fn relative_holes(span: (u32, u32), nested: &[(u32, u32)], len: usize) -> Vec<(usize, usize)> {
	let mut holes: Vec<(usize, usize)> = nested
		.iter()
		.filter(|inner| **inner != span && inner.0 >= span.0 && inner.1 <= span.1)
		.map(|inner| {
			let start = ((inner.0 - span.0) as usize).min(len);
			let end = ((inner.1 - span.0) as usize).min(len);
			(start, end)
		})
		.collect();
	holes.sort_unstable();
	holes
}

fn span_bytes(source: &str, span: (u32, u32)) -> &[u8] {
	let bytes = source.as_bytes();
	let start = (span.0 as usize).min(bytes.len());
	let end = (span.1 as usize).clamp(start, bytes.len());
	&bytes[start..end]
}

fn masked_hash(bytes: &[u8], mask: &[u8], holes: &[(usize, usize)]) -> u64 {
	let mut hasher = FxHasher::default();
	let mut pending_space = false;
	let mut emitted = false;
	let mut cursor = 0;
	let mut next_hole = 0;
	while cursor < bytes.len() {
		while next_hole < holes.len() && holes[next_hole].1 <= cursor {
			next_hole += 1;
		}
		if next_hole < holes.len() && holes[next_hole].0 <= cursor {
			cursor = holes[next_hole].1;
			next_hole += 1;
			pending_space = emitted;
			continue;
		}
		if matches_identifier_at(bytes, cursor, mask) {
			if pending_space {
				hasher.write_u8(b' ');
				pending_space = false;
			}
			hasher.write_u8(0);
			emitted = true;
			cursor += mask.len();
			continue;
		}
		let byte = bytes[cursor];
		cursor += 1;
		if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
			pending_space = emitted;
			continue;
		}
		if pending_space {
			hasher.write_u8(b' ');
			pending_space = false;
		}
		hasher.write_u8(byte);
		emitted = true;
	}
	hasher.finish()
}

fn find_identifier(bytes: &[u8], name: &[u8]) -> Option<usize> {
	if name.is_empty() {
		return None;
	}
	(0..bytes.len().saturating_sub(name.len() - 1))
		.find(|&at| matches_identifier_at(bytes, at, name))
}

fn matches_identifier_at(bytes: &[u8], at: usize, name: &[u8]) -> bool {
	if name.is_empty() || !bytes[at..].starts_with(name) {
		return false;
	}
	let before = at.checked_sub(1).map(|i| bytes[i]);
	let after = bytes.get(at + name.len()).copied();
	!before.is_some_and(is_identifier_byte) && !after.is_some_and(is_identifier_byte)
}

fn is_identifier_byte(byte: u8) -> bool {
	byte.is_ascii_alphanumeric() || byte == b'_'
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::environment;
	use code_moniker_core::lang::Lang;
	use std::path::Path;

	fn prints(source: &str, name: &[u8]) -> DefFingerprints {
		def_fingerprints(FingerprintScope {
			source,
			span: (0, source.len() as u32),
			name,
			nested_spans: &[],
		})
	}

	#[test]
	fn fingerprint_ignores_whitespace_layout() {
		assert_eq!(
			prints("fn f() { let x = 1; }", b"f").text,
			prints("fn f() {\n\tlet x = 1;\n}", b"f").text
		);
		assert_eq!(
			prints("  fn f() {}", b"f").text,
			prints("fn f() {}\n", b"f").text
		);
	}

	#[test]
	fn fingerprint_distinguishes_token_changes() {
		assert_ne!(
			prints("fn f() { let x = 1; }", b"f").text,
			prints("fn f() { let x = 2; }", b"f").text
		);
		assert_ne!(
			prints("let ab = 1;", b"").text,
			prints("let a b = 1;", b"").text
		);
	}

	#[test]
	fn fingerprint_masks_the_own_name_including_recursion() {
		assert_eq!(
			prints("fn old_name(n: u32) { old_name(n - 1); }", b"old_name").text,
			prints("fn fresh(n: u32) { fresh(n - 1); }", b"fresh").text
		);
		assert_ne!(
			prints("fn old_name(n: u32) { other(n); }", b"old_name").text,
			prints("fn fresh(n: u32) { fresh(n); }", b"fresh").text
		);
	}

	#[test]
	fn fingerprint_does_not_mask_identifier_substrings() {
		assert_ne!(prints("fetch();", b"f").text, prints("getch();", b"g").text);
		assert_eq!(
			prints("fn f() { fetch(); }", b"f").text,
			prints("fn g() { fetch(); }", b"g").text
		);
	}

	#[test]
	fn body_fingerprint_starts_at_the_name() {
		let public = prints("pub fn f() { work(); }", b"f");
		let private = prints("fn f() { work(); }", b"f");
		assert_eq!(public.body, private.body);
		assert_ne!(public.text, private.text);
	}

	#[test]
	fn identity_tail_is_stable_across_anchor_paths() {
		let source =
			"struct Holder;\nimpl Holder {\n\tfn touch(&self) {}\n}\nfn free_fn(x: u32) {}\n";
		let left = environment::extract_source(Lang::Rs, source, Path::new("src/original.rs"));
		let right =
			environment::extract_source(Lang::Rs, source, Path::new("moved/deep/renamed.rs"));
		let left_tails: Vec<_> = left
			.defs()
			.map(|def| identity_tail(&def.moniker, left.root()).expect("tail under root"))
			.collect();
		let right_tails: Vec<_> = right
			.defs()
			.map(|def| identity_tail(&def.moniker, right.root()).expect("tail under root"))
			.collect();

		assert_eq!(left_tails, right_tails);
		assert!(
			left_tails.iter().any(|tail| tail
				.last()
				.is_some_and(|seg| seg.name.starts_with(b"free_fn"))),
			"expected a free_fn tail: {left_tails:?}"
		);
	}

	#[test]
	fn identity_tail_rejects_foreign_roots() {
		let source = "fn lone() {}\n";
		let graph = environment::extract_source(Lang::Rs, source, Path::new("src/a.rs"));
		let other = environment::extract_source(Lang::Rs, source, Path::new("src/b.rs"));
		let def = graph.defs().next().expect("def");

		assert!(identity_tail(&def.moniker, other.root()).is_none());
	}

	#[test]
	fn tail_parent_drops_the_final_segment() {
		let source = "struct Holder;\nimpl Holder {\n\tfn touch(&self) {}\n}\n";
		let graph = environment::extract_source(Lang::Rs, source, Path::new("src/a.rs"));
		let method = graph
			.defs()
			.find(|def| crate::code::def_kind(def) == "method")
			.expect("method def");
		let tail = identity_tail(&method.moniker, graph.root()).expect("tail");
		let parent = tail.parent();

		assert_eq!(tail.segments().len(), parent.segments().len() + 1);
		assert!(
			parent
				.last()
				.is_some_and(|seg| seg.name.as_slice() == b"Holder"),
			"parent tail should end at the impl target: {parent:?}"
		);
	}

	#[test]
	fn tail_rewrite_prefix_moves_children_under_the_new_container() {
		let source = "struct Holder;\nimpl Holder {\n\tfn touch(&self) {}\n}\nstruct Keeper;\nimpl Keeper {\n\tfn touch(&self) {}\n}\n";
		let graph = environment::extract_source(Lang::Rs, source, Path::new("src/a.rs"));
		let tails: Vec<_> = graph
			.defs()
			.filter(|def| crate::code::def_kind(def) == "method")
			.map(|def| identity_tail(&def.moniker, graph.root()).expect("tail"))
			.collect();
		let [holder_touch, keeper_touch] = tails.as_slice() else {
			panic!("expected two methods: {tails:?}");
		};

		let rewritten = holder_touch
			.rewrite_prefix(&holder_touch.parent(), &keeper_touch.parent())
			.expect("prefix applies");

		assert_eq!(&rewritten, keeper_touch);
		assert!(
			keeper_touch
				.parent()
				.rewrite_prefix(holder_touch, keeper_touch)
				.is_none(),
			"non-prefix rewrite must not apply"
		);
	}

	#[test]
	fn split_callable_name_separates_params() {
		assert_eq!(
			split_callable_name(b"findById(id:int,label:String)"),
			(
				b"findById".as_slice(),
				Some(b"(id:int,label:String)".as_slice())
			)
		);
		assert_eq!(split_callable_name(b"Holder"), (b"Holder".as_slice(), None));
	}
}
