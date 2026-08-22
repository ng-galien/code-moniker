use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_query::{
	NoteDto, NoteResolutionDto, NotesAction, NotesQuery, NotesResult, Page, QueryError,
	QueryResponse, QueryResult, WorkspaceGeneration,
};
use code_moniker_workspace::notes::{
	Note, NoteAuthor, NoteChanges, NoteId, NoteKind, NoteResolution, NoteStatus, NotesDocument,
	ResolvedNote, resolve_notes,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;

use super::model::NotesResponseInput;
use crate::daemon::WorkspaceDaemon;
use crate::helpers::find_symbol;
use crate::pagination::page_rows;

pub(crate) fn notes_response(
	daemon: &mut WorkspaceDaemon,
	snapshot: &WorkspaceSnapshot,
	mut request: NotesQuery,
	page: Page,
	generation: Option<WorkspaceGeneration>,
) -> Result<QueryResponse, QueryError> {
	if let Some(moniker) = request.moniker.as_deref() {
		match find_symbol(snapshot, snapshot.index.inventory.all_symbols(), moniker) {
			Ok(symbol) => request.moniker = Some(symbol.identity.to_string()),
			Err(error) if error.code == "symbol_not_found" => {}
			Err(error) => return Err(error),
		}
	}
	daemon
		.notes
		.reload(&daemon.roots)
		.map_err(|err| QueryError::new("notes_load_failed", err.to_string()))?;
	let action = request.action;
	let deleted = match action {
		NotesAction::Create => {
			let note = note_from_create(&daemon.notes.snapshot().map_err(note_error)?, &request)?;
			let id = note.id.clone();
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.insert(note)?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(id)
		}
		NotesAction::Update => {
			if request.status.is_some() {
				return Err(QueryError::new(
					"invalid_note_update",
					"status changes require action=transition",
				));
			}
			let id = required_note_id(&request)?;
			let changes = note_changes(&request)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.update(id, changes, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Transition => {
			let id = required_note_id(&request)?;
			let status = request
				.status
				.as_deref()
				.ok_or_else(|| QueryError::new("missing_status", "status is required"))?;
			let status = parse_note_status(status)?;
			daemon
				.notes
				.mutate(&daemon.roots, |document| {
					document.transition(id, status, current_timestamp())?;
					Ok(())
				})
				.map_err(note_error)?;
			Some(NoteId::new(id))
		}
		NotesAction::Delete => {
			let id = required_note_id(&request)?;
			let deleted = daemon
				.notes
				.mutate(&daemon.roots, |document| document.delete(id))
				.map_err(note_error)?;
			return notes_query_response(NotesResponseInput {
				snapshot,
				action,
				notes: Vec::new(),
				deleted: Some(deleted),
				orphan: None,
				page,
				generation,
			});
		}
		NotesAction::List | NotesAction::Get => None,
	};
	daemon.notes.reload(&daemon.roots).map_err(note_error)?;
	let document = daemon.notes.snapshot().map_err(note_error)?;
	let mut notes = document.notes;
	if let Some(id) = deleted {
		notes.retain(|note| note.id == id);
	}
	if action == NotesAction::Get {
		let id = required_note_id(&request)?;
		notes.retain(|note| note.id.as_str() == id);
		if notes.is_empty() {
			return Err(QueryError::new(
				"note_not_found",
				format!("note id `{id}` does not exist"),
			));
		}
	}
	if action == NotesAction::List {
		notes = filter_notes(notes, &request);
	}
	notes_query_response(NotesResponseInput {
		snapshot,
		action,
		notes,
		deleted: None,
		orphan: request.orphan,
		page,
		generation,
	})
}

fn note_changes(request: &NotesQuery) -> Result<NoteChanges, QueryError> {
	Ok(NoteChanges {
		moniker: request.moniker.clone(),
		kind: request.kind.as_deref().map(parse_note_kind).transpose()?,
		title: request.title.clone(),
		body: request.body.clone(),
	})
}

fn notes_query_response(input: NotesResponseInput<'_>) -> Result<QueryResponse, QueryError> {
	let mut resolved = resolve_notes(&input.notes, input.snapshot);
	if let Some(orphan) = input.orphan {
		resolved.retain(|note| note.resolution.is_orphan() == orphan);
	}
	let rows = resolved
		.iter()
		.map(note_dto)
		.collect::<Result<Vec<_>, _>>()?;
	let paged = page_rows(rows, input.page, input.generation)?;
	let deleted = input
		.deleted
		.as_ref()
		.map(|note| note_dto_from_note(note, input.snapshot))
		.transpose()?;
	Ok(QueryResponse {
		generation: input.generation,
		result: QueryResult::Notes(NotesResult {
			action: notes_action_label(input.action).to_string(),
			total: paged.total,
			rows: paged.items,
			deleted,
		}),
		next_cursor: paged.next_cursor,
	})
}

fn note_error(error: anyhow::Error) -> QueryError {
	QueryError::new("notes_failed", error.to_string())
}

fn note_from_create(document: &NotesDocument, request: &NotesQuery) -> Result<Note, QueryError> {
	let moniker = required_note_string(request.moniker.as_deref(), "moniker")?.to_string();
	let title = required_note_string(request.title.as_deref(), "title")?.to_string();
	let now = current_timestamp();
	let id = request
		.id
		.as_deref()
		.map(NoteId::new)
		.unwrap_or_else(|| generated_note_id(document));
	Ok(Note {
		id,
		moniker,
		kind: request
			.kind
			.as_deref()
			.map(parse_note_kind)
			.transpose()?
			.unwrap_or(NoteKind::Note),
		status: request
			.status
			.as_deref()
			.map(parse_note_status)
			.transpose()?
			.unwrap_or(NoteStatus::Pending),
		title,
		body: request.body.clone().unwrap_or_default(),
		created_by: request
			.created_by
			.as_deref()
			.map(parse_note_author)
			.transpose()?
			.unwrap_or(NoteAuthor::Agent),
		created_at: now.clone(),
		updated_at: now,
	})
}

fn generated_note_id(document: &NotesDocument) -> NoteId {
	for attempt in 0..1000_u32 {
		let nanos = SystemTime::now()
			.duration_since(UNIX_EPOCH)
			.map(|duration| duration.as_nanos())
			.unwrap_or_default();
		let id = NoteId::new(format!("note_{nanos:x}_{attempt:x}"));
		if document.get(id.as_str()).is_none() {
			return id;
		}
	}
	NoteId::new("note_exhausted")
}

fn required_note_id(request: &NotesQuery) -> Result<&str, QueryError> {
	required_note_string(request.id.as_deref(), "id")
}

fn required_note_string<'a>(value: Option<&'a str>, key: &str) -> Result<&'a str, QueryError> {
	value.ok_or_else(|| QueryError::new(format!("missing_{key}"), format!("{key} is required")))
}

fn parse_note_kind(value: &str) -> Result<NoteKind, QueryError> {
	NoteKind::parse(value).map_err(|err| QueryError::new("invalid_note_kind", err.to_string()))
}

fn parse_note_status(value: &str) -> Result<NoteStatus, QueryError> {
	NoteStatus::parse(value).map_err(|err| QueryError::new("invalid_note_status", err.to_string()))
}

fn parse_note_author(value: &str) -> Result<NoteAuthor, QueryError> {
	NoteAuthor::parse(value).map_err(|err| QueryError::new("invalid_note_author", err.to_string()))
}

fn current_timestamp() -> String {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default();
	format!("unix:{seconds}")
}

fn filter_notes(notes: Vec<Note>, request: &NotesQuery) -> Vec<Note> {
	notes
		.into_iter()
		.filter(|note| {
			request
				.moniker
				.as_ref()
				.is_none_or(|moniker| note.moniker == *moniker)
		})
		.filter(|note| request.include_done || note.status != NoteStatus::Done)
		.collect()
}

fn note_dto(note: &ResolvedNote) -> Result<NoteDto, QueryError> {
	Ok(NoteDto {
		id: note.note.id.as_str().to_string(),
		moniker: note.note.moniker.to_owned(),
		kind: note.note.kind.as_str().to_string(),
		status: note.note.status.as_str().to_string(),
		title: note.note.title.to_owned(),
		body: note.note.body.to_owned(),
		created_by: note.note.created_by.as_str().to_string(),
		updated_at: note.note.updated_at.to_owned(),
		resolution: note_resolution_dto(&note.resolution),
	})
}

fn note_dto_from_note(note: &Note, snapshot: &WorkspaceSnapshot) -> Result<NoteDto, QueryError> {
	let mut resolved = resolve_notes(std::slice::from_ref(note), snapshot);
	let resolved = resolved
		.pop()
		.ok_or_else(|| QueryError::new("note_resolution_failed", "note did not resolve"))?;
	note_dto(&resolved)
}

fn note_resolution_dto(resolution: &NoteResolution) -> NoteResolutionDto {
	match resolution {
		NoteResolution::Resolved {
			target_label,
			target_file,
			target_slice,
		} => NoteResolutionDto::Resolved {
			target: target_label.clone(),
			file: target_file.clone(),
			slice: *target_slice,
		},
		NoteResolution::Orphan => NoteResolutionDto::Orphan,
	}
}

fn notes_action_label(action: NotesAction) -> &'static str {
	match action {
		NotesAction::List => "list",
		NotesAction::Get => "get",
		NotesAction::Create => "create",
		NotesAction::Update => "update",
		NotesAction::Transition => "transition",
		NotesAction::Delete => "delete",
	}
}
