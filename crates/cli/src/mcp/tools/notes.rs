// code-moniker: ignore-file[smell-clone-reflex]
// MCP notes responses are owned DTO projections from workspace note state.
use code_moniker_query::{
	NoteDto, NoteResolutionDto, NotesAction as QueryNotesAction, NotesQuery, NotesResult, Query,
	QueryRequest, QueryResult,
};
use code_moniker_workspace::notes::{NoteAuthor, NoteKind, NoteStatus};
use serde_json::{Value, json};

use super::scope::{
	Paging, append_call_bool_arg, append_call_cursor_arg, append_call_number_arg,
	append_call_string_arg,
};
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
	let mut query_request = QueryRequest::new(Query::Notes(notes_query(request)));
	query_request.page = request.paging.daemon_page();
	let response = context.query(query_request)?;
	match response.result {
		QueryResult::Notes(result) => Ok(render_notes_result(
			context.scheme(),
			request,
			&result,
			response.next_cursor.as_ref(),
		)),
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

fn render_notes_result(
	scheme: &str,
	request: &NoteRequest,
	result: &NotesResult,
	next: Option<&code_moniker_query::QueryCursor>,
) -> String {
	let mut output = String::new();
	if let Some(id) = request
		.id
		.as_ref()
		.filter(|_| request.action != NoteAction::List)
	{
		output.push_str(&format!("uri: {scheme}workspace/notes/{id}\n"));
	} else {
		output.push_str(&format!("uri: {scheme}workspace/notes\n"));
	}
	if let Some(next) = next {
		output.push_str(&format!(
			"completeness: partial (notes next cursor {})\n",
			next.offset
		));
	} else {
		output.push_str("completeness: full\n");
	}
	output.push_str(&format!("action: {}\n", result.action));
	output.push_str(&format!("notes: {}\n\n", result.total));
	if let Some(deleted) = &result.deleted {
		output.push_str("deleted:\n");
		render_note_entry(&mut output, deleted);
		return output;
	}
	if request.action == NoteAction::List {
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
	} else {
		output.push_str("note:\n");
	}
	if result.rows.is_empty() {
		output.push_str("  <empty>\n");
	} else {
		for note in &result.rows {
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
		append_call_cursor_arg(&mut output, "cursor", next);
		output.push('\n');
	}
	output
}

fn render_note_entry(output: &mut String, note: &NoteDto) {
	output.push_str(&format!("  - id: {}\n", note.id));
	output.push_str(&format!("    kind: {}\n", note.kind));
	output.push_str(&format!("    status: {}\n", note.status));
	output.push_str(&format!("    created_by: {}\n", note.created_by));
	output.push_str(&format!("    title: {}\n", note.title));
	output.push_str(&format!("    moniker: {}\n", note.moniker));
	match &note.resolution {
		NoteResolutionDto::Resolved {
			target,
			file,
			slice,
		} => {
			output.push_str("    resolution: resolved\n");
			output.push_str(&format!("    target: {target}\n"));
			if let Some((start, end)) = slice {
				output.push_str(&format!("    file: {file}:{start}-{end}\n"));
			} else {
				output.push_str(&format!("    file: {file}\n"));
			}
		}
		NoteResolutionDto::Orphan => {
			output.push_str("    resolution: orphan\n");
		}
	}
	output.push_str(&format!("    updated_at: {}\n", note.updated_at));
	if !note.body.is_empty() {
		output.push_str("    body:\n");
		for line in note.body.lines() {
			output.push_str("      ");
			output.push_str(line);
			output.push('\n');
		}
	}
	output.push_str("    commands:\n");
	output.push_str(&format!(
		"      get: code_moniker_notes action=\"get\" id=\"{}\"\n",
		note.id
	));
	output.push_str(&format!(
		"      transition: code_moniker_notes action=\"transition\" id=\"{}\" status=\"ongoing\"\n",
		note.id
	));
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
