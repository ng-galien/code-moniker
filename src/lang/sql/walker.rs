//! Top-level SQL statement walker. Drives `pg_parse_query` to obtain a
//! list of `RawStmt`, dispatches each to its emitter. Phase 1: DDL and
//! top-level call refs. Phase 2 hooks into `plpgsql_compile_inline` for
//! procedural bodies.
//!
//! Errors raised by the parser are caught at the whole-source level
//! and result in a module-only graph; per-statement isolation comes in
//! a follow-up once dollar-quote-aware splitting lands.

use std::ffi::{CStr, CString};

use pgrx::pg_sys;
use pgrx::pg_sys::pg_try::PgTryBuilder;

use crate::core::code_graph::{CodeGraph, DefAttrs, Position, RefAttrs};
use crate::core::moniker::Moniker;

use super::body;
use super::canonicalize::{extend_callable_arity, extend_callable_typed, extend_segment, maybe_schema};
use super::kinds;

pub(super) fn walk_source(
	source: &str,
	module: &Moniker,
	_deep: bool,
	graph: &mut CodeGraph,
) {
	let cstr = match CString::new(source) {
		Ok(c) => c,
		Err(_) => return,
	};

	let raw_list = PgTryBuilder::new(|| unsafe { pg_sys::pg_parse_query(cstr.as_ptr()) })
		.catch_others(|_| std::ptr::null_mut())
		.execute();

	if raw_list.is_null() {
		return;
	}

	let stmts: pgrx::PgList<pg_sys::RawStmt> = unsafe { pgrx::PgList::from_pg(raw_list) };
	for raw_ptr in stmts.iter_ptr() {
		if raw_ptr.is_null() {
			continue;
		}
		let raw = unsafe { &*raw_ptr };
		if raw.stmt.is_null() {
			continue;
		}
		dispatch_stmt(raw, source, module, graph);
	}
}

fn dispatch_stmt(raw: &pg_sys::RawStmt, source: &str, module: &Moniker, graph: &mut CodeGraph) {
	let node_type = unsafe { (*raw.stmt).type_ };
	let position = stmt_position(raw, source.len());

	match node_type {
		pg_sys::NodeTag::T_CreateFunctionStmt => {
			let stmt = raw.stmt as *const pg_sys::CreateFunctionStmt;
			emit_create_function(unsafe { &*stmt }, module, position, graph);
		}
		pg_sys::NodeTag::T_CreateStmt => {
			let stmt = raw.stmt as *const pg_sys::CreateStmt;
			emit_create_table(unsafe { &*stmt }, module, position, graph);
		}
		pg_sys::NodeTag::T_ViewStmt => {
			let stmt = raw.stmt as *const pg_sys::ViewStmt;
			let view_stmt = unsafe { &*stmt };
			emit_view(view_stmt, module, position, graph);
			// raw_expression_tree_walker doesn't accept ViewStmt itself
			// (it's a DDL statement). The interesting SELECT lives in
			// `query`, which IS walkable.
			collect_calls_in(view_stmt.query, module, module, graph);
		}
		_ => {
			collect_calls_in(raw.stmt, module, module, graph);
		}
	}
}

// --- def emitters -----------------------------------------------------

fn emit_create_function(
	stmt: &pg_sys::CreateFunctionStmt,
	module: &Moniker,
	position: Option<Position>,
	graph: &mut CodeGraph,
) {
	let qualified = qualified_name_from_list(stmt.funcname);
	let (schema, name) = match qualified {
		Some(q) => q,
		None => return,
	};
	let arg_types = function_param_types(stmt.parameters);
	let parent = maybe_schema(module, &schema);
	let func_moniker = extend_callable_typed(&parent, kinds::FUNCTION, &name, &arg_types);
	let signature = arg_types.join(b",".as_ref());
	let attrs = DefAttrs {
		visibility: kinds::VIS_NONE,
		signature: &signature,
	};
	if graph
		.add_def_attrs(func_moniker.clone(), kinds::FUNCTION, module, position, &attrs)
		.is_err()
	{
		return;
	}

	// Phase 2: descend into the PL/pgSQL body when this is a plpgsql
	// function. SQL-language bodies, C bindings and stand-alone SQL
	// functions are skipped — their inner SQL is parsed by raw_parser
	// at top level (or via sql_body, not yet covered).
	let (lang, body) = function_language_and_body(stmt.options);
	if lang.eq_ignore_ascii_case(b"plpgsql") && !body.is_empty() {
		body::walk_plpgsql_body(&body, &func_moniker, module, graph);
	}
}

fn emit_create_table(
	stmt: &pg_sys::CreateStmt,
	module: &Moniker,
	position: Option<Position>,
	graph: &mut CodeGraph,
) {
	if stmt.relation.is_null() {
		return;
	}
	let rv = unsafe { &*stmt.relation };
	let (schema, name) = match relation_name(rv) {
		Some(p) => p,
		None => return,
	};
	let parent = maybe_schema(module, &schema);
	let moniker = extend_segment(&parent, kinds::TABLE, &name);
	let _ = graph.add_def(moniker, kinds::TABLE, module, position);
}

fn emit_view(
	stmt: &pg_sys::ViewStmt,
	module: &Moniker,
	position: Option<Position>,
	graph: &mut CodeGraph,
) {
	if stmt.view.is_null() {
		return;
	}
	let rv = unsafe { &*stmt.view };
	let (schema, name) = match relation_name(rv) {
		Some(p) => p,
		None => return,
	};
	let parent = maybe_schema(module, &schema);
	let moniker = extend_segment(&parent, kinds::VIEW, &name);
	let _ = graph.add_def(moniker, kinds::VIEW, module, position);
}

// --- call ref collection ----------------------------------------------

/// Scratchpad passed through the C walker callback. Lifetimes are
/// erased for FFI; we re-borrow on each callback entry.
struct CallCtx {
	module: *const Moniker,
	source: *const Moniker,
	graph: *mut CodeGraph,
}

pub(super) fn collect_calls_in(
	node: *mut pg_sys::Node,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if node.is_null() {
		return;
	}
	let mut ctx = CallCtx {
		module: module as *const _,
		source: source_def as *const _,
		graph: graph as *mut _,
	};
	let ctx_ptr = &mut ctx as *mut _ as *mut ::core::ffi::c_void;
	// raw_expression_tree_walker rejects DDL statement nodes with
	// "unrecognized node type"; swallow the ereport so one foreign
	// shape does not abort the whole extraction.
	let _ = PgTryBuilder::new(|| unsafe {
		pg_sys::raw_expression_tree_walker_impl(node, Some(walker_cb), ctx_ptr)
	})
	.catch_others(|_| false)
	.execute();
}

unsafe extern "C-unwind" fn walker_cb(
	node: *mut pg_sys::Node,
	context: *mut ::core::ffi::c_void,
) -> bool {
	if node.is_null() {
		return false;
	}
	let ctx = unsafe { &mut *(context as *mut CallCtx) };
	let tag = unsafe { (*node).type_ };
	if tag == pg_sys::NodeTag::T_FuncCall {
		let fc = node as *const pg_sys::FuncCall;
		emit_call_ref(unsafe { &*fc }, ctx);
	}
	unsafe { pg_sys::raw_expression_tree_walker_impl(node, Some(walker_cb), context) }
}

fn emit_call_ref(fc: &pg_sys::FuncCall, ctx: &mut CallCtx) {
	let qualified = qualified_name_from_list(fc.funcname);
	let (schema, name) = match qualified {
		Some(q) => q,
		None => return,
	};
	let arity = list_len(fc.args);
	let module = unsafe { &*ctx.module };
	let source = unsafe { &*ctx.source };
	let parent = maybe_schema(module, &schema);
	// Raw_parser does not analyse argument types, so we only know
	// arity at a top-level call site. Defs use typed monikers; this
	// arity-only target won't match them via `=`. Mark the ref
	// `unresolved` so consumers know to project on name+arity.
	let target = extend_callable_arity(&parent, kinds::FUNCTION, &name, arity);
	let position = func_call_position(fc);
	let attrs = RefAttrs {
		receiver_hint: b"",
		alias: b"",
		confidence: kinds::CONF_UNRESOLVED,
	};
	let graph = unsafe { &mut *ctx.graph };
	let _ = graph.add_ref_attrs(source, target, kinds::REF_CALLS, position, &attrs);
}

// --- helpers ----------------------------------------------------------

fn stmt_position(raw: &pg_sys::RawStmt, source_len: usize) -> Option<Position> {
	let start = raw.stmt_location;
	if start < 0 {
		return None;
	}
	let start_u = start as u32;
	let len = raw.stmt_len.max(0) as u32;
	let mut end = start_u.saturating_add(len);
	if (end as usize) > source_len {
		end = source_len as u32;
	}
	Some((start_u, end))
}

fn func_call_position(fc: &pg_sys::FuncCall) -> Option<Position> {
	if fc.location < 0 {
		return None;
	}
	let start = fc.location as u32;
	Some((start, start))
}

fn list_len(list: *mut pg_sys::List) -> u16 {
	if list.is_null() {
		return 0;
	}
	let l: pgrx::PgList<pg_sys::Node> = unsafe { pgrx::PgList::from_pg(list) };
	l.len() as u16
}

fn qualified_name_from_list(list: *mut pg_sys::List) -> Option<(Vec<u8>, Vec<u8>)> {
	if list.is_null() {
		return None;
	}
	let parts: pgrx::PgList<pg_sys::String> = unsafe { pgrx::PgList::from_pg(list) };
	let strings: Vec<Vec<u8>> = parts
		.iter_ptr()
		.filter_map(|p| if p.is_null() { None } else { Some(unsafe { (*p).sval }) })
		.filter_map(|cstr| if cstr.is_null() { None } else { Some(cstr_to_bytes(cstr)) })
		.collect();
	match strings.len() {
		0 => None,
		1 => Some((Vec::new(), strings.into_iter().next().unwrap())),
		_ => {
			let mut it = strings.into_iter();
			let schema = it.next().unwrap();
			let name = it.last().unwrap();
			Some((schema, name))
		}
	}
}

/// CREATE FUNCTION carries the language name and body string as
/// `DefElem` entries inside `options`. Returns `(language, body)`
/// where either may be empty when absent.
fn function_language_and_body(options: *mut pg_sys::List) -> (Vec<u8>, Vec<u8>) {
	let mut lang = Vec::new();
	let mut body = Vec::new();
	if options.is_null() {
		return (lang, body);
	}
	let opts: pgrx::PgList<pg_sys::DefElem> = unsafe { pgrx::PgList::from_pg(options) };
	for opt_ptr in opts.iter_ptr() {
		if opt_ptr.is_null() {
			continue;
		}
		let opt = unsafe { &*opt_ptr };
		if opt.defname.is_null() || opt.arg.is_null() {
			continue;
		}
		let name = cstr_to_bytes(opt.defname);
		match name.as_slice() {
			b"language" => {
				if unsafe { (*opt.arg).type_ } == pg_sys::NodeTag::T_String {
					let s = opt.arg as *const pg_sys::String;
					lang = cstr_to_bytes(unsafe { (*s).sval });
				}
			}
			b"as" => {
				// `arg` is List of String. For PL/pgSQL there is one
				// element holding the body source. C-language functions
				// have two elements (lib path + symbol) — for those the
				// body is the lib path and we will drop it later via
				// the language check.
				if unsafe { (*opt.arg).type_ } == pg_sys::NodeTag::T_List {
					let list: pgrx::PgList<pg_sys::String> =
						unsafe { pgrx::PgList::from_pg(opt.arg as *mut pg_sys::List) };
					if let Some(first) = list.iter_ptr().next() {
						if !first.is_null() {
							body = cstr_to_bytes(unsafe { (*first).sval });
						}
					}
				}
			}
			_ => {}
		}
	}
	(lang, body)
}

fn relation_name(rv: &pg_sys::RangeVar) -> Option<(Vec<u8>, Vec<u8>)> {
	if rv.relname.is_null() {
		return None;
	}
	let name = cstr_to_bytes(rv.relname);
	let schema = if rv.schemaname.is_null() {
		Vec::new()
	} else {
		cstr_to_bytes(rv.schemaname)
	};
	Some((schema, name))
}

fn function_param_types(params: *mut pg_sys::List) -> Vec<Vec<u8>> {
	if params.is_null() {
		return Vec::new();
	}
	let list: pgrx::PgList<pg_sys::FunctionParameter> = unsafe { pgrx::PgList::from_pg(params) };
	list.iter_ptr()
		.filter_map(|p| {
			if p.is_null() {
				return None;
			}
			let fp = unsafe { &*p };
			if !param_mode_is_input(fp.mode) {
				return None;
			}
			Some(type_name_to_bytes(fp.argType))
		})
		.collect()
}

fn param_mode_is_input(mode: pg_sys::FunctionParameterMode::Type) -> bool {
	matches!(
		mode,
		pg_sys::FunctionParameterMode::FUNC_PARAM_IN
			| pg_sys::FunctionParameterMode::FUNC_PARAM_INOUT
			| pg_sys::FunctionParameterMode::FUNC_PARAM_VARIADIC
			| pg_sys::FunctionParameterMode::FUNC_PARAM_DEFAULT
	)
}

fn type_name_to_bytes(type_name: *mut pg_sys::TypeName) -> Vec<u8> {
	if type_name.is_null() {
		return Vec::new();
	}
	let cstr = unsafe { pg_sys::TypeNameToString(type_name) };
	if cstr.is_null() {
		return Vec::new();
	}
	let bytes = cstr_to_bytes(cstr);
	unsafe { pg_sys::pfree(cstr as *mut _) };
	strip_pg_catalog(bytes)
}

/// `pg_catalog.<type>` is the parser's qualified form for built-in type
/// keywords (`int` → `pg_catalog.int4`). Drop the implicit schema so
/// monikers stay readable; the unqualified form is still unambiguous
/// because user types must be qualified or single-segment names.
fn strip_pg_catalog(bytes: Vec<u8>) -> Vec<u8> {
	const PREFIX: &[u8] = b"pg_catalog.";
	if bytes.starts_with(PREFIX) {
		bytes[PREFIX.len()..].to_vec()
	} else {
		bytes
	}
}

fn cstr_to_bytes(p: *const ::core::ffi::c_char) -> Vec<u8> {
	if p.is_null() {
		return Vec::new();
	}
	unsafe { CStr::from_ptr(p) }.to_bytes().to_vec()
}
