# Evolution: TUI and MCP moniker notes

## Context

The TUI is already a navigation surface over monikers, usages, views, checks,
and source snippets. The next agent-facing step is to let a human attach
structured notes to a moniker while navigating, then let an agent discover and
act on those notes through MCP.

This should not be a transient TUI feature. Notes are collaboration data tied
to workspace symbols. They must be persisted, visible from both TUI and MCP,
and robust when the target moniker disappears after a refactor.

The user story is:

- a TUI user can open a note mode from the selected moniker;
- the note editor supports multiline body text;
- notes can be created, edited, deleted, and transitioned;
- monikers with notes have an indicator in the navigator tree;
- the outline panel shows notes before normal symbol details;
- MCP exposes notes to agents;
- agents can create and update notes through MCP;
- notes have status `pending`, `ongoing`, or `done`, with controlled
  transitions;
- abandoning a new empty note creates nothing.

## Data model

Persist notes in a project-local, versionable file. Prefer a directory so the
workspace can later grow more collaboration artifacts without crowding the
root:

```text
.code-moniker/notes.toml
```

Initial persisted shape:

```toml
[[notes]]
id = "note_01J..."
moniker = "code+moniker://./lang:rs/dir:crates/dir:cli/dir:src/module:ui/module:events/enum:Msg"
kind = "todo"     # note | todo | gotcha | request
status = "pending" # pending | ongoing | done
title = "Clarify note-mode key handling"
body = """
The note editor should capture normal text without leaking events into
navigation, and should keep Ctrl/Special keys explicit.
"""
created_by = "user" # user | agent
created_at = "2026-06-02T07:00:00Z"
updated_at = "2026-06-02T07:00:00Z"
```

`kind` and `status` are separate axes. A `gotcha` can be `done` after it has
been handled or documented; a `request` can stay `ongoing` while an agent is
working on it.

Computed fields are not persisted:

- `resolved`: whether the moniker resolves in the current snapshot;
- `orphan`: inverse of `resolved`;
- `target_label`: current symbol label when resolved;
- `target_file`: current source path when resolved;
- `target_slice`: current source line range when resolved.

The store must preserve notes whose moniker no longer resolves. Refactors
should not delete user intent.

## Status transitions

Allowed transitions:

```text
pending -> ongoing
pending -> done
ongoing -> pending
ongoing -> done
done -> ongoing
```

Do not allow direct `done -> pending`. Reopening done work should first move to
`ongoing`, making the intent explicit.

Both TUI and MCP must use the same transition helper. Invalid transitions
return a typed error rather than silently rewriting status.

## Orphan notes

A note is orphaned when its persisted moniker does not resolve in the current
workspace snapshot.

Orphans need first-class visibility:

- they cannot appear under their original tree node because the node no longer
  exists;
- they still represent pending human or agent intent;
- an agent may be able to repair or migrate them.

Navigator indicators:

```text
[!1]   node has at least one pending or ongoing note
[done] node has only done notes
[?1]   notes lens row is orphaned
```

Exact marker text can be tuned for width, but the distinction must remain:
active note, done-only note, orphan note.

The MCP representation should expose:

```text
resolution: resolved | orphan
```

and include the raw moniker for orphan notes.

## Notes lens

Add a dedicated TUI lens that lists notes independently from the source tree.
This is required for orphan notes and useful for triage.

The lens should be reachable from the normal navigation model, like other
feature panels. The exact key is intentionally left open until the keymap is
reviewed.

Summary header:

```text
notes lens
pending  4
ongoing  2
done     8
orphan   1
```

Rows:

```text
pending  todo     Msg key handling             crates/cli/src/ui/events.rs
ongoing  request  orphan                       code+moniker://./lang:rs/...
done     gotcha   render_shell layout split    crates/cli/src/ui/render/view.rs
```

Panel for selected note:

```text
note
kind      todo
status    pending
target    enum Msg
moniker   code+moniker://./lang:rs/...
file      crates/cli/src/ui/events.rs
orphan    no

body
...
```

Ordering:

1. `pending`;
2. `ongoing`;
3. orphan notes not already above, ordered by status then update time;
4. `done`.

Within each group, newest updated notes first.

## Outline integration

The outline panel for a selected moniker should render notes before the normal
symbol details.

Shape:

```text
notes
pending todo  Clarify note-mode key handling
  The note editor should capture normal text...

selected
kind      enum
name      Msg
...
```

If several notes exist on the selected moniker, show active notes first:

1. `pending`;
2. `ongoing`;
3. `done`.

The outline panel is read-oriented. Editing can be invoked from it, but the
editor is a separate TUI mode.

## Navigator tree indicators

The navigation tree read model should carry note summaries by moniker:

- total notes;
- active notes (`pending` or `ongoing`);
- done notes;
- orphan state is not attached to normal tree rows because orphan notes have no
  current tree node.

Tree row rendering should show a compact marker next to rows with notes. This
must be computed before rendering and exposed through view models; renderers
should not query the notes store.

## TUI note editor

Add a `Note` UI mode rather than trying to overload normal mode. Normal key
handling should stop while editing text.

Mode state:

```rust
NoteEditorState {
    note_id: Option<NoteId>,
    target_moniker: String,
    kind: NoteKind,
    status: NoteStatus,
    title: String,
    body: String,
    cursor: TextCursor,
    dirty: bool,
}
```

Behavior:

- opening note mode on a moniker with no note starts a draft;
- opening note mode on a moniker with notes selects the first active note, or
  the newest done note if all are done;
- multiline body input is captured in the panel;
- saving a non-empty draft creates a note;
- saving an existing note updates it;
- leaving a new empty draft creates nothing;
- deleting an existing note requires an explicit delete action.

"Cancel" must not mean "delete an existing note". The safe interpretation is:
closing an unsaved new empty draft abandons it. Existing note deletion is
explicit.

Suggested note-mode keys:

```text
Ctrl+s   save
Esc      close editor
Ctrl+k   cycle kind
Ctrl+o   transition status
Ctrl+p   previous status transition
Ctrl+d   delete existing note, with confirmation
Tab      move field
Shift+Tab previous field
```

These can be tuned after a full keymap review. The important boundary is that
note-mode text input does not leak into normal navigation commands.

## MCP surface

Add a dedicated MCP tool rather than overloading `code_moniker_read`:

```text
code_moniker_notes
```

Actions:

```text
list
get
create
update
transition
delete
```

Example calls:

```json
{ "action": "list", "status": "pending" }
{ "action": "list", "moniker": "code+moniker://./lang:rs/..." }
{ "action": "list", "orphan": true }
{ "action": "get", "id": "note_01J..." }
{
  "action": "create",
  "moniker": "code+moniker://./lang:rs/...",
  "kind": "todo",
  "title": "Clarify resolver behavior",
  "body": "..."
}
{ "action": "transition", "id": "note_01J...", "status": "ongoing" }
{ "action": "update", "id": "note_01J...", "body": "..." }
{ "action": "delete", "id": "note_01J..." }
```

List filters:

- `moniker`;
- `kind`;
- `status`;
- `orphan`;
- `limit`;
- `cursor`;
- `include_done`;
- `updated_since`.

Outputs should follow the existing MCP style:

- `uri` or tool identity;
- `completeness`;
- concise summary counts;
- results;
- `next:` calls preserving filters and cursor.

`code_moniker_read` can later include notes when reading a symbol URI, but
mutating note operations belong only to `code_moniker_notes`.

## Architecture

Introduce a shared note domain in the workspace crate. Notes are collaboration
data attached to workspace symbols, not a CLI-only concern:

```text
crates/workspace/src/notes/
  mod.rs
  model.rs
  store.rs
  resolve.rs
```

Responsibilities:

- `model`: `Note`, `NoteKind`, `NoteStatus`, `NoteId`, transition rules;
- `store`: load/save TOML, atomic write, stable ordering;
- `resolve`: join notes with `WorkspaceSnapshot` to compute resolved/orphan
  state;
- `WorkspaceNotes`: shared note state with reload, snapshot, and mutation
  operations.

Do not make MCP runtime own notes. MCP tools must go through the workspace
state and must not read `.code-moniker/notes.toml` directly. The MCP surface can
request a workspace notes reload before serving an action, then render from the
workspace snapshot.

Do not make the TUI own notes independently. The TUI should consume workspace
read models and refresh its note-aware view models when the workspace reports a
notes change.

Do not make renderers read notes. TUI feature/view-model code should build
note-aware VMs, and ratatui renderers should consume those VMs.

Persistence policy:

- workspace state reads notes from the workspace config root;
- create `.code-moniker/notes.toml` on first save;
- write atomically through a temp file and rename;
- preserve unknown future fields only if the TOML layer can do so cleanly;
  otherwise start with `deny_unknown_fields` and evolve deliberately.

Concurrency:

- TUI and MCP can both write notes. The first implementation can use
  read-modify-write with mtime conflict detection;
- if the file changed since load, reject with a conflict error and ask caller
  to reload;
- later, introduce a small file lock if concurrent writes become common.

Live refresh:

- external changes to `.code-moniker/notes.toml` should refresh workspace notes;
- MCP observes refreshed notes through the workspace state on the next notes
  action;
- TUI live classification emits a notes event and rebuilds note-aware view
  models from the workspace state;
- tests should exercise MCP/TUI public behavior, not require a specific
  internal storage location.

## Implementation stages

### Stage 1: Note model and store

Deliver:

- `Note`, `NoteKind`, `NoteStatus`, `NoteResolution`;
- transition helper and typed transition errors;
- TOML load/save in `.code-moniker/notes.toml`;
- orphan resolution against a `WorkspaceSnapshot`;
- tests for load, save, invalid status/kind, transitions, and orphan
  resolution.

Acceptance:

- loading an empty workspace returns no notes;
- saving creates the notes file;
- invalid transition fails;
- missing moniker resolves as orphan but remains listed.

### Stage 2: MCP notes tool

Deliver:

- `code_moniker_notes` tool registration;
- `list`, `get`, `create`, `update`, `transition`, `delete`;
- filtering and paging;
- `next:` output preserving filters;
- created notes use `created_by = "agent"` when called through MCP unless
  explicitly overridden by a trusted local caller;
- the tool reads and mutates notes through workspace state, not through direct
  file access.

Acceptance:

- agent can create a note on a known moniker;
- list by moniker returns it;
- transition follows the shared state machine;
- list `orphan=true` returns unresolved monikers;
- delete removes the note from the persisted file;
- a note written after MCP context load is visible on the next notes action.

### Stage 3: Read-only TUI notes lens

Deliver:

- new navigation lens for notes;
- summary counts by status and orphan;
- rows ordered for triage;
- selected note panel;
- orphan rows visible even without a tree node.

Acceptance:

- notes lens displays pending, ongoing, done, and orphan counts;
- selecting an orphan note shows the raw moniker;
- selecting a resolved note shows target label and file;
- no editor yet.

### Stage 4: Navigator indicators and outline notes

Deliver:

- note summary attached to navigation rows by moniker;
- compact tree marker for nodes with active/done notes;
- outline panel renders notes before selected symbol details;
- notes are read-only from outline at this stage;
- note markers and outline data come from workspace note snapshots.

Acceptance:

- a symbol with a pending note has a visible tree marker;
- a symbol with only done notes has a lower-priority marker;
- outline shows notes before `selected`;
- symbols without notes keep the current outline layout.

### Stage 5: TUI note editor

Deliver:

- `UiMode::Note`;
- multiline editor in the main panel;
- create, edit, save, abandon empty draft;
- explicit delete with confirmation;
- kind and status controls;
- shared transition validation;
- status messages after save/delete/transition.
- selected keys: `n` edits or drafts for the current moniker, `N` forces a new
  draft, `8`/`m` opens the notes lens, `Ctrl+s` saves, `Ctrl+d` confirms
  delete, `Ctrl+k` cycles kind, and `Ctrl+o`/`Ctrl+p` move status through
  allowed transitions.

Acceptance:

- opening note mode on a selected moniker creates a draft;
- `Ctrl+s` saves a non-empty note;
- `Esc` on an empty new draft creates nothing;
- editing an existing note updates it;
- invalid transition is refused;
- delete existing note requires confirmation.

### Stage 6: MCP/read integration and agent workflow polish

Deliver:

- optional note summary in `code_moniker_read` for symbol URIs;
- `next:` suggestions from notes to symbol reads and from symbol reads to
  notes;
- note IDs in outputs suitable for agent follow-up;
- optional `updated_since` filter for agent polling.

Acceptance:

- an agent reading a symbol can discover associated notes;
- an agent listing pending notes can jump to the symbol read;
- done notes can be hidden by default while still retrievable.

### Stage 7: Migration and repair helpers

Deliver:

- orphan repair command/tool action to retarget a note to a new moniker;
- optional fuzzy suggestions based on old name/file/kind;
- audit output for unresolved notes.

Acceptance:

- orphan note can be retargeted without losing history fields;
- repair suggestions are opt-in and never mutate automatically;
- MCP can list and retarget orphan notes.

## Testing posture

Use durable contracts:

- store tests for persisted model and transition semantics;
- MCP tests through tool calls and response text/JSON contract;
- TUI acceptance through rendered text and key flows;
- no tests that freeze private editor widget composition.

The note editor is the riskiest surface. Keep its tests focused on observable
behavior: typed text appears, save persists, abandon creates nothing, delete
removes only after confirmation.

## Open decisions

- exact visual marker text for tree notes;
- whether `created_by` should be restricted to `user | agent` or allow a free
  local identity;
- whether done notes appear by default in outline or only collapsed;
- whether the first persistence implementation should reject unknown TOML
  fields or preserve them for forward compatibility.
