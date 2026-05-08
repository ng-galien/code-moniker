/* Parse a PL/pgSQL body to PLpgSQL_function* via the vendored parser. */

#include "postgres.h"
#include "fmgr.h"
#include "utils/memutils.h"
#include "utils/builtins.h"
#include "catalog/pg_type.h"
#include "catalog/pg_proc.h"

#include "plpgsql.h"

extern void plpgsql_start_datums(void);
extern void plpgsql_finish_datums(PLpgSQL_function *function);

/* GUCs referenced by pl_comp.c; link-time only, values unused. */
int  plpgsql_variable_conflict   = 0; /* PLPGSQL_RESOLVE_ERROR */
bool plpgsql_print_strict_params = false;
int  plpgsql_extra_warnings      = 0;
int  plpgsql_extra_errors        = 0;

/* Register `$N` and named params as TEXTOID variables for the parser. */
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

/* Returns NULL on parse error. Pair each non-NULL with `pcm_plpgsql_free`. */
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

	/* ANYELEMENT placeholder satisfies the parser without resolving real types. */
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

/* Reclaim the per-function MemoryContext allocated by `parse_body_impl`. */
void
pcm_plpgsql_free(PLpgSQL_function *function)
{
	if (function != NULL && function->fn_cxt != NULL)
		MemoryContextDelete(function->fn_cxt);
}
