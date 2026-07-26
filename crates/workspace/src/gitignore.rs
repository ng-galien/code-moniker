use std::path::Path;

use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

/// The `.gitignore`/`.ignore` rules in effect under a root directory, kept as
/// one matcher per directory and evaluated deepest-first so a nested rule wins
/// over a shallower one (standard gitignore precedence).
#[derive(Clone, Debug)]
pub struct GitignoreStack {
	layers: Vec<Gitignore>,
}

impl GitignoreStack {
	pub fn for_root(root: &Path) -> Self {
		let mut layers = Vec::new();
		collect_layers(root, &mut layers);
		layers.sort_by_key(|layer| std::cmp::Reverse(layer.path().components().count()));
		Self { layers }
	}

	pub fn for_path(root: &Path, path: &Path) -> Self {
		let mut layers = Vec::new();
		push_layer(root, &mut layers);
		let parent = path.parent().unwrap_or(path);
		if let Ok(relative) = parent.strip_prefix(root) {
			let mut directory = root.to_path_buf();
			for component in relative.components() {
				directory.push(component.as_os_str());
				push_layer(&directory, &mut layers);
			}
		}
		layers.sort_by_key(|layer| std::cmp::Reverse(layer.path().components().count()));
		Self { layers }
	}

	pub fn is_ignored(&self, path: &Path, is_dir: bool) -> bool {
		for layer in &self.layers {
			if !path.starts_with(layer.path()) {
				continue;
			}
			match layer.matched_path_or_any_parents(path, is_dir) {
				Match::Ignore(_) => return true,
				Match::Whitelist(_) => return false,
				Match::None => {}
			}
		}
		false
	}
}

fn collect_layers(dir: &Path, out: &mut Vec<Gitignore>) {
	push_layer(dir, out);
	if let Ok(entries) = std::fs::read_dir(dir) {
		for entry in entries.flatten() {
			let path = entry.path();
			let name = path.file_name().and_then(|n| n.to_str());
			if path.is_dir() && !name.is_some_and(is_ignored_dir_name) {
				collect_layers(&path, out);
			}
		}
	}
}

fn push_layer(dir: &Path, out: &mut Vec<Gitignore>) {
	let mut builder = GitignoreBuilder::new(dir);
	let mut has_rules = false;
	for name in [".gitignore", ".ignore"] {
		let path = dir.join(name);
		if path.is_file() {
			let _ = builder.add(&path);
			has_rules = true;
		}
	}
	if has_rules {
		if let Ok(gitignore) = builder.build() {
			out.push(gitignore);
		}
	}
}

/// Directory names never worth descending into when collecting ignore rules or
/// classifying live events: VCS, build, and cache dirs that are always ignored.
pub fn is_ignored_dir_name(name: &str) -> bool {
	matches!(
		name,
		".code-moniker-cache" | ".git" | ".gradle" | "target" | "node_modules" | "build" | "dist"
	)
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn nested_anchored_pattern_stays_in_its_directory() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::write(root.join(".gitignore"), "*.log\n").expect("root gitignore");
		std::fs::create_dir_all(root.join("nested")).expect("nested dir");
		std::fs::write(root.join("nested/.gitignore"), "/keep.rs\n").expect("nested gitignore");

		let rules = GitignoreStack::for_root(root);

		assert!(rules.is_ignored(&root.join("nested/keep.rs"), false));
		assert!(!rules.is_ignored(&root.join("keep.rs"), false));
		assert!(rules.is_ignored(&root.join("server.log"), false));
	}

	#[test]
	fn nested_whitelist_overrides_shallower_ignore() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::write(root.join(".gitignore"), "*.rs\n").expect("root gitignore");
		std::fs::create_dir_all(root.join("keep")).expect("keep dir");
		std::fs::write(root.join("keep/.gitignore"), "!*.rs\n").expect("nested gitignore");

		let rules = GitignoreStack::for_root(root);

		assert!(rules.is_ignored(&root.join("top.rs"), false));
		assert!(!rules.is_ignored(&root.join("keep/lib.rs"), false));
	}

	#[test]
	fn path_stack_loads_only_the_candidate_ancestor_chain() {
		let temp = tempfile::tempdir_in(env!("CARGO_MANIFEST_DIR")).expect("temp workspace");
		let root = temp.path();
		std::fs::write(root.join(".gitignore"), "*.rs\n").expect("root gitignore");
		std::fs::create_dir_all(root.join("keep/deep")).expect("candidate dirs");
		std::fs::write(root.join("keep/.gitignore"), "!*.rs\n").expect("nested gitignore");
		std::fs::create_dir_all(root.join("unrelated")).expect("unrelated dir");
		std::fs::write(root.join("unrelated/.ignore"), "*.txt\n").expect("unrelated ignore");

		let candidate = root.join("keep/deep/lib.rs");
		let rules = GitignoreStack::for_path(root, &candidate);

		assert!(!rules.is_ignored(&candidate, false));
		assert_eq!(rules.layers.len(), 2);
	}
}
