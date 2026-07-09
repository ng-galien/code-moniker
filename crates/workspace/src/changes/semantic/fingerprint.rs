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

pub fn body_fingerprint(source: &str, span: (u32, u32)) -> u64 {
	let bytes = source.as_bytes();
	let start = (span.0 as usize).min(bytes.len());
	let end = (span.1 as usize).clamp(start, bytes.len());
	let mut hasher = FxHasher::default();
	let mut pending_space = false;
	let mut emitted = false;
	for byte in &bytes[start..end] {
		if matches!(byte, b' ' | b'\t' | b'\r' | b'\n') {
			pending_space = emitted;
			continue;
		}
		if pending_space {
			hasher.write_u8(b' ');
			pending_space = false;
		}
		hasher.write_u8(*byte);
		emitted = true;
	}
	hasher.finish()
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::environment;
	use code_moniker_core::lang::Lang;
	use std::path::Path;

	fn fingerprint_of(source: &str) -> u64 {
		body_fingerprint(source, (0, source.len() as u32))
	}

	#[test]
	fn fingerprint_ignores_whitespace_layout() {
		assert_eq!(
			fingerprint_of("fn f() { let x = 1; }"),
			fingerprint_of("fn f() {\n\tlet x = 1;\n}")
		);
		assert_eq!(fingerprint_of("  fn f() {}"), fingerprint_of("fn f() {}\n"));
	}

	#[test]
	fn fingerprint_distinguishes_token_changes() {
		assert_ne!(
			fingerprint_of("fn f() { let x = 1; }"),
			fingerprint_of("fn f() { let x = 2; }")
		);
		assert_ne!(
			fingerprint_of("let ab = 1;"),
			fingerprint_of("let a b = 1;")
		);
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
