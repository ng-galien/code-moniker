use super::*;

#[test]
fn workspace_selector_rejects_ambiguous_basenames() {
	let temp = tempfile::tempdir().expect("tempdir");
	let first = temp.path().join("a").join("same");
	let second = temp.path().join("b").join("same");
	fs::create_dir_all(&first).expect("first");
	fs::create_dir_all(&second).expect("second");
	let roots = canonical_workspace_roots([&first, &second]).expect("roots");
	let error = selected_roots(&roots, Some("same")).expect_err("ambiguous selector");
	assert_eq!(error.code, "workspace_selector_ambiguous");
}

#[test]
fn audit_excerpt_is_line_scoped_and_bounded() {
	let source = "one\n  receiver.call(\n    argument,\n  )\nfive\n";
	let excerpt = bounded_source_excerpt(source, (2, 4));

	assert_eq!(excerpt, "receiver.call( argument, )");
	assert!(excerpt.len() <= 240);
}

#[test]
fn source_root_uses_declared_workspace_root() {
	let temp = tempfile::tempdir().expect("tempdir");
	let parent = temp.path().join("workspace");
	let child = parent.join("child");
	fs::create_dir_all(child.join("src")).expect("child src");
	let roots = canonical_workspace_roots([&parent, &child]).expect("roots");
	let canonical_child = child.canonicalize().expect("canonical child");
	let source_owned_by_parent = SourceFileRecord {
		id: SourceId::at(0),
		uri: String::new(),
		source_root: 0,
		path: canonical_child.join("src/lib.rs").display().to_string(),
		rel_path: "child/src/lib.rs".to_string(),
		anchor: String::new(),
		language: "rs".to_string(),
		text: String::new(),
	};
	let selected = roots.iter().collect::<Vec<_>>();
	let root = source_root(&roots, &selected, &source_owned_by_parent).expect("source root");
	assert_eq!(root, &roots[0]);

	let source_owned_by_child = SourceFileRecord {
		source_root: 1,
		..source_owned_by_parent
	};
	let root = source_root(&roots, &selected, &source_owned_by_child).expect("source root");
	assert_eq!(root, &canonical_child);
}

#[test]
fn page_rows_rejects_cursor_from_another_generation() {
	let page = Page {
		cursor: Some(QueryCursor::new(1, Some(WorkspaceGeneration(1)))),
		limit: 1,
	};
	let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
		.expect_err("generation mismatch");
	assert_eq!(error.code, "cursor_generation_mismatch");
}

#[test]
fn page_rows_rejects_offset_only_cursor_for_generated_snapshot() {
	let page = Page {
		cursor: Some(QueryCursor::new(1, None)),
		limit: 1,
	};
	let error = page_rows(vec![1, 2, 3], page, Some(WorkspaceGeneration(2)))
		.expect_err("missing generation");
	assert_eq!(error.code, "cursor_generation_mismatch");
}
