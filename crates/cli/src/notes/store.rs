use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::model::{Note, NoteKind, NoteStatus};

pub(crate) const NOTES_DIR: &str = ".code-moniker";
pub(crate) const NOTES_FILE: &str = ".code-moniker/notes.toml";

#[derive(Clone, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NotesDocument {
	#[serde(default)]
	pub(crate) notes: Vec<Note>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(crate) struct NoteChanges {
	pub(crate) moniker: Option<String>,
	pub(crate) kind: Option<NoteKind>,
	pub(crate) title: Option<String>,
	pub(crate) body: Option<String>,
}

impl NotesDocument {
	pub(crate) fn notes(&self) -> &[Note] {
		&self.notes
	}

	pub(crate) fn get(&self, id: &str) -> Option<&Note> {
		self.notes.iter().find(|note| note.id.as_str() == id)
	}

	pub(crate) fn insert(&mut self, note: Note) -> anyhow::Result<()> {
		if self.notes.iter().any(|item| item.id == note.id) {
			anyhow::bail!("note id `{}` already exists", note.id.0);
		}
		self.notes.push(note);
		self.sort_notes();
		Ok(())
	}

	pub(crate) fn update(
		&mut self,
		id: &str,
		changes: NoteChanges,
		updated_at: impl Into<String>,
	) -> anyhow::Result<&Note> {
		let note = self
			.notes
			.iter_mut()
			.find(|note| note.id.as_str() == id)
			.ok_or_else(|| anyhow::anyhow!("note id `{id}` does not exist"))?;
		if let Some(moniker) = changes.moniker {
			note.moniker = moniker;
		}
		if let Some(kind) = changes.kind {
			note.kind = kind;
		}
		if let Some(title) = changes.title {
			note.title = title;
		}
		if let Some(body) = changes.body {
			note.body = body;
		}
		note.updated_at = updated_at.into();
		self.sort_notes();
		self.get(id)
			.ok_or_else(|| anyhow::anyhow!("note id `{id}` does not exist after update"))
	}

	pub(crate) fn transition(
		&mut self,
		id: &str,
		status: NoteStatus,
		updated_at: impl Into<String>,
	) -> anyhow::Result<&Note> {
		let note = self
			.notes
			.iter_mut()
			.find(|note| note.id.as_str() == id)
			.ok_or_else(|| anyhow::anyhow!("note id `{id}` does not exist"))?;
		note.transition_to(status, updated_at)?;
		self.sort_notes();
		self.get(id)
			.ok_or_else(|| anyhow::anyhow!("note id `{id}` does not exist after transition"))
	}

	pub(crate) fn delete(&mut self, id: &str) -> anyhow::Result<Note> {
		let Some(index) = self.notes.iter().position(|note| note.id.as_str() == id) else {
			anyhow::bail!("note id `{id}` does not exist");
		};
		Ok(self.notes.remove(index))
	}

	fn sort_notes(&mut self) {
		self.notes.sort_by(|left, right| {
			left.status
				.cmp(&right.status)
				.then_with(|| left.updated_at.cmp(&right.updated_at).reverse())
				.then_with(|| left.id.cmp(&right.id))
		});
	}
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NotesStore {
	path: PathBuf,
	loaded_modified: Option<SystemTime>,
	document: NotesDocument,
}

impl NotesStore {
	pub(crate) fn load(root: &Path) -> anyhow::Result<Self> {
		let path = notes_path(root);
		let (document, loaded_modified) = if path.exists() {
			let loaded_modified = std::fs::metadata(&path)?.modified().ok();
			let text = std::fs::read_to_string(&path)?;
			(toml::from_str(&text)?, loaded_modified)
		} else {
			(NotesDocument::default(), None)
		};
		Ok(Self {
			path,
			loaded_modified,
			document,
		})
	}

	pub(crate) fn path(&self) -> &Path {
		&self.path
	}

	pub(crate) fn document(&self) -> &NotesDocument {
		&self.document
	}

	pub(crate) fn document_mut(&mut self) -> &mut NotesDocument {
		&mut self.document
	}

	pub(crate) fn notes(&self) -> &[Note] {
		self.document.notes()
	}

	pub(crate) fn insert(&mut self, note: Note) -> anyhow::Result<()> {
		self.document.insert(note)
	}

	pub(crate) fn save(&mut self) -> anyhow::Result<()> {
		if let Some(parent) = self.path.parent() {
			std::fs::create_dir_all(parent)?;
		}
		let _lock = self.acquire_save_lock()?;
		self.ensure_not_stale()?;
		let text = toml::to_string_pretty(&self.document)?;
		let tmp = self.temp_path();
		std::fs::write(&tmp, text)?;
		std::fs::rename(&tmp, &self.path)?;
		self.loaded_modified = std::fs::metadata(&self.path)?.modified().ok();
		Ok(())
	}

	fn acquire_save_lock(&self) -> anyhow::Result<SaveLock> {
		let path = self.lock_path();
		for _ in 0..50 {
			match std::fs::OpenOptions::new()
				.write(true)
				.create_new(true)
				.open(&path)
			{
				Ok(_file) => return Ok(SaveLock { path }),
				Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
					std::thread::sleep(Duration::from_millis(10));
				}
				Err(error) => return Err(error.into()),
			}
		}
		anyhow::bail!(
			"notes file is locked by another writer: {}",
			self.path.display()
		);
	}

	fn ensure_not_stale(&self) -> anyhow::Result<()> {
		let current = match std::fs::metadata(&self.path) {
			Ok(metadata) => metadata.modified().ok(),
			Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
			Err(error) => return Err(error.into()),
		};
		if current != self.loaded_modified {
			anyhow::bail!(
				"notes file changed since load: reload `{}` before saving",
				self.path.display()
			);
		}
		Ok(())
	}

	fn temp_path(&self) -> PathBuf {
		let nonce = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		self.path
			.with_extension(format!("toml.{}.{}.tmp", std::process::id(), nonce))
	}

	fn lock_path(&self) -> PathBuf {
		self.path.with_extension("toml.lock")
	}
}

struct SaveLock {
	path: PathBuf,
}

impl Drop for SaveLock {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(&self.path);
	}
}

pub(crate) fn notes_path(root: &Path) -> PathBuf {
	root.join(NOTES_FILE)
}

pub(crate) fn notes_dir(root: &Path) -> PathBuf {
	root.join(NOTES_DIR)
}

pub(crate) fn notes_root_for_paths(paths: &[PathBuf]) -> anyhow::Result<PathBuf> {
	let roots = paths
		.iter()
		.map(|path| root_for_path(path.as_path()))
		.collect::<Vec<_>>();
	let Some(first) = roots.first() else {
		anyhow::bail!("notes require at least one workspace root");
	};
	let mut common = first.clone();
	for root in roots.iter().skip(1) {
		while !root.starts_with(&common) {
			if !common.pop() {
				anyhow::bail!("cannot find common root for notes");
			}
		}
	}
	Ok(common)
}

fn root_for_path(path: &Path) -> PathBuf {
	if path.is_dir() {
		path.to_path_buf()
	} else {
		path.parent()
			.unwrap_or_else(|| Path::new("."))
			.to_path_buf()
	}
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::notes::model::{NoteAuthor, NoteId, NoteKind, NoteStatus};

	#[test]
	fn missing_notes_file_loads_empty_document() {
		let temp = tempfile::tempdir().expect("tempdir");

		let store = NotesStore::load(temp.path()).expect("load notes");

		assert!(store.notes().is_empty());
		assert_eq!(store.path(), &notes_path(temp.path()));
	}

	#[test]
	fn save_creates_notes_file_and_round_trips() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut store = NotesStore::load(temp.path()).expect("load notes");
		store.insert(sample_note("note_1")).expect("insert note");

		store.save().expect("save notes");
		let loaded = NotesStore::load(temp.path()).expect("reload notes");

		assert_eq!(loaded.notes(), store.notes());
		assert!(notes_dir(temp.path()).is_dir());
	}

	#[test]
	fn duplicate_note_ids_are_rejected() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut store = NotesStore::load(temp.path()).expect("load notes");
		store.insert(sample_note("note_1")).expect("insert note");

		let error = store.insert(sample_note("note_1")).unwrap_err();

		assert!(format!("{error:#}").contains("already exists"));
	}

	#[test]
	fn stale_save_is_rejected() {
		let temp = tempfile::tempdir().expect("tempdir");
		let mut first = NotesStore::load(temp.path()).expect("load first");
		first.insert(sample_note("note_1")).expect("insert first");
		first.save().expect("save first");
		let mut stale = NotesStore::load(temp.path()).expect("load stale");
		let mut fresh = NotesStore::load(temp.path()).expect("load fresh");
		fresh.insert(sample_note("note_2")).expect("insert fresh");
		fresh.save().expect("save fresh");
		stale.insert(sample_note("note_3")).expect("insert stale");

		let error = stale.save().unwrap_err();

		assert!(format!("{error:#}").contains("changed since load"));
	}

	#[test]
	fn invalid_note_kind_is_rejected() {
		let temp = tempfile::tempdir().expect("tempdir");
		std::fs::create_dir_all(notes_dir(temp.path())).expect("mkdir");
		std::fs::write(
			notes_path(temp.path()),
			r#"
			[[notes]]
			id = "note_1"
			moniker = "code+moniker://./lang:rs/module:example"
			kind = "bug"
			status = "pending"
			title = "Bad kind"
			body = "Body"
			created_by = "user"
			created_at = "2026-06-02T00:00:00Z"
			updated_at = "2026-06-02T00:00:00Z"
			"#,
		)
		.expect("write notes");

		let error = NotesStore::load(temp.path()).unwrap_err();

		assert!(format!("{error:#}").contains("unknown variant"));
	}

	fn sample_note(id: &str) -> Note {
		Note {
			id: NoteId::new(id),
			moniker: "code+moniker://./lang:rs/module:example".to_string(),
			kind: NoteKind::Todo,
			status: NoteStatus::Pending,
			title: "Title".to_string(),
			body: "Body".to_string(),
			created_by: NoteAuthor::User,
			created_at: "2026-06-02T00:00:00Z".to_string(),
			updated_at: "2026-06-02T00:00:00Z".to_string(),
		}
	}
}
