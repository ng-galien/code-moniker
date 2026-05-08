/*
 * pcm_plpgsql_driver.c — drive the vendored PL/pgSQL parser to obtain a
 * PLpgSQL_function* from a raw body string, without going through
 * plpgsql_compile_inline (which is hidden as a local symbol on macOS).
 *
 * Mirrors libpg_query's compile_create_function_stmt() but stripped to the
 * minimum needed by extractor: we don't simulate parameters, return type,
 * trigger NEW/OLD records, etc. — only what the bison grammar needs to
 * produce a valid action tree (FOUND magic var must exist; namespace must
 * be initialized; datums vector must be set up).
 *
 * The catalog lookups inside `plpgsql_build_datatype` for built-in types
 * (BOOLOID for FOUND, UNKNOWNOID for parameters) succeed because we run
 * inside the host PG backend that has the catalog loaded — the same
 * pragmatic compromise documented in src/lang/sql/body.rs.
 */

#include "postgres.h"
#include "fmgr.h"
#include "utils/memutils.h"
#include "utils/builtins.h"
#include "catalog/pg_type.h"
#include "catalog/pg_proc.h"

#include "plpgsql.h"

extern void plpgsql_start_datums(void);
extern void plpgsql_finish_datums(PLpgSQL_function *function);

/* GUCs normally defined in plpgsql.so's pl_handler.c. We don't vendor
 * pl_handler.c (its call/inline/validator handlers and _PG_init would
 * collide with the loaded plpgsql.so), so define here the ones
 * pl_comp.c references when populating PLpgSQL_function fields.
 * Values don't matter for our parse-only flow but the symbols must
 * resolve at link time. */
int  plpgsql_variable_conflict   = 0; /* PLPGSQL_RESOLVE_ERROR */
bool plpgsql_print_strict_params = false;
int  plpgsql_extra_warnings      = 0;
int  plpgsql_extra_errors        = 0;

/*
 * Build the namespace entries the bison parser expects for each
 * declared parameter so identifiers like `$1` and named params
 * resolve cleanly inside the body. We don't care about the actual
 * declared types — we never run the body — but the parser refuses
 * pseudo-types (UNKNOWNOID) inside `plpgsql_build_variable`, so use
 * TEXTOID as a concrete placeholder.
 */
static void
register_params(int n_params, const char *const *param_names)
{
	int i;
	for (i = 0; i < n_params; i++)
	{
		PLpgSQL_type *argdtype;
		PLpgSQL_variable *argvariable;
		char buf[32];
		const char *name;

		snprintf(buf, sizeof(buf), "$%d", i + 1);
		argdtype = plpgsql_build_datatype(TEXTOID, -1, InvalidOid, NULL);
		name = (param_names && param_names[i] && param_names[i][0])
			   ? param_names[i] : buf;
		argvariable = plpgsql_build_variable(name, 0, argdtype, false);
		plpgsql_ns_additem(argvariable->dtype == PLPGSQL_DTYPE_VAR
							   ? PLPGSQL_NSTYPE_VAR
							   : PLPGSQL_NSTYPE_REC,
						   argvariable->dno, buf);
		if (param_names && param_names[i] && param_names[i][0])
			plpgsql_ns_additem(argvariable->dtype == PLPGSQL_DTYPE_VAR
								   ? PLPGSQL_NSTYPE_VAR
								   : PLPGSQL_NSTYPE_REC,
							   argvariable->dno, param_names[i]);
	}
}

static PLpgSQL_function *parse_body_impl(
	const char *body,
	bool is_setof,
	bool is_void,
	int n_params,
	const char *const *param_names);

/*
 * Public entry point. Wraps the parsing in PG_TRY/PG_CATCH so any
 * ereport() the bison grammar raises (unsupported parameter shapes,
 * malformed body, type-resolution failures inside
 * plpgsql_build_datatype) is caught and turned into a NULL return.
 * The caller (Rust) interprets NULL as "best-effort body extraction
 * skipped for this function" and proceeds to the next def. Pair
 * each non-NULL return with a `pcm_plpgsql_free` call so the
 * function's MemoryContext is reclaimed.
 */
PLpgSQL_function *
pcm_plpgsql_parse_body(
	const char *body,
	bool is_setof,
	bool is_void,
	int n_params,
	const char *const *param_names)
{
	PLpgSQL_function *result = NULL;
	MemoryContext caller_cxt = CurrentMemoryContext;

	PG_TRY();
	{
		result = parse_body_impl(body, is_setof, is_void, n_params, param_names);
	}
	PG_CATCH();
	{
		MemoryContextSwitchTo(caller_cxt);
		FlushErrorState();
		plpgsql_compile_tmp_cxt = NULL;
		plpgsql_error_funcname = NULL;
		plpgsql_check_syntax = false;
		result = NULL;
	}
	PG_END_TRY();

	return result;
}

static PLpgSQL_function *
parse_body_impl(
	const char *body,
	bool is_setof,
	bool is_void,
	int n_params,
	const char *const *param_names)
{
	PLpgSQL_function *function;
	MemoryContext	  func_cxt;
	PLpgSQL_variable *var;
	int				  parse_rc;

	plpgsql_scanner_init(body);

	plpgsql_error_funcname = "<inline>";
	plpgsql_check_syntax = true;

	function = (PLpgSQL_function *) palloc0(sizeof(PLpgSQL_function));
	plpgsql_curr_compile = function;

	func_cxt = AllocSetContextCreate(CurrentMemoryContext,
									 "pg_code_moniker plpgsql ctx",
									 ALLOCSET_DEFAULT_SIZES);
	plpgsql_compile_tmp_cxt = MemoryContextSwitchTo(func_cxt);

	function->fn_signature = pstrdup("<inline>");
	function->fn_is_trigger = PLPGSQL_NOT_TRIGGER;
	function->fn_input_collation = InvalidOid;
	function->fn_cxt = func_cxt;
	function->out_param_varno = -1;
	function->resolve_option = plpgsql_variable_conflict;
	function->print_strict_params = plpgsql_print_strict_params;
	function->extra_warnings = 0;
	function->extra_errors = 0;

	plpgsql_ns_init();
	plpgsql_ns_push("<inline>", PLPGSQL_LABEL_BLOCK);
	plpgsql_DumpExecTree = false;
	plpgsql_start_datums();

	/* Mirror the source DDL's return shape so the bison grammar
	 * accepts the RETURN forms the source actually uses. ANYELEMENT
	 * (a polymorphic placeholder) lets the parser accept `RETURN
	 * expr` when the function is declared with a real return type
	 * we haven't bothered resolving — we never run the body, so the
	 * type only needs to be non-VOID and non-record. */
	function->fn_rettype = is_void ? VOIDOID : ANYELEMENTOID;
	function->fn_retset = is_setof;
	function->fn_retistuple = false;
	function->fn_retisdomain = false;
	function->fn_prokind = PROKIND_FUNCTION;
	function->fn_retbyval = true;
	function->fn_rettyplen = sizeof(int32);
	function->fn_readonly = false;

	register_params(n_params, param_names);

	/* Magic FOUND variable referenced by various stmts (FOR, EXECUTE INTO). */
	var = plpgsql_build_variable("found", 0,
								 plpgsql_build_datatype(BOOLOID,
														-1,
														InvalidOid,
														NULL),
								 true);
	function->found_varno = var->dno;

	parse_rc = plpgsql_yyparse();
	if (parse_rc != 0)
		elog(ERROR, "plpgsql parser returned %d", parse_rc);
	function->action = plpgsql_parse_result;

	plpgsql_scanner_finish();
	function->fn_nargs = 0;
	plpgsql_finish_datums(function);

	plpgsql_error_funcname = NULL;
	plpgsql_check_syntax = false;

	MemoryContextSwitchTo(plpgsql_compile_tmp_cxt);
	plpgsql_compile_tmp_cxt = NULL;

	return function;
}

/*
 * Reclaim the per-function MemoryContext that `parse_body_impl`
 * created in `AllocSetContextCreate(CurrentMemoryContext, ...)`.
 * Without this, every parsed plpgsql function leaks ~few KB into
 * the surrounding extraction call's memory context — accumulates
 * across an ESAC corpus ingest.
 */
void
pcm_plpgsql_free(PLpgSQL_function *function)
{
	if (function != NULL && function->fn_cxt != NULL)
		MemoryContextDelete(function->fn_cxt);
}
