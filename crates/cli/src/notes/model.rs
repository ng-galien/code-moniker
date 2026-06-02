use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct NoteId(pub(crate) String);

impl NoteId {
	pub(crate) fn new(value: impl Into<String>) -> Self {
		Self(value.into())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoteKind {
	Note,
	Todo,
	Gotcha,
	Request,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoteStatus {
	Pending,
	Ongoing,
	Done,
}

impl NoteStatus {
	pub(crate) fn can_transition_to(self, target: Self) -> bool {
		matches!(
			(self, target),
			(Self::Pending, Self::Ongoing)
				| (Self::Pending, Self::Done)
				| (Self::Ongoing, Self::Pending)
				| (Self::Ongoing, Self::Done)
				| (Self::Done, Self::Ongoing)
		) || self == target
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NoteAuthor {
	User,
	Agent,
}

#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Note {
	pub(crate) id: NoteId,
	pub(crate) moniker: String,
	pub(crate) kind: NoteKind,
	pub(crate) status: NoteStatus,
	pub(crate) title: String,
	pub(crate) body: String,
	pub(crate) created_by: NoteAuthor,
	pub(crate) created_at: String,
	pub(crate) updated_at: String,
}

impl Note {
	pub(crate) fn transition_to(
		&mut self,
		target: NoteStatus,
		updated_at: impl Into<String>,
	) -> Result<(), TransitionError> {
		if !self.status.can_transition_to(target) {
			return Err(TransitionError {
				from: self.status,
				to: target,
			});
		}
		self.status = target;
		self.updated_at = updated_at.into();
		Ok(())
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct TransitionError {
	pub(crate) from: NoteStatus,
	pub(crate) to: NoteStatus,
}

impl std::fmt::Display for TransitionError {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"invalid note status transition: {:?} -> {:?}",
			self.from, self.to
		)
	}
}

impl std::error::Error for TransitionError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ResolvedNote {
	pub(crate) note: Note,
	pub(crate) resolution: NoteResolution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum NoteResolution {
	Resolved {
		target_label: String,
		target_file: String,
		target_slice: Option<(u32, u32)>,
	},
	Orphan,
}

impl NoteResolution {
	pub(crate) fn is_orphan(&self) -> bool {
		matches!(self, Self::Orphan)
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn status_transitions_are_controlled() {
		assert!(NoteStatus::Pending.can_transition_to(NoteStatus::Ongoing));
		assert!(NoteStatus::Pending.can_transition_to(NoteStatus::Done));
		assert!(NoteStatus::Ongoing.can_transition_to(NoteStatus::Pending));
		assert!(NoteStatus::Ongoing.can_transition_to(NoteStatus::Done));
		assert!(NoteStatus::Done.can_transition_to(NoteStatus::Ongoing));
		assert!(!NoteStatus::Done.can_transition_to(NoteStatus::Pending));
	}

	#[test]
	fn note_transition_rejects_done_to_pending() {
		let mut note = Note {
			id: NoteId::new("note_1"),
			moniker: "code+moniker://./lang:rs/module:example".to_string(),
			kind: NoteKind::Todo,
			status: NoteStatus::Done,
			title: "title".to_string(),
			body: "body".to_string(),
			created_by: NoteAuthor::User,
			created_at: "2026-06-02T00:00:00Z".to_string(),
			updated_at: "2026-06-02T00:00:00Z".to_string(),
		};

		let error = note
			.transition_to(NoteStatus::Pending, "2026-06-02T01:00:00Z")
			.unwrap_err();

		assert_eq!(error.from, NoteStatus::Done);
		assert_eq!(error.to, NoteStatus::Pending);
		assert_eq!(note.status, NoteStatus::Done);
		assert_eq!(note.updated_at, "2026-06-02T00:00:00Z");
	}

	#[test]
	fn note_transition_updates_status_and_timestamp() {
		let mut note = Note {
			id: NoteId::new("note_1"),
			moniker: "code+moniker://./lang:rs/module:example".to_string(),
			kind: NoteKind::Todo,
			status: NoteStatus::Pending,
			title: "title".to_string(),
			body: "body".to_string(),
			created_by: NoteAuthor::User,
			created_at: "2026-06-02T00:00:00Z".to_string(),
			updated_at: "2026-06-02T00:00:00Z".to_string(),
		};

		note.transition_to(NoteStatus::Ongoing, "2026-06-02T01:00:00Z")
			.expect("transition");

		assert_eq!(note.status, NoteStatus::Ongoing);
		assert_eq!(note.updated_at, "2026-06-02T01:00:00Z");
	}
}
