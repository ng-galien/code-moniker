// code-moniker: ignore-file[smell-clone-reflex]
// MCP notes responses are owned DTO projections from workspace note state.
use code_moniker_query::{
	NoteDto, NoteResolutionDto, NotesAction as QueryNotesAction, NotesQuery, NotesResult, Query,
	QueryResult,
};
use code_moniker_workspace::notes::{NoteAuthor, NoteKind, NoteStatus};
use serde::Serialize;
use serde_json::{Value, json};

use super::common::{AgentOutputOptions, OutputBudget};
use super::scope::{
	Paging, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg,
};
use super::{McpTool, OutputContract, OutputOptions, ToolDescriptor, ToolError, ToolResult};
use crate::mcp::context::McpContext;
use crate::presentation::notes as notes_presentation;

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
					"description": "Target compact moniker, canonical URI, or symbol id. Required for create, optional for list and update."
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

	fn output_contract(&self) -> OutputContract {
		OutputContract::Agent
	}

	fn call(
		&self,
		context: &McpContext,
		arguments: &Value,
		output: OutputOptions,
	) -> Result<ToolResult, ToolError> {
		let request = NoteRequest::from_arguments(arguments, output.agent_options())
			.map_err(ToolError::failed)?;
		run_notes(context, &request).map_err(ToolError::failed)
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
	output: AgentOutputOptions,
}

impl NoteRequest {
	fn from_arguments(arguments: &Value, output: AgentOutputOptions) -> anyhow::Result<Self> {
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
			paging: Paging::from_arguments_for_volume(arguments, output)?,
			output,
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

fn run_notes(context: &McpContext, request: &NoteRequest) -> anyhow::Result<ToolResult> {
	ensure_notes_uri(&request.uri, context.scheme())?;
	let response = context.query_refreshed(
		Query::Notes(notes_query(request)),
		request.paging.daemon_page(),
	)?;
	match response.result {
		QueryResult::Notes(result) => notes_view(
			context.scheme(),
			request,
			&result,
			response.next_cursor.as_ref(),
		)
		.and_then(|view| notes_presentation::mcp(&view))
		.map(ToolResult::templated),
		other => anyhow::bail!("unexpected daemon notes result: {other:?}"),
	}
}

fn notes_query(request: &NoteRequest) -> NotesQuery {
	NotesQuery {
		action: request.action.into(),
		id: request.id.clone(),
		moniker: request.moniker.clone(),
		kind: request.kind.map(|kind| kind.as_str().to_string()),
		status: request.status.map(|status| status.as_str().to_string()),
		title: request.title.clone(),
		body: request.body.clone(),
		created_by: Some(request.created_by.as_str().to_string()),
		orphan: request.orphan,
		include_done: request.include_done,
	}
}

impl From<NoteAction> for QueryNotesAction {
	fn from(action: NoteAction) -> Self {
		match action {
			NoteAction::List => Self::List,
			NoteAction::Get => Self::Get,
			NoteAction::Create => Self::Create,
			NoteAction::Update => Self::Update,
			NoteAction::Transition => Self::Transition,
			NoteAction::Delete => Self::Delete,
		}
	}
}

#[derive(Serialize)]
struct McpNotesView<'a> {
	uri: String,
	partial: bool,
	next_cursor: Option<usize>,
	action: &'a str,
	total: usize,
	volume: &'static str,
	scope: Option<McpNotesScopeView<'a>>,
	deleted: bool,
	notes: Vec<McpNoteView<'a>>,
	next_call: Option<String>,
}

#[derive(Serialize)]
struct McpNotesScopeView<'a> {
	moniker: Option<&'a str>,
	orphan: Option<bool>,
	include_done: bool,
}

#[derive(Serialize)]
struct McpNoteView<'a> {
	id: &'a str,
	moniker: &'a str,
	kind: &'a str,
	status: &'a str,
	title: &'a str,
	body: &'a str,
	created_by: &'a str,
	updated_at: &'a str,
	resolution: &'static str,
	target: Option<&'a str>,
	file: Option<&'a str>,
	slice: Option<(u32, u32)>,
	commands: Vec<String>,
}

fn notes_view<'a>(
	scheme: &str,
	request: &'a NoteRequest,
	result: &'a NotesResult,
	next: Option<&code_moniker_query::QueryCursor>,
) -> anyhow::Result<McpNotesView<'a>> {
	let uri = if let Some(id) = request
		.id
		.as_ref()
		.filter(|_| request.action != NoteAction::List)
	{
		format!("{scheme}workspace/notes/{id}")
	} else {
		format!("{scheme}workspace/notes")
	};
	let (deleted, notes) = if let Some(note) = result.deleted.as_ref() {
		(true, vec![note_view(note)])
	} else {
		(false, result.rows.iter().map(note_view).collect())
	};
	Ok(McpNotesView {
		uri,
		partial: next.is_some(),
		next_cursor: next.map(|cursor| cursor.offset),
		action: &result.action,
		total: result.total,
		volume: request.output.budget.as_str(),
		scope: (request.action == NoteAction::List).then_some(McpNotesScopeView {
			moniker: request.moniker.as_deref(),
			orphan: request.orphan,
			include_done: request.include_done,
		}),
		deleted,
		notes,
		next_call: next.map(|cursor| notes_next_call(request, cursor)),
	})
}

fn note_view(note: &NoteDto) -> McpNoteView<'_> {
	let (resolution, target, file, slice) = match &note.resolution {
		NoteResolutionDto::Resolved {
			target,
			file,
			slice,
		} => (
			"resolved",
			Some(target.as_str()),
			Some(file.as_str()),
			*slice,
		),
		NoteResolutionDto::Orphan => ("orphan", None, None, None),
	};
	McpNoteView {
		id: &note.id,
		moniker: &note.moniker,
		kind: &note.kind,
		status: &note.status,
		title: &note.title,
		body: &note.body,
		created_by: &note.created_by,
		updated_at: &note.updated_at,
		resolution,
		target,
		file,
		slice,
		commands: note_commands(note),
	}
}

fn note_commands(note: &NoteDto) -> Vec<String> {
	let mut get = "code_moniker_notes".to_string();
	append_call_string_arg(&mut get, "action", "get");
	append_call_string_arg(&mut get, "id", &note.id);
	let mut transition = "code_moniker_notes".to_string();
	append_call_string_arg(&mut transition, "action", "transition");
	append_call_string_arg(&mut transition, "id", &note.id);
	append_call_string_arg(&mut transition, "status", "ongoing");
	vec![get, transition]
}

fn notes_next_call(request: &NoteRequest, cursor: &code_moniker_query::QueryCursor) -> String {
	let mut arguments = String::new();
	append_call_string_arg(&mut arguments, "action", "list");
	if let Some(moniker) = &request.moniker {
		append_call_string_arg(&mut arguments, "moniker", moniker);
	}
	if let Some(orphan) = request.orphan {
		append_call_bool_arg(&mut arguments, "orphan", orphan);
	}
	if request.include_done {
		append_call_bool_arg(&mut arguments, "include_done", true);
	}
	append_call_number_arg(&mut arguments, "limit", request.paging.limit);
	append_call_cursor_arg(&mut arguments, "cursor", cursor);
	if request.output.budget != OutputBudget::Small {
		append_call_string_arg(&mut arguments, "budget", request.output.budget.as_str());
	}
	if !request.output.compact {
		append_call_bool_arg(&mut arguments, "compact", false);
	}
	arguments
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
