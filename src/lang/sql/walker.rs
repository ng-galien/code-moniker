
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
			collect_calls_in(view_stmt.query, module, module, graph);
		}
		_ => {
			collect_calls_in(raw.stmt, module, module, graph);
		}
	}
}


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
		..DefAttrs::default()
	};
	if graph
		.add_def_attrs(func_moniker.clone(), kinds::FUNCTION, module, position, &attrs)
		.is_err()
	{
		return;
	}

	let (lang, body) = function_language_and_body(stmt.options);
	if lang.eq_ignore_ascii_case(b"plpgsql") && !body.is_empty() {
		let (is_setof, is_void) = return_shape(stmt.returnType);
		let param_names = function_param_names(stmt.parameters);
		body::walk_plpgsql_body(
			&body,
			is_setof,
			is_void,
			&param_names,
			&func_moniker,
			module,
			graph,
		);
	}
}

fn function_param_names(params: *mut pg_sys::List) -> Vec<Vec<u8>> {
	if params.is_null() {
		return Vec::new();
	}
	let list: pgrx::PgList<pg_sys::FunctionParameter> = unsafe { pgrx::PgList::from_pg(params) };
	list.iter_ptr()
		.map(|p| {
			if p.is_null() {
				return Vec::new();
			}
			let fp = unsafe { &*p };
			if fp.name.is_null() {
				Vec::new()
			} else {
				cstr_to_bytes(fp.name)
			}
		})
		.collect()
}

fn return_shape(return_type: *mut pg_sys::TypeName) -> (bool, bool) {
	if return_type.is_null() {
		return (false, true);
	}
	let rt = unsafe { &*return_type };
	let is_setof = rt.setof;
	let mut is_void = false;
	if !rt.names.is_null() {
		let names: pgrx::PgList<pg_sys::String> = unsafe { pgrx::PgList::from_pg(rt.names) };
		if let Some(last) = names.iter_ptr().last()
			&& !last.is_null() {
				let name = cstr_to_bytes(unsafe { (*last).sval });
				if name.eq_ignore_ascii_case(b"void") {
					is_void = true;
				}
			}
	}
	(is_setof, is_void)
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
	let target = extend_callable_arity(&parent, kinds::FUNCTION, &name, arity);
	let position = func_call_position(fc);
	let attrs = RefAttrs {
		confidence: kinds::CONF_UNRESOLVED,
		..RefAttrs::default()
	};
	let graph = unsafe { &mut *ctx.graph };
	let _ = graph.add_ref_attrs(source, target, kinds::REF_CALLS, position, &attrs);
}


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
				if unsafe { (*opt.arg).type_ } == pg_sys::NodeTag::T_List {
					let list: pgrx::PgList<pg_sys::String> =
						unsafe { pgrx::PgList::from_pg(opt.arg as *mut pg_sys::List) };
					if let Some(first) = list.iter_ptr().next()
						&& !first.is_null() {
							body = cstr_to_bytes(unsafe { (*first).sval });
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
