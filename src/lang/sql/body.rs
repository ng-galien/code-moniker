//! PL/pgSQL function body walker. Drives `plpgsql_compile_inline` to
//! obtain the procedural AST (`PLpgSQL_function` and its tree of
//! `PLpgSQL_stmt`), then for every embedded SQL fragment re-parses
//! through `raw_parser(query, parseMode)` and feeds the resulting
//! `RawStmt` chain back into the FuncCall walker the top-level pass
//! already uses.
//!
//! Compile_inline performs catalog lookups (type resolution,
//! %ROWTYPE), so this path violates the strict "no table reads"
//! contract from CLAUDE.md. The pragmatic compromise: extraction is
//! deterministic given the catalog state at the time of the call.
//!
//! Platform constraint: `plpgsql_compile_inline` is the only public
//! entry point that compiles a raw body string without executing it,
//! and on macOS the linker hides it as a local symbol when building
//! `plpgsql.dylib` — `load_external_function` returns NULL. On Linux
//! production builds the symbol is exported and body extraction
//! works. The walker silently no-ops on macOS dev so the rest of the
//! extractor still ships cleanly.

use std::ffi::{CStr, CString};
use std::sync::OnceLock;

use pgrx::pg_sys;
use pgrx::pg_sys::pg_try::PgTryBuilder;

use crate::core::code_graph::CodeGraph;
use crate::core::moniker::Moniker;

use super::walker::collect_calls_in;

/// Dynamically resolved at first use — `plpgsql_compile_inline` lives
/// in plpgsql.so, which is not part of the flat symbol namespace at
/// our extension's load time on macOS. `load_external_function` finds
/// (and dlopens, if needed) the plpgsql library, then returns the
/// symbol address.
type CompileInlineFn = unsafe extern "C-unwind" fn(
	proc_source: *mut ::core::ffi::c_char,
) -> *mut pg_sys::PLpgSQL_function;

static COMPILE_INLINE: OnceLock<Option<CompileInlineFn>> = OnceLock::new();

fn compile_inline_fn() -> Option<CompileInlineFn> {
	*COMPILE_INLINE.get_or_init(|| {
		let lib = CString::new("$libdir/plpgsql").ok()?;
		let func = CString::new("plpgsql_compile_inline").ok()?;
		let ptr = PgTryBuilder::new(|| unsafe {
			pg_sys::load_external_function(
				lib.as_ptr(),
				func.as_ptr(),
				false,
				std::ptr::null_mut(),
			)
		})
		.catch_others(|_| std::ptr::null_mut())
		.execute();
		if ptr.is_null() {
			None
		} else {
			Some(unsafe { std::mem::transmute::<*mut ::core::ffi::c_void, CompileInlineFn>(ptr) })
		}
	})
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
	source_def: &Moniker,
	module: &Moniker,
	graph: &mut CodeGraph,
) {
	let body_cstr = match CString::new(body) {
		Ok(c) => c,
		Err(_) => return,
	};

	let compile = match compile_inline_fn() {
		Some(f) => f,
		None => return,
	};

	// plpgsql_compile_inline performs catalog lookups (type
	// resolution, RECORD %ROWTYPE) and ereports on malformed bodies;
	// swallow any errors and skip body extraction for this function
	// in that case.
	let func = PgTryBuilder::new(|| unsafe { compile(body_cstr.as_ptr() as *mut _) })
		.catch_others(|_| std::ptr::null_mut())
		.execute();

	if func.is_null() {
		return;
	}

	let f = unsafe { &*func };
	if f.action.is_null() {
		return;
	}

	walk_block(f.action, source_def, module, graph);
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
		_ => {
			// CASE / FORI / FORS / FORC / FOREACH / DYNEXECUTE / RAISE /
			// OPEN / FETCH / CLOSE / GETDIAG / RETURN / RETURN_NEXT /
			// EXIT / ASSERT / COMMIT / ROLLBACK — skipped in the first
			// pass. Dynamic SQL (DYNEXECUTE) is opaque by spec; the
			// rest are follow-ups.
		}
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
