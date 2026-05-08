use super::{Moniker, MonikerBuilder};

impl Moniker {
	pub fn is_ancestor_of(&self, other: &Moniker) -> bool {
		self.as_view().is_ancestor_of(&other.as_view())
	}

	pub fn parent(&self) -> Option<Moniker> {
		let view = self.as_view();
		let n = view.segment_count() as usize;
		if n == 0 {
			return None;
		}
		let mut b = MonikerBuilder::from_view(view);
		b.truncate(n - 1);
		Some(b.build())
	}

	pub fn last_kind(&self) -> Option<Vec<u8>> {
		self.as_view().segments().last().map(|s| s.kind.to_vec())
	}

	pub fn bind_match(&self, other: &Moniker) -> bool {
		self.as_view().bind_match(&other.as_view())
	}

	pub fn with_bare_last_segment(&self) -> Moniker {
		let view = self.as_view();
		let segs: Vec<_> = view.segments().collect();
		let Some(last) = segs.last() else {
			return self.clone();
		};
		let bare = bare_callable_name(last.name);
		if bare.len() == last.name.len() {
			return self.clone();
		}
		let mut b = MonikerBuilder::new();
		b.project(view.project());
		for s in &segs[..segs.len() - 1] {
			b.segment(s.kind, s.name);
		}
		b.segment(last.kind, bare);
		b.build()
	}
}

impl<'a> super::MonikerView<'a> {
	pub fn bind_match(&self, other: &super::MonikerView<'_>) -> bool {
		if self.project() != other.project() {
			return false;
		}
		let mut ls = self.segments();
		let mut rs = other.segments();
		let mut prev_l = match ls.next() {
			Some(s) => s,
			None => return false,
		};
		let mut prev_r = match rs.next() {
			Some(s) => s,
			None => return false,
		};
		loop {
			match (ls.next(), rs.next()) {
				(None, None) => return last_segment_match(self, other, prev_l.name, prev_r.name),
				(Some(_), None) | (None, Some(_)) => return false,
				(Some(nl), Some(nr)) => {
					if prev_l != prev_r {
						return false;
					}
					prev_l = nl;
					prev_r = nr;
				}
			}
		}
	}

	pub fn lang_segment(&self) -> Option<&[u8]> {
		self.segments().find(|s| s.kind == b"lang").map(|s| s.name)
	}
}

fn last_segment_match(
	left: &super::MonikerView<'_>,
	right: &super::MonikerView<'_>,
	l_name: &[u8],
	r_name: &[u8],
) -> bool {
	if l_name == r_name {
		return true;
	}
	let l_lang = left.lang_segment();
	let r_lang = right.lang_segment();
	if let Some(lang) = l_lang
		&& l_lang == r_lang
	{
		#[allow(clippy::match_same_arms)]
		match lang {
			b"sql" => return bare_callable_name(l_name) == bare_callable_name(r_name),
			b"ts" => return bare_callable_name(l_name) == bare_callable_name(r_name),
			b"java" => return bare_callable_name(l_name) == bare_callable_name(r_name),
			b"python" => return bare_callable_name(l_name) == bare_callable_name(r_name),
			b"rs" => return bare_callable_name(l_name) == bare_callable_name(r_name),
			_ => {}
		}
	}
	false
}

pub fn bare_callable_name(name: &[u8]) -> &[u8] {
	match name.iter().position(|b| *b == b'(') {
		Some(i) => &name[..i],
		None => name,
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn mk(project: &[u8], segs: &[(&[u8], &[u8])]) -> Moniker {
		let mut b = MonikerBuilder::new();
		b.project(project);
		for (k, name) in segs {
			b.segment(k, name);
		}
		b.build()
	}

	#[test]
	fn ancestor_is_reflexive() {
		let m = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		assert!(m.is_ancestor_of(&m));
	}

	#[test]
	fn ancestor_of_strict_prefix() {
		let parent = mk(b"app", &[(b"path", b"a")]);
		let child = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		assert!(parent.is_ancestor_of(&child));
		assert!(!child.is_ancestor_of(&parent));
	}

	#[test]
	fn ancestor_rejects_different_project() {
		let a = mk(b"app1", &[(b"path", b"x")]);
		let b = mk(b"app2", &[(b"path", b"x"), (b"path", b"y")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn ancestor_rejects_diverging_segment() {
		let a = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		let b = mk(b"app", &[(b"path", b"a"), (b"path", b"c")]);
		assert!(!a.is_ancestor_of(&b));
	}

	#[test]
	fn parent_drops_last_segment() {
		let m = mk(b"app", &[(b"path", b"a"), (b"path", b"b")]);
		let p = m.parent().unwrap();
		let expected = mk(b"app", &[(b"path", b"a")]);
		assert_eq!(p, expected);
	}

	#[test]
	fn parent_of_project_only_is_none() {
		let m = mk(b"app", &[]);
		assert!(m.parent().is_none());
	}

	#[test]
	fn parent_of_one_segment_is_project_only() {
		let m = mk(b"app", &[(b"path", b"a")]);
		let p = m.parent().unwrap();
		assert_eq!(p.as_view().segment_count(), 0);
		assert_eq!(p.as_view().project(), b"app");
	}

	#[test]
	fn last_kind_returns_kind_of_last_segment() {
		let m = mk(b"app", &[(b"path", b"a"), (b"class", b"Foo")]);
		assert_eq!(m.last_kind(), Some(b"class".to_vec()));
	}

	#[test]
	fn last_kind_is_none_for_project_only() {
		let m = mk(b"app", &[]);
		assert!(m.last_kind().is_none());
	}

	#[test]
	fn byte_lex_is_tree_friendly() {
		let m1 = mk(b"app", &[(b"class", b"Foo")]);
		let descendant = mk(b"app", &[(b"class", b"Foo"), (b"method", b"bar()")]);
		let deeper = mk(
			b"app",
			&[(b"class", b"Foo"), (b"method", b"bar()"), (b"path", b"x")],
		);
		let sibling = mk(b"app", &[(b"class", b"Zoo")]);

		assert!(m1.as_bytes() < descendant.as_bytes());
		assert!(descendant.as_bytes() < deeper.as_bytes());
		assert!(deeper.as_bytes() < sibling.as_bytes());
	}

	#[test]
	fn descendant_with_longer_name_stays_inside_parent_range() {
		let parent = mk(b"app", &[(b"class", b"Foo")]);
		let child_long = mk(
			b"app",
			&[
				(b"class", b"Foo"),
				(b"method", b"bar_longer_than_anything()"),
			],
		);
		let next_sibling = mk(b"app", &[(b"class", b"Zoo")]);

		assert!(parent.as_bytes() < child_long.as_bytes());
		assert!(child_long.as_bytes() < next_sibling.as_bytes());
	}

	#[test]
	fn bind_match_equal_monikers_match() {
		let m = mk(b"app", &[(b"class", b"Foo")]);
		assert!(m.bind_match(&m));
	}

	#[test]
	fn bind_match_path_vs_class_on_last_segment_matches() {
		let import = mk(b"app", &[(b"module", b"util"), (b"path", b"Foo")]);
		let def = mk(b"app", &[(b"module", b"util"), (b"class", b"Foo")]);
		assert!(import.bind_match(&def));
		assert!(def.bind_match(&import));
	}

	#[test]
	fn bind_match_rejects_different_projects() {
		let l = mk(b"app1", &[(b"class", b"Foo")]);
		let r = mk(b"app2", &[(b"class", b"Foo")]);
		assert!(!l.bind_match(&r));
	}

	#[test]
	fn bind_match_rejects_different_lang_segment() {
		let l = mk(b"app", &[(b"lang", b"python"), (b"class", b"Foo")]);
		let r = mk(b"app", &[(b"lang", b"java"), (b"class", b"Foo")]);
		assert!(!l.bind_match(&r));
	}

	#[test]
	fn bind_match_rejects_different_parent_segment_kind() {
		let l = mk(b"app", &[(b"package", b"acme"), (b"class", b"Foo")]);
		let r = mk(b"app", &[(b"path", b"acme"), (b"class", b"Foo")]);
		assert!(!l.bind_match(&r));
	}

	#[test]
	fn bind_match_rejects_different_segment_count() {
		let shallow = mk(b"app", &[(b"class", b"Foo")]);
		let deep = mk(b"app", &[(b"class", b"Foo"), (b"method", b"bar()")]);
		assert!(!shallow.bind_match(&deep));
	}

	#[test]
	fn bind_match_rejects_project_only_monikers() {
		let l = mk(b"app", &[]);
		let r = mk(b"app", &[]);
		assert!(!l.bind_match(&r));
	}

	#[test]
	fn bind_match_rejects_different_last_segment_name() {
		let l = mk(b"app", &[(b"class", b"Foo")]);
		let r = mk(b"app", &[(b"class", b"Bar")]);
		assert!(!l.bind_match(&r));
	}

	#[test]
	fn bind_match_sql_arity_call_matches_typed_def() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"schema", b"esac"),
				(b"module", b"plan"),
				(b"function", b"create_plan(uuid,text)"),
			],
		);
		let arity_call = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"schema", b"esac"),
				(b"module", b"plan"),
				(b"function", b"create_plan(2)"),
			],
		);
		assert!(arity_call.bind_match(&typed_def));
		assert!(typed_def.bind_match(&arity_call));
	}

	#[test]
	fn bind_match_sql_bare_name_matches_typed_def() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"module", b"plan"),
				(b"function", b"create_plan(uuid,text)"),
			],
		);
		let bare_call = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"module", b"plan"),
				(b"function", b"create_plan"),
			],
		);
		assert!(bare_call.bind_match(&typed_def));
	}

	#[test]
	fn bind_match_sql_different_bare_names_do_not_match() {
		let a = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"module", b"plan"),
				(b"function", b"create_plan(uuid)"),
			],
		);
		let b = mk(
			b"app",
			&[
				(b"lang", b"sql"),
				(b"module", b"plan"),
				(b"function", b"drop_plan(uuid)"),
			],
		);
		assert!(!a.bind_match(&b));
	}

	#[test]
	fn bind_match_ts_typed_def_matches_arity_call() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"foo(number)"),
			],
		);
		let arity_call = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"foo(1)"),
			],
		);
		assert!(typed_def.bind_match(&arity_call));
		assert!(arity_call.bind_match(&typed_def));
	}

	#[test]
	fn bind_match_ts_typed_def_matches_bare_read() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"foo(number,number)"),
			],
		);
		let bare_read = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"foo()"),
			],
		);
		assert!(typed_def.bind_match(&bare_read));
	}

	#[test]
	fn bind_match_ts_distinct_bare_names_do_not_match() {
		let foo = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"foo(number)"),
			],
		);
		let bar = mk(
			b"app",
			&[
				(b"lang", b"ts"),
				(b"module", b"util"),
				(b"function", b"bar(1)"),
			],
		);
		assert!(!foo.bind_match(&bar));
	}

	#[test]
	fn bind_match_java_typed_def_matches_arity_call() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"java"),
				(b"package", b"acme"),
				(b"class", b"Plan"),
				(b"method", b"create(int)"),
			],
		);
		let arity_call = mk(
			b"app",
			&[
				(b"lang", b"java"),
				(b"package", b"acme"),
				(b"class", b"Plan"),
				(b"method", b"create(1)"),
			],
		);
		assert!(typed_def.bind_match(&arity_call));
	}

	#[test]
	fn bind_match_java_typed_def_matches_bare_read() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"java"),
				(b"package", b"acme"),
				(b"class", b"Plan"),
				(b"method", b"create(int)"),
			],
		);
		let bare = mk(
			b"app",
			&[
				(b"lang", b"java"),
				(b"package", b"acme"),
				(b"class", b"Plan"),
				(b"method", b"create"),
			],
		);
		assert!(typed_def.bind_match(&bare));
	}

	#[test]
	fn bind_match_python_typed_def_matches_arity_call() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"python"),
				(b"module", b"m"),
				(b"function", b"f(int)"),
			],
		);
		let arity_call = mk(
			b"app",
			&[
				(b"lang", b"python"),
				(b"module", b"m"),
				(b"function", b"f(1)"),
			],
		);
		assert!(typed_def.bind_match(&arity_call));
	}

	#[test]
	fn bind_match_rust_typed_def_matches_arity_call() {
		let typed_def = mk(
			b"app",
			&[
				(b"lang", b"rs"),
				(b"module", b"util"),
				(b"function", b"add(i32,i32)"),
			],
		);
		let arity_call = mk(
			b"app",
			&[
				(b"lang", b"rs"),
				(b"module", b"util"),
				(b"function", b"add(2)"),
			],
		);
		assert!(typed_def.bind_match(&arity_call));
	}
}
