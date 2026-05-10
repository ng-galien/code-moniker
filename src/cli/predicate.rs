use crate::core::code_graph::{CodeGraph, DefRecord, RefRecord};
use crate::core::moniker::Moniker;

#[derive(Clone, Debug)]
pub enum Predicate {
	Eq(Moniker),
	Lt(Moniker),
	Le(Moniker),
	Gt(Moniker),
	Ge(Moniker),
	AncestorOf(Moniker),
	DescendantOf(Moniker),
	Bind(Moniker),
}

impl Predicate {
	pub fn matches(&self, m: &Moniker) -> bool {
		match self {
			Self::Eq(o) => m == o,
			Self::Lt(o) => m < o,
			Self::Le(o) => m <= o,
			Self::Gt(o) => m > o,
			Self::Ge(o) => m >= o,
			Self::AncestorOf(o) => m.is_ancestor_of(o),
			Self::DescendantOf(o) => o.is_ancestor_of(m),
			Self::Bind(o) => m.bind_match(o),
		}
	}
}

#[derive(Debug, Default)]
pub struct MatchSet<'g> {
	pub defs: Vec<&'g DefRecord>,
	pub refs: Vec<&'g RefRecord>,
}

/// A def matches when its own moniker satisfies every predicate; a ref matches
/// when its **target** moniker does. `kinds` is OR-combined; empty means any.
pub fn filter<'g>(
	graph: &'g CodeGraph,
	predicates: &[Predicate],
	kinds: &[String],
) -> MatchSet<'g> {
	let kinds_set: Vec<&[u8]> = kinds.iter().map(|s| s.as_bytes()).collect();
	let kind_ok = |k: &[u8]| -> bool { kinds_set.is_empty() || kinds_set.contains(&k) };
	let mut defs: Vec<&DefRecord> = graph
		.defs()
		.filter(|d| kind_ok(&d.kind) && predicates.iter().all(|p| p.matches(&d.moniker)))
		.collect();
	let refs: Vec<&RefRecord> = graph
		.refs()
		.filter(|r| kind_ok(&r.kind) && predicates.iter().all(|p| p.matches(&r.target)))
		.collect();
	defs.sort_by(|a, b| a.moniker.as_bytes().cmp(b.moniker.as_bytes()));
	let mut keyed: Vec<(&RefRecord, &[u8])> = refs
		.into_iter()
		.map(|r| (r, graph.def_at(r.source).moniker.as_bytes()))
		.collect();
	keyed.sort_by(|(a, a_src), (b, b_src)| {
		(a_src, a.target.as_bytes(), a.position).cmp(&(b_src, b.target.as_bytes(), b.position))
	});
	let refs = keyed.into_iter().map(|(r, _)| r).collect();
	MatchSet { defs, refs }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::core::moniker::MonikerBuilder;

	fn m(segments: &[(&[u8], &[u8])]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(b"app");
		for (k, n) in segments {
			b.segment(k, n);
		}
		b.build()
	}

	fn build_graph() -> CodeGraph {
		let root = m(&[]);
		let mut g = CodeGraph::new(root.clone(), b"module");
		let foo = m(&[(b"class", b"Foo")]);
		let bar = m(&[(b"class", b"Foo"), (b"method", b"bar")]);
		let baz = m(&[(b"class", b"Baz")]);
		g.add_def(foo.clone(), b"class", &root, Some((1, 0)))
			.unwrap();
		g.add_def(bar, b"method", &foo, Some((2, 2))).unwrap();
		g.add_def(baz.clone(), b"class", &root, Some((10, 0)))
			.unwrap();
		g.add_ref(&baz, foo, b"EXTENDS", Some((10, 14))).unwrap();
		g
	}

	#[test]
	fn no_predicate_matches_everything() {
		let g = build_graph();
		let r = filter(&g, &[], &[]);
		assert_eq!(r.defs.len(), 4);
		assert_eq!(r.refs.len(), 1);
	}

	#[test]
	fn kind_filter_or_combines() {
		let g = build_graph();
		let r = filter(&g, &[], &["method".to_string()]);
		assert_eq!(r.defs.len(), 1);
		assert_eq!(r.defs[0].kind, b"method");
		let r = filter(&g, &[], &["method".to_string(), "module".to_string()]);
		assert_eq!(r.defs.len(), 2);
	}

	#[test]
	fn descendant_of_keeps_only_strict_descendants_and_target() {
		let g = build_graph();
		let foo = m(&[(b"class", b"Foo")]);
		let r = filter(&g, &[Predicate::DescendantOf(foo)], &[]);
		let names: Vec<&[u8]> = r.defs.iter().map(|d| d.kind.as_slice()).collect();
		assert!(names.contains(&b"class".as_slice()));
		assert!(names.contains(&b"method".as_slice()));
		assert_eq!(r.defs.len(), 2);
	}

	#[test]
	fn equality_matches_one_def() {
		let g = build_graph();
		let foo = m(&[(b"class", b"Foo")]);
		let r = filter(&g, &[Predicate::Eq(foo.clone())], &[]);
		assert_eq!(r.defs.len(), 1);
		assert_eq!(&r.defs[0].moniker, &foo);
		assert_eq!(r.refs.len(), 1, "ref to Foo also matches via target");
	}

	#[test]
	fn ordering_predicates_use_byte_lex() {
		let g = build_graph();
		let baz = m(&[(b"class", b"Baz")]);
		let r = filter(&g, &[Predicate::Lt(baz.clone())], &[]);
		assert!(r.defs.iter().all(|d| d.moniker < baz));
		let r = filter(&g, &[Predicate::Ge(baz.clone())], &[]);
		assert!(r.defs.iter().all(|d| d.moniker >= baz));
	}

	#[test]
	fn ancestor_of_includes_self() {
		let g = build_graph();
		let bar = m(&[(b"class", b"Foo"), (b"method", b"bar")]);
		let r = filter(&g, &[Predicate::AncestorOf(bar)], &[]);
		let kinds: Vec<&[u8]> = r.defs.iter().map(|d| d.kind.as_slice()).collect();
		assert!(kinds.contains(&b"module".as_slice()));
		assert!(kinds.contains(&b"class".as_slice()));
		assert!(kinds.contains(&b"method".as_slice()));
	}

	#[test]
	fn predicate_and_kind_compose() {
		let g = build_graph();
		let foo = m(&[(b"class", b"Foo")]);
		let r = filter(&g, &[Predicate::DescendantOf(foo)], &["method".to_string()]);
		assert_eq!(r.defs.len(), 1);
		assert_eq!(r.defs[0].kind, b"method");
	}

	#[test]
	fn ref_filtered_by_target_moniker() {
		let g = build_graph();
		let foo = m(&[(b"class", b"Foo")]);
		let r = filter(&g, &[Predicate::Eq(foo)], &[]);
		assert_eq!(r.refs.len(), 1, "EXTENDS ref targets Foo");
	}

	#[test]
	fn defs_sorted_by_moniker_bytes() {
		let g = build_graph();
		let r = filter(&g, &[], &[]);
		let mut prev: Option<&[u8]> = None;
		for d in &r.defs {
			let cur = d.moniker.as_bytes();
			if let Some(p) = prev {
				assert!(p <= cur, "defs not sorted: {p:?} then {cur:?}");
			}
			prev = Some(cur);
		}
	}
}
