use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use code_moniker_workspace::notes::{
	Note, NoteAuthor, NoteChanges, NoteId, NoteKind, NoteResolution, NoteStatus, NotesDocument,
	resolve_notes,
};
use code_moniker_workspace::snapshot::WorkspaceSnapshot;
use serde_json::{Value, json};

use super::scope::{Paging, append_call_bool_arg, append_call_number_arg, append_call_string_arg};
use super::{McpTool, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;

const DEFAULT_NOTES_URI: &str = "workspace/notes";

pub(super) struct NotesTool;

impl NotesTool {
	pub(super) const NAME: &'static str = "code_moniker_notes";

	const DESCRIPTION: &'static str = concat!(
		"When to use: read or maintain user/agent notes attached to code-moniker symbols. ",
		"Use this before changing a symbol that may carry TODOs, gotchas, or agent requests.\n",
		"\n",
		"Notes from code-moniker.\n",
		"  action=list       — list notes, optionally scoped to one moniker or orphan status\n",
		"  action=get        — read one note by id\n",
		"  action=create     — create a note on a moniker\n",
		"  action=update     — edit note moniker, kind, title, or body\n",
		"  action=transition — move pending/ongoing/done through controlled transitions\n",
		"  action=delete     — delete one note by id\n",
		"Notes are stored in .code-moniker/notes.toml at the MCP workspace root."
	);

	fn input_schema() -> Value {
		json!({
			"type": "object",
			"properties": {
				"action": {
					"type": "string",
					"enum": ["list", "get", "create", "update", "transition", "delete"],
					"description": "Note operation to perform."
				},
				"uri": {
					"type": "string",
					"description": "workspace/notes | code+moniker://workspace/notes"
				},
				"id": {
					"type": "string",
					"description": "Stable note id. Required for get, update, transition, and delete."
				},
				"moniker": {
					"type": "string",
					"description": "Target symbol moniker. Required for create, optional for list and update."
				},
				"kind": {
					"type": "string",
					"enum": ["note", "todo", "gotcha", "request"],
					"description": "Note kind. Defaults to note on create."
				},
				"status": {
					"type": "string",
					"enum": ["pending", "ongoing", "done"],
					"description": "Initial status for create, or target status for transition."
				},
				"title": {
					"type": "string",
					"description": "Short note title."
				},
				"body": {
					"type": "string",
					"description": "Markdown/plain-text note body."
				},
				"created_by": {
					"type": "string",
					"enum": ["user", "agent"],
					"description": "Note author for create. Defaults to agent."
				},
				"orphan": {
					"type": "boolean",
					"description": "For action=list, filter notes whose target moniker no longer resolves."
				},
				"include_done": {
					"type": "boolean",
					"description": "For action=list, include done notes. Defaults false."
				},
				"limit": {
					"type": "integer",
					"minimum": 1,
					"maximum": super::scope::MAX_LIMIT,
					"description": "Maximum notes to emit for list."
				},
				"cursor": {
					"oneOf": [{ "type": "integer" }, { "type": "string" }],
					"description": "Opaque row offset returned in next calls for list."
				}
			},
			"additionalProperties": false
		})
	}
}

impl McpTool for NotesTool {
	fn descriptor(&self) -> ToolDescriptor {
		ToolDescriptor {
			name: Self::NAME,
			description: Self::DESCRIPTION,
			input_schema: Self::input_schema(),
		}
	}

	fn call(&self, context: &McpContext, arguments: &Value) -> Result<ToolResult, ToolError> {
		let request = NoteRequest::from_arguments(arguments).map_err(ToolError::failed)?;
		let text = run_notes(context, &request).map_err(ToolError::failed)?;
		Ok(ToolResult {
			text,
			is_error: false,
		})
	}
}

#[derive(Clone, Debug)]
struct NoteRequest {
	action: NoteAction,
	uri: String,
	id: Option<String>,
	moniker: Option<String>,
	kind: Option<NoteKind>,
	status: Option<NoteStatus>,
	title: Option<String>,
	body: Option<String>,
	created_by: NoteAuthor,
	orphan: Option<bool>,
	include_done: bool,
	paging: Paging,
}

impl NoteRequest {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		Ok(Self {
			action: NoteAction::from_arguments(arguments)?,
			uri: string_argument(arguments, "uri")?
				.unwrap_or_else(|| DEFAULT_NOTES_URI.to_string()),
			id: string_argument(arguments, "id")?,
			moniker: string_argument(arguments, "moniker")?,
			kind: optional_kind(arguments)?,
			status: optional_status(arguments)?,
			title: string_argument(arguments, "title")?,
			body: string_argument(arguments, "body")?,
			created_by: optional_author(arguments)?.unwrap_or(NoteAuthor::Agent),
			orphan: bool_argument(arguments, "orphan")?,
			include_done: bool_argument(arguments, "include_done")?.unwrap_or(false),
			paging: Paging::from_arguments(arguments)?,
		})
	}
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum NoteAction {
	List,
	Get,
	Create,
	Update,
	Transition,
	Delete,
}

impl NoteAction {
	fn from_arguments(arguments: &Value) -> anyhow::Result<Self> {
		match arguments
			.get("action")
			.and_then(Value::as_str)
			.unwrap_or("list")
		{
			"list" => Ok(Self::List),
			"get" => Ok(Self::Get),
			"create" => Ok(Self::Create),
			"update" => Ok(Self::Update),
			"transition" => Ok(Self::Transition),
			"delete" => Ok(Self::Delete),
			action => anyhow::bail!("unknown notes action `{action}`"),
		}
	}
}

fn run_notes(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	ensure_notes_uri(&request.uri, context.scheme())?;
	context.index().reload_notes(&context.opts().paths)?;
	let result = match request.action {
		NoteAction::List => render_note_list(context, request)?,
		NoteAction::Get => render_note_get(context, request)?,
		NoteAction::Create => create_note(context, request)?,
		NoteAction::Update => update_note(context, request)?,
		NoteAction::Transition => transition_note(context, request)?,
		NoteAction::Delete => delete_note(context, request)?,
	};
	Ok(result)
}

fn render_note_list(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	let resolved = resolved_notes(context)?;
	let mut rows = resolved
		.into_iter()
		.filter(|note| {
			request
				.moniker
				.as_ref()
				.is_none_or(|moniker| note.note.moniker == *moniker)
		})
		.filter(|note| request.include_done || note.note.status != NoteStatus::Done)
		.filter(|note| {
			request
				.orphan
				.is_none_or(|orphan| note.resolution.is_orphan() == orphan)
		})
		.collect::<Vec<_>>();
	rows.sort_by(|left, right| {
		left.note
			.status
			.cmp(&right.note.status)
			.then_with(|| left.note.updated_at.cmp(&right.note.updated_at).reverse())
			.then_with(|| left.note.id.cmp(&right.note.id))
	});
	let total = rows.len();
	let (start, end, next) = request.paging.window(&rows);
	let mut output = render_notes_header(context.scheme(), total, next, start, end);
	output.push_str("scope:\n");
	if let Some(moniker) = &request.moniker {
		output.push_str(&format!("  moniker: {moniker}\n"));
	} else {
		output.push_str("  moniker: *\n");
	}
	if let Some(orphan) = request.orphan {
		output.push_str(&format!("  orphan: {orphan}\n"));
	}
	output.push_str(&format!("  include_done: {}\n\n", request.include_done));
	output.push_str("results:\n");
	if start == end {
		output.push_str("  <empty>\n");
	} else {
		for note in &rows[start..end] {
			render_note_entry(&mut output, note);
		}
	}
	if let Some(next) = next {
		output.push_str("\nnext:\n");
		output.push_str("  - code_moniker_notes action=\"list\"");
		if let Some(moniker) = &request.moniker {
			append_call_string_arg(&mut output, "moniker", moniker);
		}
		if let Some(orphan) = request.orphan {
			append_call_bool_arg(&mut output, "orphan", orphan);
		}
		if request.include_done {
			append_call_bool_arg(&mut output, "include_done", true);
		}
		append_call_number_arg(&mut output, "limit", request.paging.limit);
		append_call_number_arg(&mut output, "cursor", next);
		output.push('\n');
	}
	Ok(output)
}

fn render_note_get(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	let id = required_id(request)?;
	let mut rows = resolved_notes(context)?;
	rows.retain(|note| note.note.id.as_str() == id);
	let Some(note) = rows.first() else {
		anyhow::bail!("note id `{id}` does not exist");
	};
	Ok(render_single_note(context.scheme(), "get", note))
}

fn create_note(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	let snapshot = context.index().index_snapshot()?;
	let moniker = required_string(request.moniker.as_deref(), "moniker")?.to_string();
	let title = required_string(request.title.as_deref(), "title")?.to_string();
	let now = current_timestamp();
	let kind = request.kind.unwrap_or(NoteKind::Note);
	let status = request.status.unwrap_or(NoteStatus::Pending);
	let body = request.body.clone().unwrap_or_default();
	let created_by = request.created_by;
	let requested_id = request.id.clone();
	let id = context
		.index()
		.mutate_notes(&context.opts().paths, |document| {
			let id = requested_id
				.as_deref()
				.map(NoteId::new)
				.unwrap_or_else(|| generated_note_id(document));
			document.insert(Note {
				id: id.clone(),
				moniker,
				kind,
				status,
				title,
				body,
				created_by,
				created_at: now.clone(),
				updated_at: now,
			})?;
			Ok(id)
		})?;
	render_changed_note(context, "create", &snapshot, Some(id.as_str()))
}

fn update_note(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	if request.status.is_some() {
		anyhow::bail!("status changes require action=transition");
	}
	let snapshot = context.index().index_snapshot()?;
	let id = required_id(request)?;
	let changes = NoteChanges {
		moniker: request.moniker.clone(),
		kind: request.kind,
		title: request.title.clone(),
		body: request.body.clone(),
	};
	context
		.index()
		.mutate_notes(&context.opts().paths, |document| {
			document.update(id, changes, current_timestamp())?;
			Ok(())
		})?;
	render_changed_note(context, "update", &snapshot, Some(id))
}

fn transition_note(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	let snapshot = context.index().index_snapshot()?;
	let id = required_id(request)?;
	let status = request
		.status
		.ok_or_else(|| anyhow::anyhow!("status is required for action=transition"))?;
	context
		.index()
		.mutate_notes(&context.opts().paths, |document| {
			document.transition(id, status, current_timestamp())?;
			Ok(())
		})?;
	render_changed_note(context, "transition", &snapshot, Some(id))
}

fn delete_note(context: &McpContext, request: &NoteRequest) -> anyhow::Result<String> {
	let id = required_id(request)?;
	let deleted = context
		.index()
		.mutate_notes(&context.opts().paths, |document| document.delete(id))?;
	let mut output = String::new();
	output.push_str(&format!("uri: {}workspace/notes/{id}\n", context.scheme()));
	output.push_str("completeness: full\n");
	output.push_str("action: delete\n");
	output.push_str("deleted:\n");
	output.push_str(&format!("  id: {}\n", deleted.id.as_str()));
	output.push_str(&format!("  moniker: {}\n", deleted.moniker));
	output.push_str(&format!("  title: {}\n", deleted.title));
	Ok(output)
}

fn render_changed_note(
	context: &McpContext,
	action: &str,
	snapshot: &Arc<WorkspaceSnapshot>,
	id: Option<&str>,
) -> anyhow::Result<String> {
	let notes = context.index().notes_snapshot()?;
	let resolved = resolve_notes(notes.notes(), snapshot);
	let note = id
		.and_then(|id| resolved.iter().find(|note| note.note.id.as_str() == id))
		.or_else(|| resolved.first());
	let Some(note) = note else {
		anyhow::bail!("note mutation did not produce a note");
	};
	Ok(render_single_note(context.scheme(), action, note))
}

fn resolved_notes(
	context: &McpContext,
) -> anyhow::Result<Vec<code_moniker_workspace::notes::ResolvedNote>> {
	let snapshot = context.index().index_snapshot()?;
	let notes = context.index().notes_snapshot()?;
	Ok(resolve_notes(notes.notes(), &snapshot))
}

fn render_notes_header(
	scheme: &str,
	total: usize,
	next: Option<usize>,
	start: usize,
	end: usize,
) -> String {
	let mut output = String::new();
	output.push_str(&format!("uri: {scheme}workspace/notes\n"));
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (notes {start}-{end} of {total}, next cursor {next})\n"
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("notes: {total}\n\n"));
	output
}

fn render_single_note(
	scheme: &str,
	action: &str,
	note: &code_moniker_workspace::notes::ResolvedNote,
) -> String {
	let mut output = String::new();
	output.push_str(&format!(
		"uri: {}workspace/notes/{}\n",
		scheme,
		note.note.id.as_str()
	));
	output.push_str("completeness: full\n");
	output.push_str(&format!("action: {action}\n"));
	output.push_str("note:\n");
	render_note_entry(&mut output, note);
	output
}

fn render_note_entry(output: &mut String, note: &code_moniker_workspace::notes::ResolvedNote) {
	output.push_str(&format!("  - id: {}\n", note.note.id.as_str()));
	output.push_str(&format!("    kind: {}\n", note.note.kind.as_str()));
	output.push_str(&format!("    status: {}\n", note.note.status.as_str()));
	output.push_str(&format!(
		"    created_by: {}\n",
		note.note.created_by.as_str()
	));
	output.push_str(&format!("    title: {}\n", note.note.title));
	output.push_str(&format!("    moniker: {}\n", note.note.moniker));
	match &note.resolution {
		NoteResolution::Resolved {
			target_label,
			target_file,
			target_slice,
		} => {
			output.push_str("    resolution: resolved\n");
			output.push_str(&format!("    target: {target_label}\n"));
			if let Some((start, end)) = target_slice {
				output.push_str(&format!("    file: {target_file}:{start}-{end}\n"));
			} else {
				output.push_str(&format!("    file: {target_file}\n"));
			}
		}
		NoteResolution::Orphan => {
			output.push_str("    resolution: orphan\n");
		}
	}
	output.push_str(&format!("    updated_at: {}\n", note.note.updated_at));
	if !note.note.body.is_empty() {
		output.push_str("    body:\n");
		for line in note.note.body.lines() {
			output.push_str("      ");
			output.push_str(line);
			output.push('\n');
		}
	}
	output.push_str("    commands:\n");
	output.push_str(&format!(
		"      get: code_moniker_notes action=\"get\" id=\"{}\"\n",
		note.note.id.as_str()
	));
	output.push_str(&format!(
		"      transition: code_moniker_notes action=\"transition\" id=\"{}\" status=\"ongoing\"\n",
		note.note.id.as_str()
	));
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

fn current_timestamp() -> String {
	let seconds = SystemTime::now()
		.duration_since(UNIX_EPOCH)
		.map(|duration| duration.as_secs())
		.unwrap_or_default();
	format!("unix:{seconds}")
}

fn ensure_notes_uri(uri: &str, scheme: &str) -> anyhow::Result<()> {
	let value = uri.trim();
	if value.is_empty()
		|| value == DEFAULT_NOTES_URI
		|| value == "notes"
		|| value == format!("{scheme}workspace/notes")
	{
		Ok(())
	} else {
		anyhow::bail!(
			"unsupported notes URI; use workspace/notes or {}workspace/notes",
			scheme
		)
	}
}

fn optional_kind(arguments: &Value) -> anyhow::Result<Option<NoteKind>> {
	string_argument(arguments, "kind")?
		.as_deref()
		.map(NoteKind::parse)
		.transpose()
}

fn optional_status(arguments: &Value) -> anyhow::Result<Option<NoteStatus>> {
	string_argument(arguments, "status")?
		.as_deref()
		.map(NoteStatus::parse)
		.transpose()
}

fn optional_author(arguments: &Value) -> anyhow::Result<Option<NoteAuthor>> {
	string_argument(arguments, "created_by")?
		.as_deref()
		.map(NoteAuthor::parse)
		.transpose()
}

fn string_argument(arguments: &Value, key: &str) -> anyhow::Result<Option<String>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	value
		.as_str()
		.map(|value| Some(value.to_string()))
		.ok_or_else(|| anyhow::anyhow!("`{key}` must be a string"))
}

fn bool_argument(arguments: &Value, key: &str) -> anyhow::Result<Option<bool>> {
	let Some(value) = arguments.get(key) else {
		return Ok(None);
	};
	value
		.as_bool()
		.map(Some)
		.ok_or_else(|| anyhow::anyhow!("`{key}` must be a boolean"))
}

fn required_id(request: &NoteRequest) -> anyhow::Result<&str> {
	required_string(request.id.as_deref(), "id")
}

fn required_string<'a>(value: Option<&'a str>, key: &str) -> anyhow::Result<&'a str> {
	value.ok_or_else(|| anyhow::anyhow!("{key} is required"))
}
