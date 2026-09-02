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

	use super::roots::{git_metadata_dirs, watch_targets_for};
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
		let _watcher = LiveWorkspaceWatcher::start_polling(
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
		let _watcher = LiveWorkspaceWatcher::start_polling(
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
	fn watcher_requests_a_rescan_when_a_directory_is_created() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start_polling(
			watch_roots_for_paths(&[temp.path().to_path_buf()], None),
			move |event| {
				let _ = tx.send(event);
			},
		)
		.expect("watcher starts");
		std::thread::sleep(Duration::from_millis(200));

		std::fs::create_dir(temp.path().join("new-module")).expect("create source directory");

		assert_eq!(
			rx.recv_timeout(Duration::from_secs(3))
				.expect("directory creation event"),
			WorkspaceLiveEvent::RescanRequired
		);
	}

	#[cfg(target_os = "linux")]
	#[test]
	fn linux_native_watcher_delivers_changes_from_ten_roots() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let mut roots = Vec::new();
		let mut sources = Vec::new();
		for index in 0..10 {
			let root = temp.path().join(format!("project-{index}"));
			let source = root.join("src/lib.rs");
			std::fs::create_dir_all(source.parent().expect("source parent")).expect("src dir");
			std::fs::write(&source, "pub fn before() {}\n").expect("seed source");
			roots.push(WorkspaceWatchRoot {
				path: root,
				git_root: None,
				ignored_paths: Vec::new(),
				notes_path: None,
			});
			sources.push(source);
		}
		let (tx, rx) = mpsc::channel();
		let _watcher = LiveWorkspaceWatcher::start(roots, move |event| {
			let _ = tx.send(event);
		})
		.expect("native watcher starts");
		std::thread::sleep(Duration::from_millis(300));

		let expected = [sources[0].clone(), sources[4].clone(), sources[9].clone()];
		for source in &expected {
			std::fs::write(source, "pub fn after() {}\n").expect("modify source");
		}

		let deadline = std::time::Instant::now() + Duration::from_secs(5);
		let mut observed = Vec::new();
		while std::time::Instant::now() < deadline
			&& expected.iter().any(|path| !observed.contains(path))
		{
			let remaining = deadline.saturating_duration_since(std::time::Instant::now());
			let Ok(event) = rx.recv_timeout(remaining) else {
				break;
			};
			if let Some(paths) = event.source_paths() {
				for path in paths {
					if expected.contains(path) && !observed.contains(path) {
						observed.push(path.clone());
					}
				}
			}
		}

		assert_eq!(
			observed.len(),
			expected.len(),
			"observed paths: {observed:?}"
		);
	}

	#[test]
	fn watch_plan_excludes_gitignored_directories_before_registration() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::create_dir_all(root.join(".git/refs/heads")).expect("git refs");
		std::fs::write(root.join(".gitignore"), "/target/\nmodule/generated/\n")
			.expect("gitignore");
		for directory in [
			"src",
			"target/generated",
			"module/src",
			"module/generated/deep",
			"module/target/src",
		] {
			std::fs::create_dir_all(root.join(directory)).expect("fixture directory");
		}

		let targets =
			watch_targets_for(&watch_roots_for_paths(&[root.to_path_buf()], None)).unwrap();
		let paths: Vec<&std::path::Path> =
			targets.iter().map(|target| target.path.as_path()).collect();

		assert!(paths.contains(&root));
		assert!(paths.contains(&root.join("src").as_path()));
		assert!(paths.contains(&root.join("module/src").as_path()));
		assert!(paths.contains(&root.join(".git/refs/heads").as_path()));
		assert!(
			targets
				.iter()
				.all(|target| target.mode == notify::RecursiveMode::NonRecursive),
			"the directory plan must never delegate recursive traversal to notify"
		);
		assert!(
			paths.contains(&root.join("module/target").as_path()),
			"a directory name is not an ignore rule"
		);
		assert!(
			!paths
				.iter()
				.any(|path| path.starts_with(root.join("target")))
		);
		assert!(
			!paths
				.iter()
				.any(|path| path.starts_with(root.join("module/generated")))
		);
	}

	#[test]
	fn gitignore_cannot_reopen_git_metadata_as_source_directories() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::write(root.join(".gitignore"), "!.git/\n").expect("gitignore");
		for directory in [
			".git/objects/pack",
			".git/logs/refs/heads",
			".git/hooks/custom",
			".git/refs/heads",
		] {
			std::fs::create_dir_all(root.join(directory)).expect("git fixture");
		}

		let targets =
			watch_targets_for(&watch_roots_for_paths(&[root.to_path_buf()], None)).unwrap();
		let git_targets = targets
			.iter()
			.filter(|target| target.path.starts_with(root.join(".git")))
			.map(|target| target.path.strip_prefix(root).unwrap().to_path_buf())
			.collect::<Vec<_>>();

		assert!(git_targets.contains(&PathBuf::from(".git")));
		assert!(git_targets.contains(&PathBuf::from(".git/refs")));
		assert!(git_targets.contains(&PathBuf::from(".git/refs/heads")));
		assert!(
			git_targets
				.iter()
				.all(|path| path == std::path::Path::new(".git") || path.starts_with(".git/refs")),
			"only bounded Git control targets may be re-added: {git_targets:?}"
		);
	}

	#[test]
	fn git_metadata_plan_resolves_linked_worktree_private_and_common_dirs() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let main = temp.path().join("main");
		let linked = temp.path().join("linked");
		std::fs::create_dir(&main).expect("main worktree");
		let git = |cwd: &std::path::Path, args: &[&str]| {
			let output = std::process::Command::new("git")
				.arg("-C")
				.arg(cwd)
				.args(args)
				.output()
				.expect("run git");
			assert!(
				output.status.success(),
				"git {args:?}: {}",
				String::from_utf8_lossy(&output.stderr)
			);
		};
		git(&main, &["init"]);
		git(
			&main,
			&["config", "user.email", "code-moniker@example.test"],
		);
		git(&main, &["config", "user.name", "Code Moniker"]);
		std::fs::write(main.join("lib.rs"), "fn main() {}\n").expect("source");
		git(&main, &["add", "."]);
		git(&main, &["commit", "-m", "initial"]);
		let linked_arg = linked.to_string_lossy();
		git(
			&main,
			&["worktree", "add", "-b", "linked-test", &linked_arg],
		);

		let dirs = git_metadata_dirs(&linked).unwrap();

		assert!(dirs.contains(&main.join(".git")));
		assert!(
			dirs.iter()
				.any(|path| path.starts_with(main.join(".git/worktrees")))
		);
	}

	#[test]
	fn linked_worktree_metadata_resolution_fails_closed() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		std::fs::write(temp.path().join(".git"), "gitdir: missing\n").expect("git pointer");

		let error = git_metadata_dirs(temp.path()).expect_err("invalid gitdir must fail");

		assert!(
			error
				.to_string()
				.contains("cannot resolve Git metadata directories")
		);
	}

	#[test]
	fn project_root_excludes_parent_ignore_watch_targets() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let module = temp.path().join("modules/service");
		std::fs::create_dir_all(temp.path().join(".git")).expect("git dir");
		std::fs::create_dir_all(&module).expect("module");
		let roots = watch_roots_for_paths(&[module], None);
		let targets = watch_targets_for(&roots).unwrap();

		assert!(!targets.iter().any(|target| target.path == temp.path()));
		assert!(targets.iter().any(|target| target.path.ends_with(".git")));
	}

	#[test]
	fn multi_root_notes_watch_does_not_promote_the_common_parent_to_a_source_root() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let unrelated = temp.path().join("unrelated/deep");
		std::fs::create_dir_all(&unrelated).expect("unrelated fixture");
		let roots = (0..10)
			.map(|index| {
				let root = temp.path().join(format!("project-{index}"));
				std::fs::create_dir_all(root.join("src")).expect("project fixture");
				root
			})
			.collect::<Vec<_>>();

		let watch_roots = watch_roots_for_paths(&roots, None);
		let targets = watch_targets_for(&watch_roots).unwrap();

		assert_eq!(watch_roots.len(), roots.len());
		assert!(
			watch_roots
				.iter()
				.all(|watch_root| watch_root.path != temp.path())
		);
		assert!(targets.iter().any(|target| target.path == temp.path()));
		assert!(
			!targets
				.iter()
				.any(|target| target.path.starts_with(&unrelated))
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
	fn c_build_context_changes_require_a_full_rescan() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		for path in [
			"/repo/Makefile",
			"/repo/compile_commands.json",
			"/repo/src/main.c",
			"/repo/include/api.h",
			"/repo/generated/model.cpp",
			"/repo/generated/wrapper.hpp",
		] {
			assert_eq!(
				classifier.classify_paths_with_git_signals(&[PathBuf::from(path)], true),
				Some(WorkspaceLiveEvent::RescanRequired),
				"{path} must rebuild C build provenance"
			);
		}

		assert_eq!(
			classifier.classify_paths_with_git_signals(&[PathBuf::from("/repo/src/lib.rs")], true,),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/src/lib.rs"
			)]))
		);
	}

	#[test]
	fn tsconfig_changes_require_a_full_rescan() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		for path in [
			"/repo/tsconfig.json",
			"/repo/tsconfig.app.json",
			"/repo/packages/api/tsconfig.node.json",
		] {
			assert_eq!(
				classifier.classify_paths_with_git_signals(&[PathBuf::from(path)], true),
				Some(WorkspaceLiveEvent::RescanRequired),
				"{path} must rebuild TypeScript SDK profiles",
			);
		}
	}

	#[test]
	fn source_group_config_changes_require_a_full_rescan() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: None,
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		assert_eq!(
			classifier.classify_paths_with_git_signals(
				&[PathBuf::from("/repo/.code-moniker.toml")],
				true,
			),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn ignore_rule_changes_require_a_full_rescan() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: Some(PathBuf::from("/repo")),
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);

		for path in ["/repo/.gitignore", "/repo/module/.ignore"] {
			assert_eq!(
				classifier.classify_paths_with_git_signals(&[PathBuf::from(path)], true),
				Some(WorkspaceLiveEvent::RescanRequired),
				"{path} must rebuild the watch plan"
			);
		}
	}

	#[test]
	fn classifies_source_create_remove_as_incremental_source_changes() {
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
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/src/new.rs"
			)]))
		);
		assert_eq!(
			classifier.classify_event(
				&notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::File))
					.add_path(PathBuf::from("/repo/src/old.rs"))
			),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![PathBuf::from(
				"/repo/src/old.rs"
			)]))
		);
	}

	#[test]
	fn classifies_source_rename_as_incremental_source_changes() {
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
			Some(WorkspaceLiveEvent::SourcesChanged(vec![
				PathBuf::from("/repo/src/old.rs"),
				PathBuf::from("/repo/src/new.rs"),
			]))
		);
	}

	#[test]
	fn classifies_missing_source_modify_as_incremental_source_changes() {
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
				.add_path(missing.clone())
			),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![missing]))
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
	fn removed_directories_rebuild_the_non_recursive_watch_plan() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		std::fs::create_dir(temp.path().join(".git")).expect("git dir");
		std::fs::write(temp.path().join(".gitignore"), "ignored/\n").expect("gitignore");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		let removed_source =
			notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder))
				.add_path(temp.path().join("module"));
		assert_eq!(
			classifier.classify_event(&removed_source),
			Some(WorkspaceLiveEvent::RescanRequired)
		);

		let removed_ignored =
			notify::Event::new(notify::EventKind::Remove(notify::event::RemoveKind::Folder))
				.add_path(temp.path().join("ignored"));
		assert_eq!(classifier.classify_event(&removed_ignored), None);
	}

	#[test]
	fn renamed_away_directories_rebuild_the_non_recursive_watch_plan() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let module = temp.path().join("module");
		std::fs::create_dir_all(&module).expect("module dir");
		let roots = watch_roots_for_paths(&[temp.path().to_path_buf()], None);
		let targets = watch_targets_for(&roots).unwrap();
		let classifier = WorkspaceEventClassifier::new_with_watch_targets(roots, &targets).unwrap();
		std::fs::remove_dir(&module).expect("remove watched module");

		let renamed = notify::Event::new(notify::EventKind::Modify(
			notify::event::ModifyKind::Name(notify::event::RenameMode::From),
		))
		.add_path(module);
		assert_eq!(
			classifier.classify_event(&renamed),
			Some(WorkspaceLiveEvent::RescanRequired)
		);
	}

	#[test]
	fn renamed_files_do_not_rebuild_the_directory_watch_plan() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::create_dir_all(root.join(".git/refs/heads")).expect("refs");
		let roots = watch_roots_for_paths(&[root.to_path_buf()], None);
		let targets = watch_targets_for(&roots).unwrap();
		let classifier = WorkspaceEventClassifier::new_with_watch_targets(roots, &targets).unwrap();

		let readme = notify::Event::new(notify::EventKind::Modify(
			notify::event::ModifyKind::Name(notify::event::RenameMode::Both),
		))
		.add_path(root.join("README.old"))
		.add_path(root.join("README.md"));
		assert_eq!(classifier.classify_event(&readme), None);

		let git_ref = notify::Event::new(notify::EventKind::Modify(
			notify::event::ModifyKind::Name(notify::event::RenameMode::Both),
		))
		.add_path(root.join(".git/refs/heads/main.lock"))
		.add_path(root.join(".git/refs/heads/main"));
		assert_eq!(
			classifier.classify_event(&git_ref),
			Some(WorkspaceLiveEvent::GitBaseChanged)
		);
	}

	#[test]
	fn created_git_ref_directories_rebuild_the_watch_plan() {
		let classifier = WorkspaceEventClassifier::new(vec![WorkspaceWatchRoot {
			path: PathBuf::from("/repo"),
			git_root: Some(PathBuf::from("/repo")),
			ignored_paths: Vec::new(),
			notes_path: None,
		}]);
		let event =
			notify::Event::new(notify::EventKind::Create(notify::event::CreateKind::Folder))
				.add_path(PathBuf::from("/repo/.git/refs/heads/feature"));

		assert_eq!(
			classifier.classify_event(&event),
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
	fn nested_explicit_root_is_not_rejected_by_an_outer_ignore_rule() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let nested = temp.path().join("nested");
		std::fs::create_dir_all(nested.join("src")).expect("nested source");
		std::fs::write(temp.path().join(".gitignore"), "nested/\n").expect("gitignore");
		let roots = vec![temp.path().to_path_buf(), nested.clone()];
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(&roots, None));
		let source = nested.join("src/lib.rs");

		assert_eq!(
			classifier.classify_paths_with_git_signals(std::slice::from_ref(&source), true),
			Some(WorkspaceLiveEvent::SourcesChanged(vec![source]))
		);
	}

	#[test]
	fn ignore_files_inside_ignored_trees_do_not_rebuild_the_plan() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		std::fs::create_dir_all(temp.path().join("target")).expect("ignored tree");
		std::fs::write(temp.path().join(".gitignore"), "target/\n").expect("gitignore");
		let classifier = WorkspaceEventClassifier::new(watch_roots_for_paths(
			&[temp.path().to_path_buf()],
			None,
		));

		assert_eq!(
			classifier
				.classify_paths_with_git_signals(&[temp.path().join("target/.gitignore")], true,),
			None
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
