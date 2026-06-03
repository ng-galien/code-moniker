mod model;
mod resolve;
mod store;

pub use model::{
	Note, NoteAuthor, NoteId, NoteKind, NoteResolution, NoteStatus, ResolvedNote, TransitionError,
};
pub use resolve::resolve_notes;
pub use store::{
	NoteChanges, NotesDocument, NotesStore, NotesWatchTarget, WorkspaceNotes, notes_dir,
	notes_path, notes_root_for_paths, notes_watch_targets_for_paths,
};
