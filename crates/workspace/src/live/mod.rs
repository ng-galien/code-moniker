mod model;
mod roots;
mod watcher;

pub use model::{WorkspaceLiveEvent, WorkspaceLiveRefreshPlan, WorkspaceWatchRoot};
#[cfg(test)]
pub(crate) use roots::WorkspaceEventClassifier;
pub(crate) use roots::watch_roots_for_paths;
pub use watcher::LiveWorkspaceWatcher;

#[cfg(test)]
mod tests {
	use std::path::PathBuf;
	use std::sync::mpsc;
	use std::time::Duration;

	use super::{
		LiveWorkspaceWatcher, WorkspaceEventClassifier, WorkspaceLiveEvent, WorkspaceWatchRoot,
		watch_roots_for_paths,
	};

	#[test]
	fn watcher_publishes_source_changes() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let source = temp.path().join("src").join("lib.rs");
		std::fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
		std::fs::write(&source, "pub fn before() {}\n").expect("seed source");
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start(
			watch_roots_for_paths(&[temp.path().to_path_buf()], None),
			move |event| {
				let _ = tx.send(event);
			},
		)
		.expect("watcher starts");
		std::thread::sleep(Duration::from_millis(200));

		std::fs::write(&source, "pub fn before() {}\npub fn after() {}\n").expect("modify source");

		let event = rx
			.recv_timeout(Duration::from_secs(3))
			.expect("source change event");
		assert!(
			matches!(
				event,
				WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
			),
			"unexpected event: {event:?}"
		);
	}

	#[test]
	fn watcher_publishes_atomic_source_replaces() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let source = temp.path().join("src").join("lib.rs");
		std::fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
		std::fs::write(&source, "pub fn before() {}\n").expect("seed source");
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start(
			watch_roots_for_paths(&[temp.path().to_path_buf()], None),
			move |event| {
				let _ = tx.send(event);
			},
		)
		.expect("watcher starts");
		std::thread::sleep(Duration::from_millis(200));

		let replacement = source.with_extension("rs.tmp");
		std::fs::write(&replacement, "pub fn before() {}\npub fn after() {}\n")
			.expect("write replacement");
		std::fs::rename(&replacement, &source).expect("replace source");

		let event = rx
			.recv_timeout(Duration::from_secs(3))
			.expect("source replace event");
		assert!(
			matches!(
				event,
				WorkspaceLiveEvent::SourcesChanged(_) | WorkspaceLiveEvent::RescanRequired
			),
			"unexpected event: {event:?}"
		);
	}

	#[test]
	fn classifies_source_changes_with_changed_paths() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: Some(PathBuf::from("/repo/.code-moniker/notes.toml")),
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/src/lib.rs")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/src/lib.rs"
			)]))
		);
	}

	#[test]
	fn ignores_non_language_files_under_source_root() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/README.md")], true),
			None
		);
		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
					.add_path(PathBuf::from("/repo/README.md"))
			),
			None
		);
	}

	#[test]
	fn classifies_manifest_changes_as_live_path_refresh() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier
				.classify_paths_with_git_signals(&[PathBuf::from("/repo/package.json")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/package.json"
			)]))
		);
	}

	#[test]
	fn classifies_source_create_remove_as_rescan_required() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::File))
					.add_path(PathBuf::from("/repo/src/new.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::File))
					.add_path(PathBuf::from("/repo/src/old.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_source_rename_as_rescan_required() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Name(
					notify::event::RenameMode::Both,
				)))
				.add_path(PathBuf::from("/repo/src/old.rs"))
				.add_path(PathBuf::from("/repo/src/new.rs"))
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_missing_source_modify_as_rescan_required() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let missing = temp.path().join("src").join("deleted.rs");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Modify(notify::event::ModifyKind::Data(
					notify::event::DataChange::Content,
				)))
				.add_path(missing)
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn classifies_source_directory_changes_as_rescan_required() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let src = temp.path().join("src");
		std::fs::create_dir_all(&src).expect("src dir");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[src], true),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn coalesces_source_with_notes_and_git_base_without_dropping_signals() {
		assert_eq!(
			WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from("/repo/src/lib.rs")])
				.coalesce(WorkspaceLiveEvent::Notes),
			WorkspaceLiveEvent::SourcesAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
		);
		assert_eq!(
			WorkspaceLiveEvent::SourcesAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
				.coalesce(WorkspaceLiveEvent::GitBaseChanged),
			WorkspaceLiveEvent::SourcesGitBaseAndNotes(vec![PathBuf::from("/repo/src/lib.rs")])
		);
	}

	#[test]
	fn classifies_atomic_notes_writes_as_notes_refresh() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: Some(PathBuf::from("/repo/.code-moniker/notes.toml")),
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.code-moniker/notes.toml.tmp")],
				true,
			),
			Some(WorkspaceLiveEvent::Notes)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.code-moniker/notes.toml")],
				false,
			),
			Some(WorkspaceLiveEvent::Notes)
		);
	}

	#[test]
	fn classifies_git_refs_as_git_base_changes() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: Some(PathBuf::from("/repo")),
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/.git/HEAD")], true),
			Some(WorkspaceLiveEvent::GitBaseChanged)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.git/refs/heads/main")],
				true,
			),
			Some(WorkspaceLiveEvent::GitBaseChanged)
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/.git/index")], true),
			None
		);
	}

	#[test]
	fn coalesces_notes_and_git_base_without_dropping_either() {
		assert_eq!(
			WorkspaceLiveEvent::GitBaseChanged.coalesce(WorkspaceLiveEvent::Notes),
			WorkspaceLiveEvent::GitBaseAndNotes
		);
		assert_eq!(
			WorkspaceLiveEvent::GitBaseAndNotes.coalesce(WorkspaceLiveEvent::RescanRequired),
			WorkspaceLiveEvent::RescanGitBaseAndNotes
		);
	}

	#[test]
	fn respects_gitignore_in_live_classifier() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root_path = temp.path().to_path_buf();

		std::fs::write(root_path.join(".gitignore"), ".metals/\n*.log\n").expect("write gitignore");

		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			std::slice::from_ref(&root_path),
			None,
		));

		assert_eq!(
			classifier
				.classify_paths_with_git_signals(&[root_path.join(".metals/metals.log")], true),
			None
		);
		assert_eq!(
			classifier.classify_paths_with_git_signals(&[root_path.join("build.log")], true),
			None
		);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[root_path.join("src/lib.rs")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![
				root_path.join("src/lib.rs")
			]))
		);
	}

	#[test]
	fn anchors_nested_gitignore_patterns_to_their_directory() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root_path = temp.path().to_path_buf();

		std::fs::write(root_path.join(".gitignore"), "*.log\n").expect("write root gitignore");
		std::fs::create_dir_all(root_path.join("nested")).expect("nested dir");
		std::fs::write(root_path.join("nested/.gitignore"), "/keep.rs\n")
			.expect("write nested gitignore");

		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			std::slice::from_ref(&root_path),
			None,
		));

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[root_path.join("nested/keep.rs")], true),
			None
		);

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[root_path.join("keep.rs")], true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![
				root_path.join("keep.rs")
			]))
		);
	}
}
