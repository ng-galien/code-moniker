//! PL/pgSQL function body walker. Drives the vendored bison parser
//! (sources under `vendor/plpgsql/`, compiled by `build.rs`) and
//! emits one `calls` ref per FuncCall found in any embedded SQL
//! fragment. Each fragment is re-parsed via `raw_parser` and routed
//! through `walker::collect_calls_in`, the same dispatch the
//! top-level pass uses.

use std::ffi::{CStr, CString};

use pgrx::pg_sys;
use pgrx::pg_sys::pg_try::PgTryBuilder;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::walker::collect_calls_in;

unsafe extern "C-unwind" {
	/// Defined in `vendor/plpgsql/pcm_plpgsql_driver.c`. Drives the
	/// vendored bison parser to compile a body string into a
	/// `PLpgSQL_function` whose `action` field is the parsed
	/// `PLpgSQL_stmt_block` tree.
	///
	/// `is_setof` / `is_void` mirror the CreateFunctionStmt's return
	/// type so bison accepts the RETURN forms the source uses.
	/// `param_names` is an array of length `n_params`: each slot is a
	/// NUL-terminated parameter name (or NULL/empty for anonymous
	/// `$N` only). Without these registered the parser raises
	/// "variable $1 does not exist" on bodies that reference
	/// parameters.
	///
	/// Caller wraps in `PgTryBuilder` to catch syntax errors as
	/// ereport longjmps. Each non-NULL return must be paired with a
	/// `pcm_plpgsql_free` call once the AST has been walked.
	fn pcm_plpgsql_parse_body(
		body: *const ::core::ffi::c_char,
		is_setof: bool,
		is_void: bool,
		n_params: ::core::ffi::c_int,
		param_names: *const *const ::core::ffi::c_char,
	) -> *mut pg_sys::PLpgSQL_function;

	fn pcm_plpgsql_free(function: *mut pg_sys::PLpgSQL_function);
}

/// Parse the body of a PL/pgSQL function and emit `calls` refs for
/// every function call found in any embedded SQL fragment.
///
/// `body` is the raw text inside `AS $$ ... $$;`. `source_def` is the
/// moniker of the surrounding `CREATE FUNCTION` def — refs are
/// anchored there so consumers attribute calls to their containing
/// function.
pub(super) fn walk_plpgsql_body(
	body: &[u8],
	is_setof: bool,
	is_void: bool,
	param_names: &[Vec<u8>],
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let body_cstr = match CString::new(body) {
		Ok(c) => c,
		Err(_) => return,
	};

	// Each named param needs to outlive the FFI call; build CStrings
	// up-front and a parallel pointer array.
	let param_cstrs: Vec<CString> = param_names
		.iter()
		.map(|n| CString::new(n.as_slice()).unwrap_or_default())
		.collect();
	let param_ptrs: Vec<*const ::core::ffi::c_char> =
		param_cstrs.iter().map(|c| c.as_ptr()).collect();

	// The vendored parser ereports on malformed bodies — swallow
	// any longjmp and skip body extraction in that case.
	let func = PgTryBuilder::new(|| unsafe {
		pcm_plpgsql_parse_body(
			body_cstr.as_ptr(),
			is_setof,
			is_void,
			param_ptrs.len() as ::core::ffi::c_int,
			if param_ptrs.is_empty() { std::ptr::null() } else { param_ptrs.as_ptr() },
		)
	})
		.catch_others(|_| std::ptr::null_mut())
		.execute();

	if func.is_null() {
		return;
	}

	let action = unsafe { (*func).action };
	if !action.is_null() {
		walk_block(action, source_def, module, graph);
	}
	// `walk_block` only reads pointers into PG-allocated nodes — it
	// builds Rust-owned `Moniker` / `Vec<u8>` copies as it goes — so
	// reclaiming the parser's MemoryContext after the walk is safe.
	unsafe { pcm_plpgsql_free(func) };
}

fn walk_stmt_list(list: *mut pg_sys::List, source_def: &Moniker, module: &Moniker, graph: &mut CodeGraph) {
	if list.is_null() {
		return;
	}
	let stmts: pgrx::PgList<pg_sys::PLpgSQL_stmt> = unsafe { pgrx::PgList::from_pg(list) };
	for ptr in stmts.iter_ptr() {
		if ptr.is_null() {
			continue;
		}
		walk_stmt(ptr, source_def, module, graph);
	}
}

fn walk_block(
	block: *mut pg_sys::PLpgSQL_stmt_block,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if block.is_null() {
		return;
	}
	let b = unsafe { &*block };
	walk_stmt_list(b.body, source_def, module, graph);
}

fn walk_stmt(
	stmt: *mut pg_sys::PLpgSQL_stmt,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let cmd = unsafe { (*stmt).cmd_type };
	use pg_sys::PLpgSQL_stmt_type::*;
	match cmd {
		PLPGSQL_STMT_BLOCK => {
			walk_block(stmt as *mut pg_sys::PLpgSQL_stmt_block, source_def, module, graph);
		}
		PLPGSQL_STMT_ASSIGN => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_assign) };
			walk_expr(s.expr, source_def, module, graph);
		}
		PLPGSQL_STMT_IF => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_if) };
			walk_expr(s.cond, source_def, module, graph);
			walk_stmt_list(s.then_body, source_def, module, graph);
			walk_elsif_list(s.elsif_list, source_def, module, graph);
			walk_stmt_list(s.else_body, source_def, module, graph);
		}
		PLPGSQL_STMT_LOOP => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_loop) };
			walk_stmt_list(s.body, source_def, module, graph);
		}
		PLPGSQL_STMT_WHILE => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_while) };
			walk_expr(s.cond, source_def, module, graph);
			walk_stmt_list(s.body, source_def, module, graph);
		}
		PLPGSQL_STMT_RETURN_QUERY => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_return_query) };
			walk_expr(s.query, source_def, module, graph);
		}
		PLPGSQL_STMT_EXECSQL => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_execsql) };
			walk_expr(s.sqlstmt, source_def, module, graph);
		}
		PLPGSQL_STMT_PERFORM => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_perform) };
			walk_expr(s.expr, source_def, module, graph);
		}
		PLPGSQL_STMT_CALL => {
			let s = unsafe { &*(stmt as *mut pg_sys::PLpgSQL_stmt_call) };
			walk_expr(s.expr, source_def, module, graph);
		}
		// DYNEXECUTE is opaque by spec (dynamic SQL inside `EXECUTE
		// format(...)` cannot be parsed without resolving the
		// format string). Other stmt kinds (RAISE, RETURN*, FOR*,
		// CASE, OPEN/FETCH/CLOSE, GETDIAG, EXIT, ASSERT, COMMIT,
		// ROLLBACK) may carry expressions worth walking but are not
		// reached yet.
		_ => {}
	}
}

fn walk_elsif_list(
	list: *mut pg_sys::List,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if list.is_null() {
		return;
	}
	let elsifs: pgrx::PgList<pg_sys::PLpgSQL_if_elsif> = unsafe { pgrx::PgList::from_pg(list) };
	for ptr in elsifs.iter_ptr() {
		if ptr.is_null() {
			continue;
		}
		let e = unsafe { &*ptr };
		walk_expr(e.cond, source_def, module, graph);
		walk_stmt_list(e.stmts, source_def, module, graph);
	}
}

fn walk_expr(
	expr: *mut pg_sys::PLpgSQL_expr,
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	if expr.is_null() {
		return;
	}
	let e = unsafe { &*expr };
	if e.query.is_null() {
		return;
	}
	let query = unsafe { CStr::from_ptr(e.query) };
	let cstr = match CString::new(query.to_bytes()) {
		Ok(c) => c,
		Err(_) => return,
	};
	let raw_list = PgTryBuilder::new(|| unsafe { pg_sys::raw_parser(cstr.as_ptr(), e.parseMode) })
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
		collect_calls_in(raw.stmt, source_def, module, graph);
	}
}
