/* A Bison parser, made by GNU Bison 2.3.  */

/* Skeleton implementation for Bison's Yacc-like parsers in C

   Copyright (C) 1984, 1989, 1990, 2000, 2001, 2002, 2003, 2004, 2005, 2006
   Free Software Foundation, Inc.

   This program is free software; you can redistribute it and/or modify
   it under the terms of the GNU General Public License as published by
   the Free Software Foundation; either version 2, or (at your option)
   any later version.

   This program is distributed in the hope that it will be useful,
   but WITHOUT ANY WARRANTY; without even the implied warranty of
   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
   GNU General Public License for more details.

   You should have received a copy of the GNU General Public License
   along with this program; if not, write to the Free Software
   Foundation, Inc., 51 Franklin Street, Fifth Floor,
   Boston, MA 02110-1301, USA.  */

/* As a special exception, you may create a larger work that contains
   part or all of the Bison parser skeleton and distribute that work
   under terms of your choice, so long as that work isn't itself a
   parser generator using the skeleton or a modified version thereof
   as a parser skeleton.  Alternatively, if you modify or redistribute
   the parser skeleton itself, you may (at your option) remove this
   special exception, which will cause the skeleton and the resulting
   Bison output files to be licensed under the GNU General Public
   License without this special exception.

   This special exception was added by the Free Software Foundation in
   version 2.2 of Bison.  */

/* C LALR(1) parser skeleton written by Richard Stallman, by
   simplifying the original so-called "semantic" parser.  */

/* All symbols defined below should begin with yy or YY, to avoid
   infringing on user name space.  This should be done even for local
   variables, as they might otherwise be expanded by user macros.
   There are some unavoidable exceptions within include files to
   define necessary library symbols; they are noted "INFRINGES ON
   USER NAME SPACE" below.  */

/* Identify Bison output.  */
#define YYBISON 1

/* Bison version.  */
#define YYBISON_VERSION "2.3"

/* Skeleton name.  */
#define YYSKELETON_NAME "yacc.c"

/* Pure parsers.  */
#define YYPURE 0

/* Using locations.  */
#define YYLSP_NEEDED 1

/* Substitute the variable and function names.  */
#define yyparse plpgsql_yyparse
#define yylex   plpgsql_yylex
#define yyerror plpgsql_yyerror
#define yylval  plpgsql_yylval
#define yychar  plpgsql_yychar
#define yydebug plpgsql_yydebug
#define yynerrs plpgsql_yynerrs
#define yylloc plpgsql_yylloc

/* Tokens.  */
#ifndef YYTOKENTYPE
# define YYTOKENTYPE
   /* Put the tokens into the symbol table, so that GDB and other debuggers
      know about them.  */
   enum yytokentype {
     IDENT = 258,
     UIDENT = 259,
     FCONST = 260,
     SCONST = 261,
     USCONST = 262,
     BCONST = 263,
     XCONST = 264,
     Op = 265,
     ICONST = 266,
     PARAM = 267,
     TYPECAST = 268,
     DOT_DOT = 269,
     COLON_EQUALS = 270,
     EQUALS_GREATER = 271,
     LESS_EQUALS = 272,
     GREATER_EQUALS = 273,
     NOT_EQUALS = 274,
     T_WORD = 275,
     T_CWORD = 276,
     T_DATUM = 277,
     LESS_LESS = 278,
     GREATER_GREATER = 279,
     K_ABSOLUTE = 280,
     K_ALIAS = 281,
     K_ALL = 282,
     K_AND = 283,
     K_ARRAY = 284,
     K_ASSERT = 285,
     K_BACKWARD = 286,
     K_BEGIN = 287,
     K_BY = 288,
     K_CALL = 289,
     K_CASE = 290,
     K_CHAIN = 291,
     K_CLOSE = 292,
     K_COLLATE = 293,
     K_COLUMN = 294,
     K_COLUMN_NAME = 295,
     K_COMMIT = 296,
     K_CONSTANT = 297,
     K_CONSTRAINT = 298,
     K_CONSTRAINT_NAME = 299,
     K_CONTINUE = 300,
     K_CURRENT = 301,
     K_CURSOR = 302,
     K_DATATYPE = 303,
     K_DEBUG = 304,
     K_DECLARE = 305,
     K_DEFAULT = 306,
     K_DETAIL = 307,
     K_DIAGNOSTICS = 308,
     K_DO = 309,
     K_DUMP = 310,
     K_ELSE = 311,
     K_ELSIF = 312,
     K_END = 313,
     K_ERRCODE = 314,
     K_ERROR = 315,
     K_EXCEPTION = 316,
     K_EXECUTE = 317,
     K_EXIT = 318,
     K_FETCH = 319,
     K_FIRST = 320,
     K_FOR = 321,
     K_FOREACH = 322,
     K_FORWARD = 323,
     K_FROM = 324,
     K_GET = 325,
     K_HINT = 326,
     K_IF = 327,
     K_IMPORT = 328,
     K_IN = 329,
     K_INFO = 330,
     K_INSERT = 331,
     K_INTO = 332,
     K_IS = 333,
     K_LAST = 334,
     K_LOG = 335,
     K_LOOP = 336,
     K_MERGE = 337,
     K_MESSAGE = 338,
     K_MESSAGE_TEXT = 339,
     K_MOVE = 340,
     K_NEXT = 341,
     K_NO = 342,
     K_NOT = 343,
     K_NOTICE = 344,
     K_NULL = 345,
     K_OPEN = 346,
     K_OPTION = 347,
     K_OR = 348,
     K_PERFORM = 349,
     K_PG_CONTEXT = 350,
     K_PG_DATATYPE_NAME = 351,
     K_PG_EXCEPTION_CONTEXT = 352,
     K_PG_EXCEPTION_DETAIL = 353,
     K_PG_EXCEPTION_HINT = 354,
     K_PG_ROUTINE_OID = 355,
     K_PRINT_STRICT_PARAMS = 356,
     K_PRIOR = 357,
     K_QUERY = 358,
     K_RAISE = 359,
     K_RELATIVE = 360,
     K_RETURN = 361,
     K_RETURNED_SQLSTATE = 362,
     K_REVERSE = 363,
     K_ROLLBACK = 364,
     K_ROW_COUNT = 365,
     K_ROWTYPE = 366,
     K_SCHEMA = 367,
     K_SCHEMA_NAME = 368,
     K_SCROLL = 369,
     K_SLICE = 370,
     K_SQLSTATE = 371,
     K_STACKED = 372,
     K_STRICT = 373,
     K_TABLE = 374,
     K_TABLE_NAME = 375,
     K_THEN = 376,
     K_TO = 377,
     K_TYPE = 378,
     K_USE_COLUMN = 379,
     K_USE_VARIABLE = 380,
     K_USING = 381,
     K_VARIABLE_CONFLICT = 382,
     K_WARNING = 383,
     K_WHEN = 384,
     K_WHILE = 385
   };
#endif
/* Tokens.  */
#define IDENT 258
#define UIDENT 259
#define FCONST 260
#define SCONST 261
#define USCONST 262
#define BCONST 263
#define XCONST 264
#define Op 265
#define ICONST 266
#define PARAM 267
#define TYPECAST 268
#define DOT_DOT 269
#define COLON_EQUALS 270
#define EQUALS_GREATER 271
#define LESS_EQUALS 272
#define GREATER_EQUALS 273
#define NOT_EQUALS 274
#define T_WORD 275
#define T_CWORD 276
#define T_DATUM 277
#define LESS_LESS 278
#define GREATER_GREATER 279
#define K_ABSOLUTE 280
#define K_ALIAS 281
#define K_ALL 282
#define K_AND 283
#define K_ARRAY 284
#define K_ASSERT 285
#define K_BACKWARD 286
#define K_BEGIN 287
#define K_BY 288
#define K_CALL 289
#define K_CASE 290
#define K_CHAIN 291
#define K_CLOSE 292
#define K_COLLATE 293
#define K_COLUMN 294
#define K_COLUMN_NAME 295
#define K_COMMIT 296
#define K_CONSTANT 297
#define K_CONSTRAINT 298
#define K_CONSTRAINT_NAME 299
#define K_CONTINUE 300
#define K_CURRENT 301
#define K_CURSOR 302
#define K_DATATYPE 303
#define K_DEBUG 304
#define K_DECLARE 305
#define K_DEFAULT 306
#define K_DETAIL 307
#define K_DIAGNOSTICS 308
#define K_DO 309
#define K_DUMP 310
#define K_ELSE 311
#define K_ELSIF 312
#define K_END 313
#define K_ERRCODE 314
#define K_ERROR 315
#define K_EXCEPTION 316
#define K_EXECUTE 317
#define K_EXIT 318
#define K_FETCH 319
#define K_FIRST 320
#define K_FOR 321
#define K_FOREACH 322
#define K_FORWARD 323
#define K_FROM 324
#define K_GET 325
#define K_HINT 326
#define K_IF 327
#define K_IMPORT 328
#define K_IN 329
#define K_INFO 330
#define K_INSERT 331
#define K_INTO 332
#define K_IS 333
#define K_LAST 334
#define K_LOG 335
#define K_LOOP 336
#define K_MERGE 337
#define K_MESSAGE 338
#define K_MESSAGE_TEXT 339
#define K_MOVE 340
#define K_NEXT 341
#define K_NO 342
#define K_NOT 343
#define K_NOTICE 344
#define K_NULL 345
#define K_OPEN 346
#define K_OPTION 347
#define K_OR 348
#define K_PERFORM 349
#define K_PG_CONTEXT 350
#define K_PG_DATATYPE_NAME 351
#define K_PG_EXCEPTION_CONTEXT 352
#define K_PG_EXCEPTION_DETAIL 353
#define K_PG_EXCEPTION_HINT 354
#define K_PG_ROUTINE_OID 355
#define K_PRINT_STRICT_PARAMS 356
#define K_PRIOR 357
#define K_QUERY 358
#define K_RAISE 359
#define K_RELATIVE 360
#define K_RETURN 361
#define K_RETURNED_SQLSTATE 362
#define K_REVERSE 363
#define K_ROLLBACK 364
#define K_ROW_COUNT 365
#define K_ROWTYPE 366
#define K_SCHEMA 367
#define K_SCHEMA_NAME 368
#define K_SCROLL 369
#define K_SLICE 370
#define K_SQLSTATE 371
#define K_STACKED 372
#define K_STRICT 373
#define K_TABLE 374
#define K_TABLE_NAME 375
#define K_THEN 376
#define K_TO 377
#define K_TYPE 378
#define K_USE_COLUMN 379
#define K_USE_VARIABLE 380
#define K_USING 381
#define K_VARIABLE_CONFLICT 382
#define K_WARNING 383
#define K_WHEN 384
#define K_WHILE 385




/* Copy the first part of user declarations.  */
#line 1 "pl_gram.y"

/*-------------------------------------------------------------------------
 *
 * pl_gram.y			- Parser for the PL/pgSQL procedural language
 *
 * Portions Copyright (c) 1996-2024, PostgreSQL Global Development Group
 * Portions Copyright (c) 1994, Regents of the University of California
 *
 *
 * IDENTIFICATION
 *	  src/pl/plpgsql/src/pl_gram.y
 *
 *-------------------------------------------------------------------------
 */

#include "postgres.h"

#include "catalog/namespace.h"
#include "catalog/pg_proc.h"
#include "catalog/pg_type.h"
#include "parser/parser.h"
#include "parser/parse_type.h"
#include "parser/scanner.h"
#include "parser/scansup.h"
#include "utils/builtins.h"

#include "plpgsql.h"


/* Location tracking support --- simpler than bison's default */
#define YYLLOC_DEFAULT(Current, Rhs, N) \
	do { \
		if (N) \
			(Current) = (Rhs)[1]; \
		else \
			(Current) = (Rhs)[0]; \
	} while (0)

/*
 * Bison doesn't allocate anything that needs to live across parser calls,
 * so we can easily have it use palloc instead of malloc.  This prevents
 * memory leaks if we error out during parsing.
 */
#define YYMALLOC palloc
#define YYFREE   pfree


typedef struct
{
	int			location;
} sql_error_callback_arg;

#define parser_errposition(pos)  plpgsql_scanner_errposition(pos)

union YYSTYPE;					/* need forward reference for tok_is_keyword */

static	bool			tok_is_keyword(int token, union YYSTYPE *lval,
									   int kw_token, const char *kw_str);
static	void			word_is_not_variable(PLword *word, int location);
static	void			cword_is_not_variable(PLcword *cword, int location);
static	void			current_token_is_not_variable(int tok);
static	PLpgSQL_expr	*read_sql_construct(int until,
											int until2,
											int until3,
											const char *expected,
											RawParseMode parsemode,
											bool isexpression,
											bool valid_sql,
											int *startloc,
											int *endtoken);
static	PLpgSQL_expr	*read_sql_expression(int until,
											 const char *expected);
static	PLpgSQL_expr	*read_sql_expression2(int until, int until2,
											  const char *expected,
											  int *endtoken);
static	PLpgSQL_expr	*read_sql_stmt(void);
static	PLpgSQL_type	*read_datatype(int tok);
static	PLpgSQL_stmt	*make_execsql_stmt(int firsttoken, int location,
										   PLword *word);
static	PLpgSQL_stmt_fetch *read_fetch_direction(void);
static	void			 complete_direction(PLpgSQL_stmt_fetch *fetch,
											bool *check_FROM);
static	PLpgSQL_stmt	*make_return_stmt(int location);
static	PLpgSQL_stmt	*make_return_next_stmt(int location);
static	PLpgSQL_stmt	*make_return_query_stmt(int location);
static  PLpgSQL_stmt	*make_case(int location, PLpgSQL_expr *t_expr,
								   List *case_when_list, List *else_stmts);
static	char			*NameOfDatum(PLwdatum *wdatum);
static	void			 check_assignable(PLpgSQL_datum *datum, int location);
static	void			 read_into_target(PLpgSQL_variable **target,
										  bool *strict);
static	PLpgSQL_row		*read_into_scalar_list(char *initial_name,
											   PLpgSQL_datum *initial_datum,
											   int initial_location);
static	PLpgSQL_row		*make_scalar_list1(char *initial_name,
										   PLpgSQL_datum *initial_datum,
										   int lineno, int location);
static	void			 check_sql_expr(const char *stmt,
										RawParseMode parseMode, int location);
static	void			 plpgsql_sql_error_callback(void *arg);
static	PLpgSQL_type	*parse_datatype(const char *string, int location);
static	void			 check_labels(const char *start_label,
									  const char *end_label,
									  int end_location);
static	PLpgSQL_expr	*read_cursor_args(PLpgSQL_var *cursor,
										  int until);
static	List			*read_raise_options(void);
static	void			check_raise_parameters(PLpgSQL_stmt_raise *stmt);



/* Enabling traces.  */
#ifndef YYDEBUG
# define YYDEBUG 0
#endif

/* Enabling verbose error messages.  */
#ifdef YYERROR_VERBOSE
# undef YYERROR_VERBOSE
# define YYERROR_VERBOSE 1
#else
# define YYERROR_VERBOSE 0
#endif

/* Enabling the token table.  */
#ifndef YYTOKEN_TABLE
# define YYTOKEN_TABLE 0
#endif

#if ! defined YYSTYPE && ! defined YYSTYPE_IS_DECLARED
typedef union YYSTYPE
#line 117 "pl_gram.y"
{
	core_YYSTYPE core_yystype;
	/* these fields must match core_YYSTYPE: */
	int			ival;
	char	   *str;
	const char *keyword;

	PLword		word;
	PLcword		cword;
	PLwdatum	wdatum;
	bool		boolean;
	Oid			oid;
	struct
	{
		char	   *name;
		int			lineno;
	}			varname;
	struct
	{
		char	   *name;
		int			lineno;
		PLpgSQL_datum *scalar;
		PLpgSQL_datum *row;
	}			forvariable;
	struct
	{
		char	   *label;
		int			n_initvars;
		int		   *initvarnos;
	}			declhdr;
	struct
	{
		List	   *stmts;
		char	   *end_label;
		int			end_label_location;
	}			loop_body;
	List	   *list;
	PLpgSQL_type *dtype;
	PLpgSQL_datum *datum;
	PLpgSQL_var	*var;
	PLpgSQL_expr *expr;
	PLpgSQL_stmt *stmt;
	PLpgSQL_condition *condition;
	PLpgSQL_exception *exception;
	PLpgSQL_exception_block	*exception_block;
	PLpgSQL_nsitem *nsitem;
	PLpgSQL_diag_item *diagitem;
	PLpgSQL_stmt_fetch *fetch;
	PLpgSQL_case_when *casewhen;
}
/* Line 193 of yacc.c.  */
#line 526 "pl_gram.c"
	YYSTYPE;
# define yystype YYSTYPE /* obsolescent; will be withdrawn */
# define YYSTYPE_IS_DECLARED 1
# define YYSTYPE_IS_TRIVIAL 1
#endif

#if ! defined YYLTYPE && ! defined YYLTYPE_IS_DECLARED
typedef struct YYLTYPE
{
  int first_line;
  int first_column;
  int last_line;
  int last_column;
} YYLTYPE;
# define yyltype YYLTYPE /* obsolescent; will be withdrawn */
# define YYLTYPE_IS_DECLARED 1
# define YYLTYPE_IS_TRIVIAL 1
#endif


/* Copy the second part of user declarations.  */


/* Line 216 of yacc.c.  */
#line 551 "pl_gram.c"

#ifdef short
# undef short
#endif

#ifdef YYTYPE_UINT8
typedef YYTYPE_UINT8 yytype_uint8;
#else
typedef unsigned char yytype_uint8;
#endif

#ifdef YYTYPE_INT8
typedef YYTYPE_INT8 yytype_int8;
#elif (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
typedef signed char yytype_int8;
#else
typedef short int yytype_int8;
#endif

#ifdef YYTYPE_UINT16
typedef YYTYPE_UINT16 yytype_uint16;
#else
typedef unsigned short int yytype_uint16;
#endif

#ifdef YYTYPE_INT16
typedef YYTYPE_INT16 yytype_int16;
#else
typedef short int yytype_int16;
#endif

#ifndef YYSIZE_T
# ifdef __SIZE_TYPE__
#  define YYSIZE_T __SIZE_TYPE__
# elif defined size_t
#  define YYSIZE_T size_t
# elif ! defined YYSIZE_T && (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
#  include <stddef.h> /* INFRINGES ON USER NAME SPACE */
#  define YYSIZE_T size_t
# else
#  define YYSIZE_T unsigned int
# endif
#endif

#define YYSIZE_MAXIMUM ((YYSIZE_T) -1)

#ifndef YY_
# if defined YYENABLE_NLS && YYENABLE_NLS
#  if ENABLE_NLS
#   include <libintl.h> /* INFRINGES ON USER NAME SPACE */
#   define YY_(msgid) dgettext ("bison-runtime", msgid)
#  endif
# endif
# ifndef YY_
#  define YY_(msgid) msgid
# endif
#endif

/* Suppress unused-variable warnings by "using" E.  */
#if ! defined lint || defined __GNUC__
# define YYUSE(e) ((void) (e))
#else
# define YYUSE(e) /* empty */
#endif

/* Identity function, used to suppress warnings about constant conditions.  */
#ifndef lint
# define YYID(n) (n)
#else
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static int
YYID (int i)
#else
static int
YYID (i)
    int i;
#endif
{
  return i;
}
#endif

#if ! defined yyoverflow || YYERROR_VERBOSE

/* The parser invokes alloca or malloc; define the necessary symbols.  */

# ifdef YYSTACK_USE_ALLOCA
#  if YYSTACK_USE_ALLOCA
#   ifdef __GNUC__
#    define YYSTACK_ALLOC __builtin_alloca
#   elif defined __BUILTIN_VA_ARG_INCR
#    include <alloca.h> /* INFRINGES ON USER NAME SPACE */
#   elif defined _AIX
#    define YYSTACK_ALLOC __alloca
#   elif defined _MSC_VER
#    include <malloc.h> /* INFRINGES ON USER NAME SPACE */
#    define alloca _alloca
#   else
#    define YYSTACK_ALLOC alloca
#    if ! defined _ALLOCA_H && ! defined _STDLIB_H && (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
#     include <stdlib.h> /* INFRINGES ON USER NAME SPACE */
#     ifndef _STDLIB_H
#      define _STDLIB_H 1
#     endif
#    endif
#   endif
#  endif
# endif

# ifdef YYSTACK_ALLOC
   /* Pacify GCC's `empty if-body' warning.  */
#  define YYSTACK_FREE(Ptr) do { /* empty */; } while (YYID (0))
#  ifndef YYSTACK_ALLOC_MAXIMUM
    /* The OS might guarantee only one guard page at the bottom of the stack,
       and a page size can be as small as 4096 bytes.  So we cannot safely
       invoke alloca (N) if N exceeds 4096.  Use a slightly smaller number
       to allow for a few compiler-allocated temporary stack slots.  */
#   define YYSTACK_ALLOC_MAXIMUM 4032 /* reasonable circa 2006 */
#  endif
# else
#  define YYSTACK_ALLOC YYMALLOC
#  define YYSTACK_FREE YYFREE
#  ifndef YYSTACK_ALLOC_MAXIMUM
#   define YYSTACK_ALLOC_MAXIMUM YYSIZE_MAXIMUM
#  endif
#  if (defined __cplusplus && ! defined _STDLIB_H \
       && ! ((defined YYMALLOC || defined malloc) \
	     && (defined YYFREE || defined free)))
#   include <stdlib.h> /* INFRINGES ON USER NAME SPACE */
#   ifndef _STDLIB_H
#    define _STDLIB_H 1
#   endif
#  endif
#  ifndef YYMALLOC
#   define YYMALLOC malloc
#   if ! defined malloc && ! defined _STDLIB_H && (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
void *malloc (YYSIZE_T); /* INFRINGES ON USER NAME SPACE */
#   endif
#  endif
#  ifndef YYFREE
#   define YYFREE free
#   if ! defined free && ! defined _STDLIB_H && (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
void free (void *); /* INFRINGES ON USER NAME SPACE */
#   endif
#  endif
# endif
#endif /* ! defined yyoverflow || YYERROR_VERBOSE */


#if (! defined yyoverflow \
     && (! defined __cplusplus \
	 || (defined YYLTYPE_IS_TRIVIAL && YYLTYPE_IS_TRIVIAL \
	     && defined YYSTYPE_IS_TRIVIAL && YYSTYPE_IS_TRIVIAL)))

/* A type that is properly aligned for any stack member.  */
union yyalloc
{
  yytype_int16 yyss;
  YYSTYPE yyvs;
    YYLTYPE yyls;
};

/* The size of the maximum gap between one aligned stack and the next.  */
# define YYSTACK_GAP_MAXIMUM (sizeof (union yyalloc) - 1)

/* The size of an array large to enough to hold all stacks, each with
   N elements.  */
# define YYSTACK_BYTES(N) \
     ((N) * (sizeof (yytype_int16) + sizeof (YYSTYPE) + sizeof (YYLTYPE)) \
      + 2 * YYSTACK_GAP_MAXIMUM)

/* Copy COUNT objects from FROM to TO.  The source and destination do
   not overlap.  */
# ifndef YYCOPY
#  if defined __GNUC__ && 1 < __GNUC__
#   define YYCOPY(To, From, Count) \
      __builtin_memcpy (To, From, (Count) * sizeof (*(From)))
#  else
#   define YYCOPY(To, From, Count)		\
      do					\
	{					\
	  YYSIZE_T yyi;				\
	  for (yyi = 0; yyi < (Count); yyi++)	\
	    (To)[yyi] = (From)[yyi];		\
	}					\
      while (YYID (0))
#  endif
# endif

/* Relocate STACK from its old location to the new one.  The
   local variables YYSIZE and YYSTACKSIZE give the old and new number of
   elements in the stack, and YYPTR gives the new location of the
   stack.  Advance YYPTR to a properly aligned location for the next
   stack.  */
# define YYSTACK_RELOCATE(Stack)					\
    do									\
      {									\
	YYSIZE_T yynewbytes;						\
	YYCOPY (&yyptr->Stack, Stack, yysize);				\
	Stack = &yyptr->Stack;						\
	yynewbytes = yystacksize * sizeof (*Stack) + YYSTACK_GAP_MAXIMUM; \
	yyptr += yynewbytes / sizeof (*yyptr);				\
      }									\
    while (YYID (0))

#endif

/* YYFINAL -- State number of the termination state.  */
#define YYFINAL  3
/* YYLAST -- Last index in YYTABLE.  */
#define YYLAST   1293

/* YYNTOKENS -- Number of terminals.  */
#define YYNTOKENS  137
/* YYNNTS -- Number of nonterminals.  */
#define YYNNTS  87
/* YYNRULES -- Number of rules.  */
#define YYNRULES  253
/* YYNRULES -- Number of states.  */
#define YYNSTATES  334

/* YYTRANSLATE(YYLEX) -- Bison symbol number corresponding to YYLEX.  */
#define YYUNDEFTOK  2
#define YYMAXUTOK   385

#define YYTRANSLATE(YYX)						\
  ((unsigned int) (YYX) <= YYMAXUTOK ? yytranslate[YYX] : YYUNDEFTOK)

/* YYTRANSLATE[YYLEX] -- Bison symbol number corresponding to YYLEX.  */
static const yytype_uint8 yytranslate[] =
{
       0,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,   131,     2,     2,     2,     2,
     133,   134,     2,     2,   135,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,   132,
       2,   136,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     2,     2,     2,     2,
       2,     2,     2,     2,     2,     2,     1,     2,     3,     4,
       5,     6,     7,     8,     9,    10,    11,    12,    13,    14,
      15,    16,    17,    18,    19,    20,    21,    22,    23,    24,
      25,    26,    27,    28,    29,    30,    31,    32,    33,    34,
      35,    36,    37,    38,    39,    40,    41,    42,    43,    44,
      45,    46,    47,    48,    49,    50,    51,    52,    53,    54,
      55,    56,    57,    58,    59,    60,    61,    62,    63,    64,
      65,    66,    67,    68,    69,    70,    71,    72,    73,    74,
      75,    76,    77,    78,    79,    80,    81,    82,    83,    84,
      85,    86,    87,    88,    89,    90,    91,    92,    93,    94,
      95,    96,    97,    98,    99,   100,   101,   102,   103,   104,
     105,   106,   107,   108,   109,   110,   111,   112,   113,   114,
     115,   116,   117,   118,   119,   120,   121,   122,   123,   124,
     125,   126,   127,   128,   129,   130
};

#if YYDEBUG
/* YYPRHS[YYN] -- Index of the first RHS symbol of rule number YYN in
   YYRHS.  */
static const yytype_uint16 yyprhs[] =
{
       0,     0,     3,     7,     8,    11,    15,    19,    23,    27,
      31,    33,    35,    36,    38,    45,    47,    50,    54,    56,
      59,    61,    63,    65,    69,    76,    82,    83,    91,    92,
      95,    97,    98,    99,   103,   105,   109,   112,   114,   116,
     118,   120,   122,   124,   126,   127,   129,   130,   131,   134,
     137,   140,   141,   144,   146,   148,   150,   152,   154,   156,
     157,   160,   163,   165,   167,   169,   171,   173,   175,   177,
     179,   181,   183,   185,   187,   189,   191,   193,   195,   197,
     199,   201,   203,   205,   207,   209,   211,   213,   215,   217,
     223,   224,   226,   228,   232,   234,   238,   239,   241,   243,
     245,   254,   255,   260,   261,   264,   272,   273,   276,   278,
     282,   283,   286,   290,   295,   300,   303,   305,   307,   309,
     318,   319,   322,   326,   328,   330,   332,   334,   336,   342,
     344,   346,   348,   350,   352,   354,   357,   362,   367,   368,
     372,   375,   379,   383,   386,   390,   391,   393,   395,   397,
     398,   399,   403,   406,   408,   413,   417,   419,   421,   422,
     423,   424,   425,   429,   430,   434,   435,   437,   439,   442,
     444,   446,   448,   450,   452,   454,   456,   458,   460,   462,
     464,   466,   468,   470,   472,   474,   476,   478,   480,   482,
     484,   486,   488,   490,   492,   494,   496,   498,   500,   502,
     504,   506,   508,   510,   512,   514,   516,   518,   520,   522,
     524,   526,   528,   530,   532,   534,   536,   538,   540,   542,
     544,   546,   548,   550,   552,   554,   556,   558,   560,   562,
     564,   566,   568,   570,   572,   574,   576,   578,   580,   582,
     584,   586,   588,   590,   592,   594,   596,   598,   600,   602,
     604,   606,   608,   610
};

/* YYRHS -- A `-1'-separated list of the rules' RHS.  */
static const yytype_int16 yyrhs[] =
{
     138,     0,    -1,   139,   143,   142,    -1,    -1,   139,   140,
      -1,   131,    92,    55,    -1,   131,   101,   141,    -1,   131,
     127,    60,    -1,   131,   127,   125,    -1,   131,   127,   124,
      -1,    20,    -1,   223,    -1,    -1,   132,    -1,   144,    32,
     165,   209,    58,   220,    -1,   218,    -1,   218,   145,    -1,
     218,   145,   146,    -1,    50,    -1,   146,   147,    -1,   147,
      -1,   148,    -1,    50,    -1,    23,   222,    24,    -1,   157,
     158,   159,   160,   161,   162,    -1,   157,    26,    66,   156,
     132,    -1,    -1,   157,   150,    47,   149,   152,   155,   151,
      -1,    -1,    87,   114,    -1,   114,    -1,    -1,    -1,   133,
     153,   134,    -1,   154,    -1,   153,   135,   154,    -1,   157,
     159,    -1,    78,    -1,    66,    -1,    20,    -1,   223,    -1,
      21,    -1,    20,    -1,   223,    -1,    -1,    42,    -1,    -1,
      -1,    38,    20,    -1,    38,   223,    -1,    38,    21,    -1,
      -1,    88,    90,    -1,   132,    -1,   163,    -1,   164,    -1,
      51,    -1,   136,    -1,    15,    -1,    -1,   165,   166,    -1,
     143,   132,    -1,   169,    -1,   176,    -1,   179,    -1,   184,
      -1,   185,    -1,   186,    -1,   189,    -1,   191,    -1,   193,
      -1,   194,    -1,   195,    -1,   197,    -1,   198,    -1,   167,
      -1,   168,    -1,   170,    -1,   199,    -1,   200,    -1,   201,
      -1,   203,    -1,   204,    -1,   205,    -1,   206,    -1,    94,
      -1,    34,    -1,    54,    -1,    22,    -1,    70,   171,    53,
     172,   132,    -1,    -1,    46,    -1,   117,    -1,   172,   135,
     173,    -1,   173,    -1,   175,   164,   174,    -1,    -1,    22,
      -1,    20,    -1,    21,    -1,    72,   216,   165,   177,   178,
      58,    72,   132,    -1,    -1,   177,    57,   216,   165,    -1,
      -1,    56,   165,    -1,    35,   180,   181,   183,    58,    35,
     132,    -1,    -1,   181,   182,    -1,   182,    -1,   129,   216,
     165,    -1,    -1,    56,   165,    -1,   219,    81,   196,    -1,
     219,   130,   217,   196,    -1,   219,    66,   187,   196,    -1,
     188,    74,    -1,    22,    -1,    20,    -1,    21,    -1,   219,
      67,   188,   190,    74,    29,   217,   196,    -1,    -1,   115,
      11,    -1,   192,   220,   221,    -1,    63,    -1,    45,    -1,
     106,    -1,   104,    -1,    30,    -1,   165,    58,    81,   220,
     132,    -1,    73,    -1,    76,    -1,    82,    -1,    20,    -1,
      21,    -1,    62,    -1,    91,   208,    -1,    64,   202,   208,
      77,    -1,    85,   202,   208,   132,    -1,    -1,    37,   208,
     132,    -1,    90,   132,    -1,    41,   207,   132,    -1,   109,
     207,   132,    -1,    28,    36,    -1,    28,    87,    36,    -1,
      -1,    22,    -1,    20,    -1,    21,    -1,    -1,    -1,    61,
     210,   211,    -1,   211,   212,    -1,   212,    -1,   129,   213,
     121,   165,    -1,   213,    93,   214,    -1,   214,    -1,   222,
      -1,    -1,    -1,    -1,    -1,    23,   222,    24,    -1,    -1,
      23,   222,    24,    -1,    -1,   222,    -1,   132,    -1,   129,
     215,    -1,    20,    -1,   223,    -1,    22,    -1,    25,    -1,
      26,    -1,    28,    -1,    29,    -1,    30,    -1,    31,    -1,
      34,    -1,    36,    -1,    37,    -1,    38,    -1,    39,    -1,
      40,    -1,    41,    -1,    42,    -1,    43,    -1,    44,    -1,
      45,    -1,    46,    -1,    47,    -1,    48,    -1,    49,    -1,
      51,    -1,    52,    -1,    53,    -1,    54,    -1,    55,    -1,
      57,    -1,    59,    -1,    60,    -1,    61,    -1,    63,    -1,
      64,    -1,    65,    -1,    68,    -1,    70,    -1,    71,    -1,
      73,    -1,    75,    -1,    76,    -1,    78,    -1,    79,    -1,
      80,    -1,    82,    -1,    83,    -1,    84,    -1,    85,    -1,
      86,    -1,    87,    -1,    89,    -1,    91,    -1,    92,    -1,
      94,    -1,    95,    -1,    96,    -1,    97,    -1,    98,    -1,
      99,    -1,   100,    -1,   101,    -1,   102,    -1,   103,    -1,
     104,    -1,   105,    -1,   106,    -1,   107,    -1,   108,    -1,
     109,    -1,   110,    -1,   111,    -1,   112,    -1,   113,    -1,
     114,    -1,   115,    -1,   116,    -1,   117,    -1,   119,    -1,
     120,    -1,   123,    -1,   124,    -1,   125,    -1,   127,    -1,
     128,    -1
};

/* YYRLINE[YYN] -- source line where rule number YYN was defined.  */
static const yytype_uint16 yyrline[] =
{
       0,   359,   359,   365,   366,   369,   373,   382,   386,   390,
     396,   400,   405,   406,   409,   432,   440,   447,   456,   468,
     469,   472,   473,   477,   490,   528,   534,   533,   560,   563,
     567,   574,   580,   583,   614,   618,   624,   632,   633,   635,
     650,   665,   693,   721,   752,   753,   758,   770,   771,   776,
     781,   788,   789,   793,   795,   801,   802,   810,   811,   815,
     816,   826,   828,   830,   832,   834,   836,   838,   840,   842,
     844,   846,   848,   850,   852,   854,   856,   858,   860,   862,
     864,   866,   868,   870,   872,   876,   912,   930,   951,   990,
    1054,  1057,  1061,  1067,  1071,  1077,  1090,  1137,  1155,  1160,
    1167,  1185,  1188,  1202,  1205,  1211,  1218,  1232,  1236,  1242,
    1254,  1257,  1272,  1290,  1309,  1343,  1601,  1627,  1641,  1648,
    1687,  1690,  1696,  1749,  1753,  1759,  1785,  1930,  1954,  1972,
    1976,  1980,  1984,  1995,  2008,  2072,  2150,  2180,  2193,  2198,
    2212,  2219,  2233,  2248,  2249,  2250,  2254,  2276,  2281,  2289,
    2291,  2290,  2332,  2336,  2342,  2355,  2364,  2370,  2407,  2411,
    2415,  2419,  2423,  2431,  2435,  2443,  2446,  2453,  2455,  2462,
    2466,  2470,  2479,  2480,  2481,  2482,  2483,  2484,  2485,  2486,
    2487,  2488,  2489,  2490,  2491,  2492,  2493,  2494,  2495,  2496,
    2497,  2498,  2499,  2500,  2501,  2502,  2503,  2504,  2505,  2506,
    2507,  2508,  2509,  2510,  2511,  2512,  2513,  2514,  2515,  2516,
    2517,  2518,  2519,  2520,  2521,  2522,  2523,  2524,  2525,  2526,
    2527,  2528,  2529,  2530,  2531,  2532,  2533,  2534,  2535,  2536,
    2537,  2538,  2539,  2540,  2541,  2542,  2543,  2544,  2545,  2546,
    2547,  2548,  2549,  2550,  2551,  2552,  2553,  2554,  2555,  2556,
    2557,  2558,  2559,  2560
};
#endif

#if YYDEBUG || YYERROR_VERBOSE || YYTOKEN_TABLE
/* YYTNAME[SYMBOL-NUM] -- String name of the symbol SYMBOL-NUM.
   First, the terminals, then, starting at YYNTOKENS, nonterminals.  */
static const char *const yytname[] =
{
  "$end", "error", "$undefined", "IDENT", "UIDENT", "FCONST", "SCONST",
  "USCONST", "BCONST", "XCONST", "Op", "ICONST", "PARAM", "TYPECAST",
  "DOT_DOT", "COLON_EQUALS", "EQUALS_GREATER", "LESS_EQUALS",
  "GREATER_EQUALS", "NOT_EQUALS", "T_WORD", "T_CWORD", "T_DATUM",
  "LESS_LESS", "GREATER_GREATER", "K_ABSOLUTE", "K_ALIAS", "K_ALL",
  "K_AND", "K_ARRAY", "K_ASSERT", "K_BACKWARD", "K_BEGIN", "K_BY",
  "K_CALL", "K_CASE", "K_CHAIN", "K_CLOSE", "K_COLLATE", "K_COLUMN",
  "K_COLUMN_NAME", "K_COMMIT", "K_CONSTANT", "K_CONSTRAINT",
  "K_CONSTRAINT_NAME", "K_CONTINUE", "K_CURRENT", "K_CURSOR", "K_DATATYPE",
  "K_DEBUG", "K_DECLARE", "K_DEFAULT", "K_DETAIL", "K_DIAGNOSTICS", "K_DO",
  "K_DUMP", "K_ELSE", "K_ELSIF", "K_END", "K_ERRCODE", "K_ERROR",
  "K_EXCEPTION", "K_EXECUTE", "K_EXIT", "K_FETCH", "K_FIRST", "K_FOR",
  "K_FOREACH", "K_FORWARD", "K_FROM", "K_GET", "K_HINT", "K_IF",
  "K_IMPORT", "K_IN", "K_INFO", "K_INSERT", "K_INTO", "K_IS", "K_LAST",
  "K_LOG", "K_LOOP", "K_MERGE", "K_MESSAGE", "K_MESSAGE_TEXT", "K_MOVE",
  "K_NEXT", "K_NO", "K_NOT", "K_NOTICE", "K_NULL", "K_OPEN", "K_OPTION",
  "K_OR", "K_PERFORM", "K_PG_CONTEXT", "K_PG_DATATYPE_NAME",
  "K_PG_EXCEPTION_CONTEXT", "K_PG_EXCEPTION_DETAIL", "K_PG_EXCEPTION_HINT",
  "K_PG_ROUTINE_OID", "K_PRINT_STRICT_PARAMS", "K_PRIOR", "K_QUERY",
  "K_RAISE", "K_RELATIVE", "K_RETURN", "K_RETURNED_SQLSTATE", "K_REVERSE",
  "K_ROLLBACK", "K_ROW_COUNT", "K_ROWTYPE", "K_SCHEMA", "K_SCHEMA_NAME",
  "K_SCROLL", "K_SLICE", "K_SQLSTATE", "K_STACKED", "K_STRICT", "K_TABLE",
  "K_TABLE_NAME", "K_THEN", "K_TO", "K_TYPE", "K_USE_COLUMN",
  "K_USE_VARIABLE", "K_USING", "K_VARIABLE_CONFLICT", "K_WARNING",
  "K_WHEN", "K_WHILE", "'#'", "';'", "'('", "')'", "','", "'='", "$accept",
  "pl_function", "comp_options", "comp_option", "option_value", "opt_semi",
  "pl_block", "decl_sect", "decl_start", "decl_stmts", "decl_stmt",
  "decl_statement", "@1", "opt_scrollable", "decl_cursor_query",
  "decl_cursor_args", "decl_cursor_arglist", "decl_cursor_arg",
  "decl_is_for", "decl_aliasitem", "decl_varname", "decl_const",
  "decl_datatype", "decl_collate", "decl_notnull", "decl_defval",
  "decl_defkey", "assign_operator", "proc_sect", "proc_stmt",
  "stmt_perform", "stmt_call", "stmt_assign", "stmt_getdiag",
  "getdiag_area_opt", "getdiag_list", "getdiag_list_item", "getdiag_item",
  "getdiag_target", "stmt_if", "stmt_elsifs", "stmt_else", "stmt_case",
  "opt_expr_until_when", "case_when_list", "case_when", "opt_case_else",
  "stmt_loop", "stmt_while", "stmt_for", "for_control", "for_variable",
  "stmt_foreach_a", "foreach_slice", "stmt_exit", "exit_type",
  "stmt_return", "stmt_raise", "stmt_assert", "loop_body", "stmt_execsql",
  "stmt_dynexecute", "stmt_open", "stmt_fetch", "stmt_move",
  "opt_fetch_direction", "stmt_close", "stmt_null", "stmt_commit",
  "stmt_rollback", "opt_transaction_chain", "cursor_variable",
  "exception_sect", "@2", "proc_exceptions", "proc_exception",
  "proc_conditions", "proc_condition", "expr_until_semi",
  "expr_until_then", "expr_until_loop", "opt_block_label",
  "opt_loop_label", "opt_label", "opt_exitcond", "any_identifier",
  "unreserved_keyword", 0
};
#endif

# ifdef YYPRINT
/* YYTOKNUM[YYLEX-NUM] -- Internal token number corresponding to
   token YYLEX-NUM.  */
static const yytype_uint16 yytoknum[] =
{
       0,   256,   257,   258,   259,   260,   261,   262,   263,   264,
     265,   266,   267,   268,   269,   270,   271,   272,   273,   274,
     275,   276,   277,   278,   279,   280,   281,   282,   283,   284,
     285,   286,   287,   288,   289,   290,   291,   292,   293,   294,
     295,   296,   297,   298,   299,   300,   301,   302,   303,   304,
     305,   306,   307,   308,   309,   310,   311,   312,   313,   314,
     315,   316,   317,   318,   319,   320,   321,   322,   323,   324,
     325,   326,   327,   328,   329,   330,   331,   332,   333,   334,
     335,   336,   337,   338,   339,   340,   341,   342,   343,   344,
     345,   346,   347,   348,   349,   350,   351,   352,   353,   354,
     355,   356,   357,   358,   359,   360,   361,   362,   363,   364,
     365,   366,   367,   368,   369,   370,   371,   372,   373,   374,
     375,   376,   377,   378,   379,   380,   381,   382,   383,   384,
     385,    35,    59,    40,    41,    44,    61
};
# endif

/* YYR1[YYN] -- Symbol number of symbol that rule YYN derives.  */
static const yytype_uint8 yyr1[] =
{
       0,   137,   138,   139,   139,   140,   140,   140,   140,   140,
     141,   141,   142,   142,   143,   144,   144,   144,   145,   146,
     146,   147,   147,   147,   148,   148,   149,   148,   150,   150,
     150,   151,   152,   152,   153,   153,   154,   155,   155,   156,
     156,   156,   157,   157,   158,   158,   159,   160,   160,   160,
     160,   161,   161,   162,   162,   163,   163,   164,   164,   165,
     165,   166,   166,   166,   166,   166,   166,   166,   166,   166,
     166,   166,   166,   166,   166,   166,   166,   166,   166,   166,
     166,   166,   166,   166,   166,   167,   168,   168,   169,   170,
     171,   171,   171,   172,   172,   173,   174,   175,   175,   175,
     176,   177,   177,   178,   178,   179,   180,   181,   181,   182,
     183,   183,   184,   185,   186,   187,   188,   188,   188,   189,
     190,   190,   191,   192,   192,   193,   194,   195,   196,   197,
     197,   197,   197,   197,   198,   199,   200,   201,   202,   203,
     204,   205,   206,   207,   207,   207,   208,   208,   208,   209,
     210,   209,   211,   211,   212,   213,   213,   214,   215,   216,
     217,   218,   218,   219,   219,   220,   220,   221,   221,   222,
     222,   222,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223,   223,   223,   223,   223,   223,   223,
     223,   223,   223,   223
};

/* YYR2[YYN] -- Number of symbols composing right hand side of rule YYN.  */
static const yytype_uint8 yyr2[] =
{
       0,     2,     3,     0,     2,     3,     3,     3,     3,     3,
       1,     1,     0,     1,     6,     1,     2,     3,     1,     2,
       1,     1,     1,     3,     6,     5,     0,     7,     0,     2,
       1,     0,     0,     3,     1,     3,     2,     1,     1,     1,
       1,     1,     1,     1,     0,     1,     0,     0,     2,     2,
       2,     0,     2,     1,     1,     1,     1,     1,     1,     0,
       2,     2,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     5,
       0,     1,     1,     3,     1,     3,     0,     1,     1,     1,
       8,     0,     4,     0,     2,     7,     0,     2,     1,     3,
       0,     2,     3,     4,     4,     2,     1,     1,     1,     8,
       0,     2,     3,     1,     1,     1,     1,     1,     5,     1,
       1,     1,     1,     1,     1,     2,     4,     4,     0,     3,
       2,     3,     3,     2,     3,     0,     1,     1,     1,     0,
       0,     3,     2,     1,     4,     3,     1,     1,     0,     0,
       0,     0,     3,     0,     3,     0,     1,     1,     2,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1,     1,     1,     1,     1,     1,     1,
       1,     1,     1,     1
};

/* YYDEFACT[STATE-NAME] -- Default rule to reduce with in state
   STATE-NUM when YYTABLE doesn't specify something else to do.  Zero
   means the default is an error.  */
static const yytype_uint8 yydefact[] =
{
       3,     0,   161,     1,     0,     0,     4,    12,     0,    15,
     169,   171,   172,   173,   174,   175,   176,   177,   178,   179,
     180,   181,   182,   183,   184,   185,   186,   187,   188,   189,
     190,   191,   192,   193,   194,   195,   196,   197,   198,   199,
     200,   201,   202,   203,   204,   205,   206,   207,   208,   209,
     210,   211,   212,   213,   214,   215,   216,   217,   218,   219,
     220,   221,   222,   223,   224,   225,   226,   227,   228,   229,
     230,   231,   232,   233,   234,   235,   236,   237,   238,   239,
     240,   241,   242,   243,   244,   245,   246,   247,   248,   249,
     250,   251,   252,   253,     0,   170,     0,     0,     0,    13,
       2,    59,    18,    16,   162,     5,    10,     6,    11,     7,
       9,     8,   163,    42,     0,    22,    17,    20,    21,    44,
      43,   132,   133,    88,     0,   127,    86,   106,     0,   145,
     124,    87,   150,   134,   123,   138,    90,   159,   129,   130,
     131,   138,     0,     0,    85,   126,   125,   145,     0,    60,
      75,    76,    62,    77,    63,    64,    65,    66,    67,    68,
      69,   165,    70,    71,    72,    73,    74,    78,    79,    80,
      81,    82,    83,    84,     0,     0,     0,    19,     0,    45,
       0,    30,     0,    46,     0,     0,   147,   148,   146,     0,
       0,     0,     0,     0,    91,    92,     0,    59,     0,   140,
     135,     0,    61,     0,   166,   165,     0,     0,    59,   160,
      23,     0,    29,    26,    47,   164,   159,   110,   108,   139,
     143,     0,   141,     0,   151,   153,     0,     0,   163,     0,
     142,   158,   167,   122,    14,   117,   118,   116,    59,     0,
     120,   163,   112,    59,    39,    41,     0,    40,    32,     0,
      51,    59,    59,   107,     0,   144,     0,   156,   157,   152,
     136,    98,    99,    97,     0,    94,     0,   103,   137,   168,
     114,   115,     0,     0,     0,   113,    25,     0,     0,    48,
      50,    49,     0,     0,   163,   163,     0,     0,    59,    89,
       0,    58,    57,    96,    59,   159,     0,   121,     0,   165,
       0,    34,    46,    38,    37,    31,    52,    56,    53,    24,
      54,    55,     0,   155,   163,    93,    95,   163,    59,     0,
     160,     0,    33,     0,    36,    27,   105,   163,     0,    59,
     128,    35,   100,   119
};

/* YYDEFGOTO[NTERM-NUM].  */
static const yytype_int16 yydefgoto[] =
{
      -1,     1,     2,     6,   107,   100,   148,     8,   103,   116,
     117,   118,   248,   182,   325,   278,   300,   301,   305,   246,
     119,   183,   214,   250,   283,   309,   310,   293,   241,   149,
     150,   151,   152,   153,   196,   264,   265,   316,   266,   154,
     267,   296,   155,   185,   217,   218,   254,   156,   157,   158,
     238,   239,   159,   273,   160,   161,   162,   163,   164,   242,
     165,   166,   167,   168,   169,   193,   170,   171,   172,   173,
     191,   189,   174,   192,   224,   225,   256,   257,   269,   197,
     243,     9,   175,   203,   233,   204,    95
};

/* YYPACT[STATE-NUM] -- Index in YYTABLE of the portion describing
   STATE-NUM.  */
#define YYPACT_NINF -249
static const yytype_int16 yypact[] =
{
    -249,    16,   -18,  -249,   315,   -57,  -249,   -99,     8,    -8,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,    24,  -249,     3,   639,   -19,  -249,
    -249,  -249,  -249,   206,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,   986,  -249,   315,  -249,   206,  -249,  -249,   -20,
    -249,  -249,  -249,  -249,   315,  -249,  -249,  -249,   112,    27,
    -249,  -249,  -249,  -249,  -249,  -249,   -32,  -249,  -249,  -249,
    -249,  -249,   -72,   112,  -249,  -249,  -249,    27,   -63,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,   315,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,    14,    -3,    58,  -249,    18,  -249,
     -28,  -249,    56,  -249,    85,   -15,  -249,  -249,  -249,   -16,
     -21,    -6,    -1,   112,  -249,  -249,    82,  -249,   112,  -249,
    -249,     4,  -249,   -83,  -249,   315,   124,   124,  -249,  -249,
    -249,   424,  -249,  -249,    73,     0,  -249,   -48,  -249,  -249,
    -249,   102,  -249,   315,    -1,  -249,    64,   142,   854,    10,
    -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,    74,
      32,  1052,  -249,  -249,  -249,  -249,    19,  -249,    17,   533,
      68,  -249,  -249,  -249,   101,  -249,   -65,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,   -78,  -249,   -12,    20,  -249,  -249,
    -249,  -249,   149,    91,    86,  -249,  -249,   745,   -41,  -249,
    -249,  -249,    76,   -13,   -11,  1118,   133,   315,  -249,  -249,
     142,  -249,  -249,  -249,  -249,  -249,   111,  -249,   141,   315,
     -43,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -249,
    -249,  -249,    40,  -249,    67,  -249,  -249,  1184,  -249,   103,
    -249,    42,  -249,   745,  -249,  -249,  -249,   920,    45,  -249,
    -249,  -249,  -249,  -249
};

/* YYPGOTO[NTERM-NUM].  */
static const yytype_int16 yypgoto[] =
{
    -249,  -249,  -249,  -249,  -249,  -249,   176,  -249,  -249,  -249,
      63,  -249,  -249,  -249,  -249,  -249,  -249,  -143,  -249,  -249,
    -248,  -249,  -120,  -249,  -249,  -249,  -249,  -100,   -97,  -249,
    -249,  -249,  -249,  -249,  -249,  -249,  -106,  -249,  -249,  -249,
    -249,  -249,  -249,  -249,  -249,   -31,  -249,  -249,  -249,  -249,
    -249,   -22,  -249,  -249,  -249,  -249,  -249,  -249,  -249,  -207,
    -249,  -249,  -249,  -249,  -249,    46,  -249,  -249,  -249,  -249,
      41,  -125,  -249,  -249,  -249,   -35,  -249,   -95,  -249,  -199,
    -130,  -249,  -249,  -192,  -249,    -4,   -96
};

/* YYTABLE[YYPACT[STATE-NUM]].  What to do in state STATE-NUM.  If
   positive, shift that token.  If negative, reduce the rule which
   number is the opposite.  If zero, do what YYDEFACT says.
   If YYTABLE_NINF, syntax error.  */
#define YYTABLE_NINF -163
static const yytype_int16 yytable[] =
{
      94,   108,   291,   291,   112,     4,   178,   120,   252,   121,
     122,   123,   124,   234,   194,   220,     3,   251,   200,   125,
     120,  -161,   179,   126,   127,   303,   128,   -28,   287,   302,
     129,   270,  -162,    99,   130,    96,   275,   304,   307,  -161,
     101,   109,   102,   131,    97,  -109,   231,  -109,   104,   232,
    -162,   133,   134,   135,   289,   190,   288,   290,   105,   136,
     199,   137,   138,   206,   207,   139,   221,   180,   226,   202,
      98,   140,   205,   229,   141,   302,   294,   295,   208,   142,
     143,   216,   210,   144,   211,   195,   212,   121,   122,   123,
     124,   322,   323,   145,   181,   146,   318,   125,   147,  -161,
     228,   126,   127,   213,   128,   110,   111,   321,   129,   215,
     176,   249,   130,     5,   216,   247,   219,  -161,  -109,   308,
     184,   131,   333,   292,   292,  -154,   222,   209,   223,   133,
     134,   135,   186,   187,   188,   227,   230,   136,   255,   137,
     138,   260,   268,   139,   235,   236,   237,   272,   271,   140,
     277,   276,   141,   281,   284,   285,   282,   142,   143,   286,
     297,   144,   261,   262,   263,   298,   306,   299,   312,   319,
     320,   145,   326,   146,   330,   328,   147,   332,     7,   177,
     331,   120,   324,   311,   315,   240,   253,   198,   201,   259,
     329,   314,   313,     0,     0,     0,  -154,   317,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,     0,
       0,     0,     0,     0,     0,     0,     0,     0,     0,   258,
       0,   327,     0,     0,     0,     0,   113,   120,     0,   114,
       0,    12,    13,     0,    14,    15,    16,    17,     0,     0,
      18,     0,    19,    20,    21,    22,    23,    24,    25,    26,
      27,    28,    29,    30,    31,    32,   115,    33,    34,    35,
      36,    37,     0,    38,     0,    39,    40,    41,     0,    42,
      43,    44,     0,     0,    45,     0,    46,    47,     0,    48,
       0,    49,    50,   258,    51,    52,    53,     0,    54,    55,
      56,    57,    58,    59,     0,    60,     0,    61,    62,     0,
      63,    64,    65,    66,    67,    68,    69,    70,    71,    72,
      73,    74,    75,    76,    77,    78,    79,    80,    81,    82,
      83,    84,    85,    86,     0,    87,    88,     0,     0,    89,
      90,    91,     0,    92,    93,    10,     0,    11,     0,     0,
      12,    13,     0,    14,    15,    16,    17,     0,     0,    18,
       0,    19,    20,    21,    22,    23,    24,    25,    26,    27,
      28,    29,    30,    31,    32,     0,    33,    34,    35,    36,
      37,     0,    38,     0,    39,    40,    41,     0,    42,    43,
      44,     0,     0,    45,     0,    46,    47,     0,    48,     0,
      49,    50,     0,    51,    52,    53,     0,    54,    55,    56,
      57,    58,    59,     0,    60,     0,    61,    62,     0,    63,
      64,    65,    66,    67,    68,    69,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      84,    85,    86,     0,    87,    88,     0,     0,    89,    90,
      91,     0,    92,    93,   244,   245,     0,     0,     0,    12,
      13,     0,    14,    15,    16,    17,     0,     0,    18,     0,
      19,    20,    21,    22,    23,    24,    25,    26,    27,    28,
      29,    30,    31,    32,     0,    33,    34,    35,    36,    37,
       0,    38,     0,    39,    40,    41,     0,    42,    43,    44,
       0,     0,    45,     0,    46,    47,     0,    48,     0,    49,
      50,     0,    51,    52,    53,     0,    54,    55,    56,    57,
      58,    59,     0,    60,     0,    61,    62,     0,    63,    64,
      65,    66,    67,    68,    69,    70,    71,    72,    73,    74,
      75,    76,    77,    78,    79,    80,    81,    82,    83,    84,
      85,    86,     0,    87,    88,     0,     0,    89,    90,    91,
       0,    92,    93,   279,   280,     0,     0,     0,    12,    13,
       0,    14,    15,    16,    17,     0,     0,    18,     0,    19,
      20,    21,    22,    23,    24,    25,    26,    27,    28,    29,
      30,    31,    32,     0,    33,    34,    35,    36,    37,     0,
      38,     0,    39,    40,    41,     0,    42,    43,    44,     0,
       0,    45,     0,    46,    47,     0,    48,     0,    49,    50,
       0,    51,    52,    53,     0,    54,    55,    56,    57,    58,
      59,     0,    60,     0,    61,    62,     0,    63,    64,    65,
      66,    67,    68,    69,    70,    71,    72,    73,    74,    75,
      76,    77,    78,    79,    80,    81,    82,    83,    84,    85,
      86,     0,    87,    88,     0,     0,    89,    90,    91,   106,
      92,    93,     0,     0,    12,    13,     0,    14,    15,    16,
      17,     0,     0,    18,     0,    19,    20,    21,    22,    23,
      24,    25,    26,    27,    28,    29,    30,    31,    32,     0,
      33,    34,    35,    36,    37,     0,    38,     0,    39,    40,
      41,     0,    42,    43,    44,     0,     0,    45,     0,    46,
      47,     0,    48,     0,    49,    50,     0,    51,    52,    53,
       0,    54,    55,    56,    57,    58,    59,     0,    60,     0,
      61,    62,     0,    63,    64,    65,    66,    67,    68,    69,
      70,    71,    72,    73,    74,    75,    76,    77,    78,    79,
      80,    81,    82,    83,    84,    85,    86,     0,    87,    88,
       0,     0,    89,    90,    91,   113,    92,    93,     0,     0,
      12,    13,     0,    14,    15,    16,    17,     0,     0,    18,
       0,    19,    20,    21,    22,    23,    24,    25,    26,    27,
      28,    29,    30,    31,    32,     0,    33,    34,    35,    36,
      37,     0,    38,     0,    39,    40,    41,     0,    42,    43,
      44,     0,     0,    45,     0,    46,    47,     0,    48,     0,
      49,    50,     0,    51,    52,    53,     0,    54,    55,    56,
      57,    58,    59,     0,    60,     0,    61,    62,     0,    63,
      64,    65,    66,    67,    68,    69,    70,    71,    72,    73,
      74,    75,    76,    77,    78,    79,    80,    81,    82,    83,
      84,    85,    86,     0,    87,    88,     0,     0,    89,    90,
      91,     0,    92,    93,   121,   122,   123,   124,     0,     0,
       0,     0,     0,     0,   125,     0,  -161,     0,   126,   127,
       0,   128,     0,     0,     0,   129,     0,     0,     0,   130,
       0,     0,     0,     0,  -161,     0,     0,     0,   131,     0,
    -101,  -101,  -101,     0,     0,     0,   133,   134,   135,     0,
       0,     0,     0,     0,   136,     0,   137,   138,     0,     0,
     139,     0,     0,     0,     0,     0,   140,     0,     0,   141,
     121,   122,   123,   124,   142,   143,     0,     0,   144,     0,
     125,     0,  -161,     0,   126,   127,     0,   128,   145,     0,
     146,   129,     0,   147,     0,   130,     0,     0,     0,     0,
    -161,     0,     0,     0,   131,     0,  -102,  -102,  -102,     0,
       0,     0,   133,   134,   135,     0,     0,     0,     0,     0,
     136,     0,   137,   138,     0,     0,   139,     0,     0,     0,
       0,     0,   140,     0,     0,   141,   121,   122,   123,   124,
     142,   143,     0,     0,   144,     0,   125,     0,  -161,     0,
     126,   127,     0,   128,   145,     0,   146,   129,     0,   147,
       0,   130,     0,     0,     0,     0,  -161,     0,     0,     0,
     131,     0,     0,     0,  -149,     0,     0,   132,   133,   134,
     135,     0,     0,     0,     0,     0,   136,     0,   137,   138,
       0,     0,   139,     0,     0,     0,     0,     0,   140,     0,
       0,   141,   121,   122,   123,   124,   142,   143,     0,     0,
     144,     0,   125,     0,  -161,     0,   126,   127,     0,   128,
     145,     0,   146,   129,     0,   147,     0,   130,     0,     0,
       0,     0,  -161,     0,     0,     0,   131,     0,     0,     0,
     274,     0,     0,     0,   133,   134,   135,     0,     0,     0,
       0,     0,   136,     0,   137,   138,     0,     0,   139,     0,
       0,     0,     0,     0,   140,     0,     0,   141,   121,   122,
     123,   124,   142,   143,     0,     0,   144,     0,   125,     0,
    -161,     0,   126,   127,     0,   128,   145,     0,   146,   129,
       0,   147,     0,   130,     0,     0,     0,     0,  -161,     0,
       0,     0,   131,     0,     0,     0,  -111,     0,     0,     0,
     133,   134,   135,     0,     0,     0,     0,     0,   136,     0,
     137,   138,     0,     0,   139,     0,     0,     0,     0,     0,
     140,     0,     0,   141,   121,   122,   123,   124,   142,   143,
       0,     0,   144,     0,   125,     0,  -161,     0,   126,   127,
       0,   128,   145,     0,   146,   129,     0,   147,     0,   130,
       0,     0,     0,     0,  -161,     0,     0,     0,   131,     0,
       0,     0,  -104,     0,     0,     0,   133,   134,   135,     0,
       0,     0,     0,     0,   136,     0,   137,   138,     0,     0,
     139,     0,     0,     0,     0,     0,   140,     0,     0,   141,
       0,     0,     0,     0,   142,   143,     0,     0,   144,     0,
       0,     0,     0,     0,     0,     0,     0,     0,   145,     0,
     146,     0,     0,   147
};

static const yytype_int16 yycheck[] =
{
       4,    97,    15,    15,   101,    23,    26,   103,    56,    20,
      21,    22,    23,   205,    46,    36,     0,   216,   143,    30,
     116,    32,    42,    34,    35,    66,    37,    47,    93,   277,
      41,   238,    32,   132,    45,    92,   243,    78,    51,    50,
      32,    60,    50,    54,   101,    56,   129,    58,    24,   132,
      50,    62,    63,    64,   132,    28,   121,   135,    55,    70,
     132,    72,    73,    66,    67,    76,    87,    87,   193,   132,
     127,    82,    58,   198,    85,   323,    56,    57,    81,    90,
      91,   129,    24,    94,    66,   117,   114,    20,    21,    22,
      23,   134,   135,   104,   114,   106,   295,    30,   109,    32,
     197,    34,    35,    47,    37,   124,   125,   299,    41,    24,
     114,    38,    45,   131,   129,   211,   132,    50,   129,   132,
     124,    54,   329,   136,   136,    58,   132,   130,   129,    62,
      63,    64,    20,    21,    22,    53,   132,    70,    36,    72,
      73,    77,   132,    76,    20,    21,    22,   115,    74,    82,
     133,   132,    85,   249,   251,   252,    88,    90,    91,    58,
      11,    94,    20,    21,    22,    74,    90,    81,    35,    58,
      29,   104,   132,   106,   132,    72,   109,   132,     2,   116,
     323,   277,   302,   283,   290,   207,   217,   141,   147,   224,
     320,   288,   287,    -1,    -1,    -1,   129,   294,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,   223,
      -1,   318,    -1,    -1,    -1,    -1,    20,   323,    -1,    23,
      -1,    25,    26,    -1,    28,    29,    30,    31,    -1,    -1,
      34,    -1,    36,    37,    38,    39,    40,    41,    42,    43,
      44,    45,    46,    47,    48,    49,    50,    51,    52,    53,
      54,    55,    -1,    57,    -1,    59,    60,    61,    -1,    63,
      64,    65,    -1,    -1,    68,    -1,    70,    71,    -1,    73,
      -1,    75,    76,   287,    78,    79,    80,    -1,    82,    83,
      84,    85,    86,    87,    -1,    89,    -1,    91,    92,    -1,
      94,    95,    96,    97,    98,    99,   100,   101,   102,   103,
     104,   105,   106,   107,   108,   109,   110,   111,   112,   113,
     114,   115,   116,   117,    -1,   119,   120,    -1,    -1,   123,
     124,   125,    -1,   127,   128,    20,    -1,    22,    -1,    -1,
      25,    26,    -1,    28,    29,    30,    31,    -1,    -1,    34,
      -1,    36,    37,    38,    39,    40,    41,    42,    43,    44,
      45,    46,    47,    48,    49,    -1,    51,    52,    53,    54,
      55,    -1,    57,    -1,    59,    60,    61,    -1,    63,    64,
      65,    -1,    -1,    68,    -1,    70,    71,    -1,    73,    -1,
      75,    76,    -1,    78,    79,    80,    -1,    82,    83,    84,
      85,    86,    87,    -1,    89,    -1,    91,    92,    -1,    94,
      95,    96,    97,    98,    99,   100,   101,   102,   103,   104,
     105,   106,   107,   108,   109,   110,   111,   112,   113,   114,
     115,   116,   117,    -1,   119,   120,    -1,    -1,   123,   124,
     125,    -1,   127,   128,    20,    21,    -1,    -1,    -1,    25,
      26,    -1,    28,    29,    30,    31,    -1,    -1,    34,    -1,
      36,    37,    38,    39,    40,    41,    42,    43,    44,    45,
      46,    47,    48,    49,    -1,    51,    52,    53,    54,    55,
      -1,    57,    -1,    59,    60,    61,    -1,    63,    64,    65,
      -1,    -1,    68,    -1,    70,    71,    -1,    73,    -1,    75,
      76,    -1,    78,    79,    80,    -1,    82,    83,    84,    85,
      86,    87,    -1,    89,    -1,    91,    92,    -1,    94,    95,
      96,    97,    98,    99,   100,   101,   102,   103,   104,   105,
     106,   107,   108,   109,   110,   111,   112,   113,   114,   115,
     116,   117,    -1,   119,   120,    -1,    -1,   123,   124,   125,
      -1,   127,   128,    20,    21,    -1,    -1,    -1,    25,    26,
      -1,    28,    29,    30,    31,    -1,    -1,    34,    -1,    36,
      37,    38,    39,    40,    41,    42,    43,    44,    45,    46,
      47,    48,    49,    -1,    51,    52,    53,    54,    55,    -1,
      57,    -1,    59,    60,    61,    -1,    63,    64,    65,    -1,
      -1,    68,    -1,    70,    71,    -1,    73,    -1,    75,    76,
      -1,    78,    79,    80,    -1,    82,    83,    84,    85,    86,
      87,    -1,    89,    -1,    91,    92,    -1,    94,    95,    96,
      97,    98,    99,   100,   101,   102,   103,   104,   105,   106,
     107,   108,   109,   110,   111,   112,   113,   114,   115,   116,
     117,    -1,   119,   120,    -1,    -1,   123,   124,   125,    20,
     127,   128,    -1,    -1,    25,    26,    -1,    28,    29,    30,
      31,    -1,    -1,    34,    -1,    36,    37,    38,    39,    40,
      41,    42,    43,    44,    45,    46,    47,    48,    49,    -1,
      51,    52,    53,    54,    55,    -1,    57,    -1,    59,    60,
      61,    -1,    63,    64,    65,    -1,    -1,    68,    -1,    70,
      71,    -1,    73,    -1,    75,    76,    -1,    78,    79,    80,
      -1,    82,    83,    84,    85,    86,    87,    -1,    89,    -1,
      91,    92,    -1,    94,    95,    96,    97,    98,    99,   100,
     101,   102,   103,   104,   105,   106,   107,   108,   109,   110,
     111,   112,   113,   114,   115,   116,   117,    -1,   119,   120,
      -1,    -1,   123,   124,   125,    20,   127,   128,    -1,    -1,
      25,    26,    -1,    28,    29,    30,    31,    -1,    -1,    34,
      -1,    36,    37,    38,    39,    40,    41,    42,    43,    44,
      45,    46,    47,    48,    49,    -1,    51,    52,    53,    54,
      55,    -1,    57,    -1,    59,    60,    61,    -1,    63,    64,
      65,    -1,    -1,    68,    -1,    70,    71,    -1,    73,    -1,
      75,    76,    -1,    78,    79,    80,    -1,    82,    83,    84,
      85,    86,    87,    -1,    89,    -1,    91,    92,    -1,    94,
      95,    96,    97,    98,    99,   100,   101,   102,   103,   104,
     105,   106,   107,   108,   109,   110,   111,   112,   113,   114,
     115,   116,   117,    -1,   119,   120,    -1,    -1,   123,   124,
     125,    -1,   127,   128,    20,    21,    22,    23,    -1,    -1,
      -1,    -1,    -1,    -1,    30,    -1,    32,    -1,    34,    35,
      -1,    37,    -1,    -1,    -1,    41,    -1,    -1,    -1,    45,
      -1,    -1,    -1,    -1,    50,    -1,    -1,    -1,    54,    -1,
      56,    57,    58,    -1,    -1,    -1,    62,    63,    64,    -1,
      -1,    -1,    -1,    -1,    70,    -1,    72,    73,    -1,    -1,
      76,    -1,    -1,    -1,    -1,    -1,    82,    -1,    -1,    85,
      20,    21,    22,    23,    90,    91,    -1,    -1,    94,    -1,
      30,    -1,    32,    -1,    34,    35,    -1,    37,   104,    -1,
     106,    41,    -1,   109,    -1,    45,    -1,    -1,    -1,    -1,
      50,    -1,    -1,    -1,    54,    -1,    56,    57,    58,    -1,
      -1,    -1,    62,    63,    64,    -1,    -1,    -1,    -1,    -1,
      70,    -1,    72,    73,    -1,    -1,    76,    -1,    -1,    -1,
      -1,    -1,    82,    -1,    -1,    85,    20,    21,    22,    23,
      90,    91,    -1,    -1,    94,    -1,    30,    -1,    32,    -1,
      34,    35,    -1,    37,   104,    -1,   106,    41,    -1,   109,
      -1,    45,    -1,    -1,    -1,    -1,    50,    -1,    -1,    -1,
      54,    -1,    -1,    -1,    58,    -1,    -1,    61,    62,    63,
      64,    -1,    -1,    -1,    -1,    -1,    70,    -1,    72,    73,
      -1,    -1,    76,    -1,    -1,    -1,    -1,    -1,    82,    -1,
      -1,    85,    20,    21,    22,    23,    90,    91,    -1,    -1,
      94,    -1,    30,    -1,    32,    -1,    34,    35,    -1,    37,
     104,    -1,   106,    41,    -1,   109,    -1,    45,    -1,    -1,
      -1,    -1,    50,    -1,    -1,    -1,    54,    -1,    -1,    -1,
      58,    -1,    -1,    -1,    62,    63,    64,    -1,    -1,    -1,
      -1,    -1,    70,    -1,    72,    73,    -1,    -1,    76,    -1,
      -1,    -1,    -1,    -1,    82,    -1,    -1,    85,    20,    21,
      22,    23,    90,    91,    -1,    -1,    94,    -1,    30,    -1,
      32,    -1,    34,    35,    -1,    37,   104,    -1,   106,    41,
      -1,   109,    -1,    45,    -1,    -1,    -1,    -1,    50,    -1,
      -1,    -1,    54,    -1,    -1,    -1,    58,    -1,    -1,    -1,
      62,    63,    64,    -1,    -1,    -1,    -1,    -1,    70,    -1,
      72,    73,    -1,    -1,    76,    -1,    -1,    -1,    -1,    -1,
      82,    -1,    -1,    85,    20,    21,    22,    23,    90,    91,
      -1,    -1,    94,    -1,    30,    -1,    32,    -1,    34,    35,
      -1,    37,   104,    -1,   106,    41,    -1,   109,    -1,    45,
      -1,    -1,    -1,    -1,    50,    -1,    -1,    -1,    54,    -1,
      -1,    -1,    58,    -1,    -1,    -1,    62,    63,    64,    -1,
      -1,    -1,    -1,    -1,    70,    -1,    72,    73,    -1,    -1,
      76,    -1,    -1,    -1,    -1,    -1,    82,    -1,    -1,    85,
      -1,    -1,    -1,    -1,    90,    91,    -1,    -1,    94,    -1,
      -1,    -1,    -1,    -1,    -1,    -1,    -1,    -1,   104,    -1,
     106,    -1,    -1,   109
};

/* YYSTOS[STATE-NUM] -- The (internal number of the) accessing
   symbol of state STATE-NUM.  */
static const yytype_uint8 yystos[] =
{
       0,   138,   139,     0,    23,   131,   140,   143,   144,   218,
      20,    22,    25,    26,    28,    29,    30,    31,    34,    36,
      37,    38,    39,    40,    41,    42,    43,    44,    45,    46,
      47,    48,    49,    51,    52,    53,    54,    55,    57,    59,
      60,    61,    63,    64,    65,    68,    70,    71,    73,    75,
      76,    78,    79,    80,    82,    83,    84,    85,    86,    87,
      89,    91,    92,    94,    95,    96,    97,    98,    99,   100,
     101,   102,   103,   104,   105,   106,   107,   108,   109,   110,
     111,   112,   113,   114,   115,   116,   117,   119,   120,   123,
     124,   125,   127,   128,   222,   223,    92,   101,   127,   132,
     142,    32,    50,   145,    24,    55,    20,   141,   223,    60,
     124,   125,   165,    20,    23,    50,   146,   147,   148,   157,
     223,    20,    21,    22,    23,    30,    34,    35,    37,    41,
      45,    54,    61,    62,    63,    64,    70,    72,    73,    76,
      82,    85,    90,    91,    94,   104,   106,   109,   143,   166,
     167,   168,   169,   170,   176,   179,   184,   185,   186,   189,
     191,   192,   193,   194,   195,   197,   198,   199,   200,   201,
     203,   204,   205,   206,   209,   219,   222,   147,    26,    42,
      87,   114,   150,   158,   222,   180,    20,    21,    22,   208,
      28,   207,   210,   202,    46,   117,   171,   216,   202,   132,
     208,   207,   132,   220,   222,    58,    66,    67,    81,   130,
      24,    66,   114,    47,   159,    24,   129,   181,   182,   132,
      36,    87,   132,   129,   211,   212,   208,    53,   165,   208,
     132,   129,   132,   221,   220,    20,    21,    22,   187,   188,
     188,   165,   196,   217,    20,    21,   156,   223,   149,    38,
     160,   216,    56,   182,   183,    36,   213,   214,   222,   212,
      77,    20,    21,    22,   172,   173,   175,   177,   132,   215,
     196,    74,   115,   190,    58,   196,   132,   133,   152,    20,
      21,   223,    88,   161,   165,   165,    58,    93,   121,   132,
     135,    15,   136,   164,    56,    57,   178,    11,    74,    81,
     153,   154,   157,    66,    78,   155,    90,    51,   132,   162,
     163,   164,    35,   214,   165,   173,   174,   165,   216,    58,
      29,   220,   134,   135,   159,   151,   132,   165,    72,   217,
     132,   154,   132,   196
};

#define yyerrok		(yyerrstatus = 0)
#define yyclearin	(yychar = YYEMPTY)
#define YYEMPTY		(-2)
#define YYEOF		0

#define YYACCEPT	goto yyacceptlab
#define YYABORT		goto yyabortlab
#define YYERROR		goto yyerrorlab


/* Like YYERROR except do call yyerror.  This remains here temporarily
   to ease the transition to the new meaning of YYERROR, for GCC.
   Once GCC version 2 has supplanted version 1, this can go.  */

#define YYFAIL		goto yyerrlab

#define YYRECOVERING()  (!!yyerrstatus)

#define YYBACKUP(Token, Value)					\
do								\
  if (yychar == YYEMPTY && yylen == 1)				\
    {								\
      yychar = (Token);						\
      yylval = (Value);						\
      yytoken = YYTRANSLATE (yychar);				\
      YYPOPSTACK (1);						\
      goto yybackup;						\
    }								\
  else								\
    {								\
      yyerror (YY_("syntax error: cannot back up")); \
      YYERROR;							\
    }								\
while (YYID (0))


#define YYTERROR	1
#define YYERRCODE	256


/* YYLLOC_DEFAULT -- Set CURRENT to span from RHS[1] to RHS[N].
   If N is 0, then set CURRENT to the empty location which ends
   the previous symbol: RHS[0] (always defined).  */

#define YYRHSLOC(Rhs, K) ((Rhs)[K])
#ifndef YYLLOC_DEFAULT
# define YYLLOC_DEFAULT(Current, Rhs, N)				\
    do									\
      if (YYID (N))                                                    \
	{								\
	  (Current).first_line   = YYRHSLOC (Rhs, 1).first_line;	\
	  (Current).first_column = YYRHSLOC (Rhs, 1).first_column;	\
	  (Current).last_line    = YYRHSLOC (Rhs, N).last_line;		\
	  (Current).last_column  = YYRHSLOC (Rhs, N).last_column;	\
	}								\
      else								\
	{								\
	  (Current).first_line   = (Current).last_line   =		\
	    YYRHSLOC (Rhs, 0).last_line;				\
	  (Current).first_column = (Current).last_column =		\
	    YYRHSLOC (Rhs, 0).last_column;				\
	}								\
    while (YYID (0))
#endif


/* YY_LOCATION_PRINT -- Print the location on the stream.
   This macro was not mandated originally: define only if we know
   we won't break user code: when these are the locations we know.  */

#ifndef YY_LOCATION_PRINT
# if defined YYLTYPE_IS_TRIVIAL && YYLTYPE_IS_TRIVIAL
#  define YY_LOCATION_PRINT(File, Loc)			\
     fprintf (File, "%d.%d-%d.%d",			\
	      (Loc).first_line, (Loc).first_column,	\
	      (Loc).last_line,  (Loc).last_column)
# else
#  define YY_LOCATION_PRINT(File, Loc) ((void) 0)
# endif
#endif


/* YYLEX -- calling `yylex' with the right arguments.  */

#ifdef YYLEX_PARAM
# define YYLEX yylex (YYLEX_PARAM)
#else
# define YYLEX yylex ()
#endif

/* Enable debugging if requested.  */
#if YYDEBUG

# ifndef YYFPRINTF
#  include <stdio.h> /* INFRINGES ON USER NAME SPACE */
#  define YYFPRINTF fprintf
# endif

# define YYDPRINTF(Args)			\
do {						\
  if (yydebug)					\
    YYFPRINTF Args;				\
} while (YYID (0))

# define YY_SYMBOL_PRINT(Title, Type, Value, Location)			  \
do {									  \
  if (yydebug)								  \
    {									  \
      YYFPRINTF (stderr, "%s ", Title);					  \
      yy_symbol_print (stderr,						  \
		  Type, Value, Location); \
      YYFPRINTF (stderr, "\n");						  \
    }									  \
} while (YYID (0))


/*--------------------------------.
| Print this symbol on YYOUTPUT.  |
`--------------------------------*/

/*ARGSUSED*/
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static void
yy_symbol_value_print (FILE *yyoutput, int yytype, YYSTYPE const * const yyvaluep, YYLTYPE const * const yylocationp)
#else
static void
yy_symbol_value_print (yyoutput, yytype, yyvaluep, yylocationp)
    FILE *yyoutput;
    int yytype;
    YYSTYPE const * const yyvaluep;
    YYLTYPE const * const yylocationp;
#endif
{
  if (!yyvaluep)
    return;
  YYUSE (yylocationp);
# ifdef YYPRINT
  if (yytype < YYNTOKENS)
    YYPRINT (yyoutput, yytoknum[yytype], *yyvaluep);
# else
  YYUSE (yyoutput);
# endif
  switch (yytype)
    {
      default:
	break;
    }
}


/*--------------------------------.
| Print this symbol on YYOUTPUT.  |
`--------------------------------*/

#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static void
yy_symbol_print (FILE *yyoutput, int yytype, YYSTYPE const * const yyvaluep, YYLTYPE const * const yylocationp)
#else
static void
yy_symbol_print (yyoutput, yytype, yyvaluep, yylocationp)
    FILE *yyoutput;
    int yytype;
    YYSTYPE const * const yyvaluep;
    YYLTYPE const * const yylocationp;
#endif
{
  if (yytype < YYNTOKENS)
    YYFPRINTF (yyoutput, "token %s (", yytname[yytype]);
  else
    YYFPRINTF (yyoutput, "nterm %s (", yytname[yytype]);

  YY_LOCATION_PRINT (yyoutput, *yylocationp);
  YYFPRINTF (yyoutput, ": ");
  yy_symbol_value_print (yyoutput, yytype, yyvaluep, yylocationp);
  YYFPRINTF (yyoutput, ")");
}

/*------------------------------------------------------------------.
| yy_stack_print -- Print the state stack from its BOTTOM up to its |
| TOP (included).                                                   |
`------------------------------------------------------------------*/

#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static void
yy_stack_print (yytype_int16 *bottom, yytype_int16 *top)
#else
static void
yy_stack_print (bottom, top)
    yytype_int16 *bottom;
    yytype_int16 *top;
#endif
{
  YYFPRINTF (stderr, "Stack now");
  for (; bottom <= top; ++bottom)
    YYFPRINTF (stderr, " %d", *bottom);
  YYFPRINTF (stderr, "\n");
}

# define YY_STACK_PRINT(Bottom, Top)				\
do {								\
  if (yydebug)							\
    yy_stack_print ((Bottom), (Top));				\
} while (YYID (0))


/*------------------------------------------------.
| Report that the YYRULE is going to be reduced.  |
`------------------------------------------------*/

#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static void
yy_reduce_print (YYSTYPE *yyvsp, YYLTYPE *yylsp, int yyrule)
#else
static void
yy_reduce_print (yyvsp, yylsp, yyrule)
    YYSTYPE *yyvsp;
    YYLTYPE *yylsp;
    int yyrule;
#endif
{
  int yynrhs = yyr2[yyrule];
  int yyi;
  unsigned long int yylno = yyrline[yyrule];
  YYFPRINTF (stderr, "Reducing stack by rule %d (line %lu):\n",
	     yyrule - 1, yylno);
  /* The symbols being reduced.  */
  for (yyi = 0; yyi < yynrhs; yyi++)
    {
      fprintf (stderr, "   $%d = ", yyi + 1);
      yy_symbol_print (stderr, yyrhs[yyprhs[yyrule] + yyi],
		       &(yyvsp[(yyi + 1) - (yynrhs)])
		       , &(yylsp[(yyi + 1) - (yynrhs)])		       );
      fprintf (stderr, "\n");
    }
}

# define YY_REDUCE_PRINT(Rule)		\
do {					\
  if (yydebug)				\
    yy_reduce_print (yyvsp, yylsp, Rule); \
} while (YYID (0))

/* Nonzero means print parse trace.  It is left uninitialized so that
   multiple parsers can coexist.  */
int yydebug;
#else /* !YYDEBUG */
# define YYDPRINTF(Args)
# define YY_SYMBOL_PRINT(Title, Type, Value, Location)
# define YY_STACK_PRINT(Bottom, Top)
# define YY_REDUCE_PRINT(Rule)
#endif /* !YYDEBUG */


/* YYINITDEPTH -- initial size of the parser's stacks.  */
#ifndef	YYINITDEPTH
# define YYINITDEPTH 200
#endif

/* YYMAXDEPTH -- maximum size the stacks can grow to (effective only
   if the built-in stack extension method is used).

   Do not make this value too large; the results are undefined if
   YYSTACK_ALLOC_MAXIMUM < YYSTACK_BYTES (YYMAXDEPTH)
   evaluated with infinite-precision integer arithmetic.  */

#ifndef YYMAXDEPTH
# define YYMAXDEPTH 10000
#endif



#if YYERROR_VERBOSE

# ifndef yystrlen
#  if defined __GLIBC__ && defined _STRING_H
#   define yystrlen strlen
#  else
/* Return the length of YYSTR.  */
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static YYSIZE_T
yystrlen (const char *yystr)
#else
static YYSIZE_T
yystrlen (yystr)
    const char *yystr;
#endif
{
  YYSIZE_T yylen;
  for (yylen = 0; yystr[yylen]; yylen++)
    continue;
  return yylen;
}
#  endif
# endif

# ifndef yystpcpy
#  if defined __GLIBC__ && defined _STRING_H && defined _GNU_SOURCE
#   define yystpcpy stpcpy
#  else
/* Copy YYSRC to YYDEST, returning the address of the terminating '\0' in
   YYDEST.  */
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static char *
yystpcpy (char *yydest, const char *yysrc)
#else
static char *
yystpcpy (yydest, yysrc)
    char *yydest;
    const char *yysrc;
#endif
{
  char *yyd = yydest;
  const char *yys = yysrc;

  while ((*yyd++ = *yys++) != '\0')
    continue;

  return yyd - 1;
}
#  endif
# endif

# ifndef yytnamerr
/* Copy to YYRES the contents of YYSTR after stripping away unnecessary
   quotes and backslashes, so that it's suitable for yyerror.  The
   heuristic is that double-quoting is unnecessary unless the string
   contains an apostrophe, a comma, or backslash (other than
   backslash-backslash).  YYSTR is taken from yytname.  If YYRES is
   null, do not copy; instead, return the length of what the result
   would have been.  */
static YYSIZE_T
yytnamerr (char *yyres, const char *yystr)
{
  if (*yystr == '"')
    {
      YYSIZE_T yyn = 0;
      char const *yyp = yystr;

      for (;;)
	switch (*++yyp)
	  {
	  case '\'':
	  case ',':
	    goto do_not_strip_quotes;

	  case '\\':
	    if (*++yyp != '\\')
	      goto do_not_strip_quotes;
	    /* Fall through.  */
	  default:
	    if (yyres)
	      yyres[yyn] = *yyp;
	    yyn++;
	    break;

	  case '"':
	    if (yyres)
	      yyres[yyn] = '\0';
	    return yyn;
	  }
    do_not_strip_quotes: ;
    }

  if (! yyres)
    return yystrlen (yystr);

  return yystpcpy (yyres, yystr) - yyres;
}
# endif

/* Copy into YYRESULT an error message about the unexpected token
   YYCHAR while in state YYSTATE.  Return the number of bytes copied,
   including the terminating null byte.  If YYRESULT is null, do not
   copy anything; just return the number of bytes that would be
   copied.  As a special case, return 0 if an ordinary "syntax error"
   message will do.  Return YYSIZE_MAXIMUM if overflow occurs during
   size calculation.  */
static YYSIZE_T
yysyntax_error (char *yyresult, int yystate, int yychar)
{
  int yyn = yypact[yystate];

  if (! (YYPACT_NINF < yyn && yyn <= YYLAST))
    return 0;
  else
    {
      int yytype = YYTRANSLATE (yychar);
      YYSIZE_T yysize0 = yytnamerr (0, yytname[yytype]);
      YYSIZE_T yysize = yysize0;
      YYSIZE_T yysize1;
      int yysize_overflow = 0;
      enum { YYERROR_VERBOSE_ARGS_MAXIMUM = 5 };
      char const *yyarg[YYERROR_VERBOSE_ARGS_MAXIMUM];
      int yyx;

# if 0
      /* This is so xgettext sees the translatable formats that are
	 constructed on the fly.  */
      YY_("syntax error, unexpected %s");
      YY_("syntax error, unexpected %s, expecting %s");
      YY_("syntax error, unexpected %s, expecting %s or %s");
      YY_("syntax error, unexpected %s, expecting %s or %s or %s");
      YY_("syntax error, unexpected %s, expecting %s or %s or %s or %s");
# endif
      char *yyfmt;
      char const *yyf;
      static char const yyunexpected[] = "syntax error, unexpected %s";
      static char const yyexpecting[] = ", expecting %s";
      static char const yyor[] = " or %s";
      char yyformat[sizeof yyunexpected
		    + sizeof yyexpecting - 1
		    + ((YYERROR_VERBOSE_ARGS_MAXIMUM - 2)
		       * (sizeof yyor - 1))];
      char const *yyprefix = yyexpecting;

      /* Start YYX at -YYN if negative to avoid negative indexes in
	 YYCHECK.  */
      int yyxbegin = yyn < 0 ? -yyn : 0;

      /* Stay within bounds of both yycheck and yytname.  */
      int yychecklim = YYLAST - yyn + 1;
      int yyxend = yychecklim < YYNTOKENS ? yychecklim : YYNTOKENS;
      int yycount = 1;

      yyarg[0] = yytname[yytype];
      yyfmt = yystpcpy (yyformat, yyunexpected);

      for (yyx = yyxbegin; yyx < yyxend; ++yyx)
	if (yycheck[yyx + yyn] == yyx && yyx != YYTERROR)
	  {
	    if (yycount == YYERROR_VERBOSE_ARGS_MAXIMUM)
	      {
		yycount = 1;
		yysize = yysize0;
		yyformat[sizeof yyunexpected - 1] = '\0';
		break;
	      }
	    yyarg[yycount++] = yytname[yyx];
	    yysize1 = yysize + yytnamerr (0, yytname[yyx]);
	    yysize_overflow |= (yysize1 < yysize);
	    yysize = yysize1;
	    yyfmt = yystpcpy (yyfmt, yyprefix);
	    yyprefix = yyor;
	  }

      yyf = YY_(yyformat);
      yysize1 = yysize + yystrlen (yyf);
      yysize_overflow |= (yysize1 < yysize);
      yysize = yysize1;

      if (yysize_overflow)
	return YYSIZE_MAXIMUM;

      if (yyresult)
	{
	  /* Avoid sprintf, as that infringes on the user's name space.
	     Don't have undefined behavior even if the translation
	     produced a string with the wrong number of "%s"s.  */
	  char *yyp = yyresult;
	  int yyi = 0;
	  while ((*yyp = *yyf) != '\0')
	    {
	      if (*yyp == '%' && yyf[1] == 's' && yyi < yycount)
		{
		  yyp += yytnamerr (yyp, yyarg[yyi++]);
		  yyf += 2;
		}
	      else
		{
		  yyp++;
		  yyf++;
		}
	    }
	}
      return yysize;
    }
}
#endif /* YYERROR_VERBOSE */


/*-----------------------------------------------.
| Release the memory associated to this symbol.  |
`-----------------------------------------------*/

/*ARGSUSED*/
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
static void
yydestruct (const char *yymsg, int yytype, YYSTYPE *yyvaluep, YYLTYPE *yylocationp)
#else
static void
yydestruct (yymsg, yytype, yyvaluep, yylocationp)
    const char *yymsg;
    int yytype;
    YYSTYPE *yyvaluep;
    YYLTYPE *yylocationp;
#endif
{
  YYUSE (yyvaluep);
  YYUSE (yylocationp);

  if (!yymsg)
    yymsg = "Deleting";
  YY_SYMBOL_PRINT (yymsg, yytype, yyvaluep, yylocationp);

  switch (yytype)
    {

      default:
	break;
    }
}


/* Prevent warnings from -Wmissing-prototypes.  */

#ifdef YYPARSE_PARAM
#if defined __STDC__ || defined __cplusplus
int yyparse (void *YYPARSE_PARAM);
#else
int yyparse ();
#endif
#else /* ! YYPARSE_PARAM */
#if defined __STDC__ || defined __cplusplus
int yyparse (void);
#else
int yyparse ();
#endif
#endif /* ! YYPARSE_PARAM */



/* The look-ahead symbol.  */
int yychar;

/* The semantic value of the look-ahead symbol.  */
YYSTYPE yylval;

/* Number of syntax errors so far.  */
int yynerrs;
/* Location data for the look-ahead symbol.  */
YYLTYPE yylloc;



/*----------.
| yyparse.  |
`----------*/

#ifdef YYPARSE_PARAM
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
int
yyparse (void *YYPARSE_PARAM)
#else
int
yyparse (YYPARSE_PARAM)
    void *YYPARSE_PARAM;
#endif
#else /* ! YYPARSE_PARAM */
#if (defined __STDC__ || defined __C99__FUNC__ \
     || defined __cplusplus || defined _MSC_VER)
int
yyparse (void)
#else
int
yyparse ()

#endif
#endif
{
  
  int yystate;
  int yyn;
  int yyresult;
  /* Number of tokens to shift before error messages enabled.  */
  int yyerrstatus;
  /* Look-ahead token as an internal (translated) token number.  */
  int yytoken = 0;
#if YYERROR_VERBOSE
  /* Buffer for error messages, and its allocated size.  */
  char yymsgbuf[128];
  char *yymsg = yymsgbuf;
  YYSIZE_T yymsg_alloc = sizeof yymsgbuf;
#endif

  /* Three stacks and their tools:
     `yyss': related to states,
     `yyvs': related to semantic values,
     `yyls': related to locations.

     Refer to the stacks thru separate pointers, to allow yyoverflow
     to reallocate them elsewhere.  */

  /* The state stack.  */
  yytype_int16 yyssa[YYINITDEPTH];
  yytype_int16 *yyss = yyssa;
  yytype_int16 *yyssp;

  /* The semantic value stack.  */
  YYSTYPE yyvsa[YYINITDEPTH];
  YYSTYPE *yyvs = yyvsa;
  YYSTYPE *yyvsp;

  /* The location stack.  */
  YYLTYPE yylsa[YYINITDEPTH];
  YYLTYPE *yyls = yylsa;
  YYLTYPE *yylsp;
  /* The locations where the error started and ended.  */
  YYLTYPE yyerror_range[2];

#define YYPOPSTACK(N)   (yyvsp -= (N), yyssp -= (N), yylsp -= (N))

  YYSIZE_T yystacksize = YYINITDEPTH;

  /* The variables used to return semantic value and location from the
     action routines.  */
  YYSTYPE yyval;
  YYLTYPE yyloc;

  /* The number of symbols on the RHS of the reduced rule.
     Keep to zero when no symbol should be popped.  */
  int yylen = 0;

  YYDPRINTF ((stderr, "Starting parse\n"));

  yystate = 0;
  yyerrstatus = 0;
  yynerrs = 0;
  yychar = YYEMPTY;		/* Cause a token to be read.  */

  /* Initialize stack pointers.
     Waste one element of value and location stack
     so that they stay on the same level as the state stack.
     The wasted elements are never initialized.  */

  yyssp = yyss;
  yyvsp = yyvs;
  yylsp = yyls;
#if defined YYLTYPE_IS_TRIVIAL && YYLTYPE_IS_TRIVIAL
  /* Initialize the default location before parsing starts.  */
  yylloc.first_line   = yylloc.last_line   = 1;
  yylloc.first_column = yylloc.last_column = 0;
#endif

  goto yysetstate;

/*------------------------------------------------------------.
| yynewstate -- Push a new state, which is found in yystate.  |
`------------------------------------------------------------*/
 yynewstate:
  /* In all cases, when you get here, the value and location stacks
     have just been pushed.  So pushing a state here evens the stacks.  */
  yyssp++;

 yysetstate:
  *yyssp = yystate;

  if (yyss + yystacksize - 1 <= yyssp)
    {
      /* Get the current used size of the three stacks, in elements.  */
      YYSIZE_T yysize = yyssp - yyss + 1;

#ifdef yyoverflow
      {
	/* Give user a chance to reallocate the stack.  Use copies of
	   these so that the &'s don't force the real ones into
	   memory.  */
	YYSTYPE *yyvs1 = yyvs;
	yytype_int16 *yyss1 = yyss;
	YYLTYPE *yyls1 = yyls;

	/* Each stack pointer address is followed by the size of the
	   data in use in that stack, in bytes.  This used to be a
	   conditional around just the two extra args, but that might
	   be undefined if yyoverflow is a macro.  */
	yyoverflow (YY_("memory exhausted"),
		    &yyss1, yysize * sizeof (*yyssp),
		    &yyvs1, yysize * sizeof (*yyvsp),
		    &yyls1, yysize * sizeof (*yylsp),
		    &yystacksize);
	yyls = yyls1;
	yyss = yyss1;
	yyvs = yyvs1;
      }
#else /* no yyoverflow */
# ifndef YYSTACK_RELOCATE
      goto yyexhaustedlab;
# else
      /* Extend the stack our own way.  */
      if (YYMAXDEPTH <= yystacksize)
	goto yyexhaustedlab;
      yystacksize *= 2;
      if (YYMAXDEPTH < yystacksize)
	yystacksize = YYMAXDEPTH;

      {
	yytype_int16 *yyss1 = yyss;
	union yyalloc *yyptr =
	  (union yyalloc *) YYSTACK_ALLOC (YYSTACK_BYTES (yystacksize));
	if (! yyptr)
	  goto yyexhaustedlab;
	YYSTACK_RELOCATE (yyss);
	YYSTACK_RELOCATE (yyvs);
	YYSTACK_RELOCATE (yyls);
#  undef YYSTACK_RELOCATE
	if (yyss1 != yyssa)
	  YYSTACK_FREE (yyss1);
      }
# endif
#endif /* no yyoverflow */

      yyssp = yyss + yysize - 1;
      yyvsp = yyvs + yysize - 1;
      yylsp = yyls + yysize - 1;

      YYDPRINTF ((stderr, "Stack size increased to %lu\n",
		  (unsigned long int) yystacksize));

      if (yyss + yystacksize - 1 <= yyssp)
	YYABORT;
    }

  YYDPRINTF ((stderr, "Entering state %d\n", yystate));

  goto yybackup;

/*-----------.
| yybackup.  |
`-----------*/
yybackup:

  /* Do appropriate processing given the current state.  Read a
     look-ahead token if we need one and don't already have one.  */

  /* First try to decide what to do without reference to look-ahead token.  */
  yyn = yypact[yystate];
  if (yyn == YYPACT_NINF)
    goto yydefault;

  /* Not known => get a look-ahead token if don't already have one.  */

  /* YYCHAR is either YYEMPTY or YYEOF or a valid look-ahead symbol.  */
  if (yychar == YYEMPTY)
    {
      YYDPRINTF ((stderr, "Reading a token: "));
      yychar = YYLEX;
    }

  if (yychar <= YYEOF)
    {
      yychar = yytoken = YYEOF;
      YYDPRINTF ((stderr, "Now at end of input.\n"));
    }
  else
    {
      yytoken = YYTRANSLATE (yychar);
      YY_SYMBOL_PRINT ("Next token is", yytoken, &yylval, &yylloc);
    }

  /* If the proper action on seeing token YYTOKEN is to reduce or to
     detect an error, take that action.  */
  yyn += yytoken;
  if (yyn < 0 || YYLAST < yyn || yycheck[yyn] != yytoken)
    goto yydefault;
  yyn = yytable[yyn];
  if (yyn <= 0)
    {
      if (yyn == 0 || yyn == YYTABLE_NINF)
	goto yyerrlab;
      yyn = -yyn;
      goto yyreduce;
    }

  if (yyn == YYFINAL)
    YYACCEPT;

  /* Count tokens shifted since error; after three, turn off error
     status.  */
  if (yyerrstatus)
    yyerrstatus--;

  /* Shift the look-ahead token.  */
  YY_SYMBOL_PRINT ("Shifting", yytoken, &yylval, &yylloc);

  /* Discard the shifted token unless it is eof.  */
  if (yychar != YYEOF)
    yychar = YYEMPTY;

  yystate = yyn;
  *++yyvsp = yylval;
  *++yylsp = yylloc;
  goto yynewstate;


/*-----------------------------------------------------------.
| yydefault -- do the default action for the current state.  |
`-----------------------------------------------------------*/
yydefault:
  yyn = yydefact[yystate];
  if (yyn == 0)
    goto yyerrlab;
  goto yyreduce;


/*-----------------------------.
| yyreduce -- Do a reduction.  |
`-----------------------------*/
yyreduce:
  /* yyn is the number of a rule to reduce with.  */
  yylen = yyr2[yyn];

  /* If YYLEN is nonzero, implement the default value of the action:
     `$$ = $1'.

     Otherwise, the following line sets YYVAL to garbage.
     This behavior is undocumented and Bison
     users should not rely upon it.  Assigning to YYVAL
     unconditionally makes the parser a bit smaller, and it avoids a
     GCC warning that YYVAL may be used uninitialized.  */
  yyval = yyvsp[1-yylen];

  /* Default location.  */
  YYLLOC_DEFAULT (yyloc, (yylsp - yylen), yylen);
  YY_REDUCE_PRINT (yyn);
  switch (yyn)
    {
        case 2:
#line 360 "pl_gram.y"
    {
						plpgsql_parse_result = (PLpgSQL_stmt_block *) (yyvsp[(2) - (3)].stmt);
					;}
    break;

  case 5:
#line 370 "pl_gram.y"
    {
						plpgsql_DumpExecTree = true;
					;}
    break;

  case 6:
#line 374 "pl_gram.y"
    {
						if (strcmp((yyvsp[(3) - (3)].str), "on") == 0)
							plpgsql_curr_compile->print_strict_params = true;
						else if (strcmp((yyvsp[(3) - (3)].str), "off") == 0)
							plpgsql_curr_compile->print_strict_params = false;
						else
							elog(ERROR, "unrecognized print_strict_params option %s", (yyvsp[(3) - (3)].str));
					;}
    break;

  case 7:
#line 383 "pl_gram.y"
    {
						plpgsql_curr_compile->resolve_option = PLPGSQL_RESOLVE_ERROR;
					;}
    break;

  case 8:
#line 387 "pl_gram.y"
    {
						plpgsql_curr_compile->resolve_option = PLPGSQL_RESOLVE_VARIABLE;
					;}
    break;

  case 9:
#line 391 "pl_gram.y"
    {
						plpgsql_curr_compile->resolve_option = PLPGSQL_RESOLVE_COLUMN;
					;}
    break;

  case 10:
#line 397 "pl_gram.y"
    {
					(yyval.str) = (yyvsp[(1) - (1)].word).ident;
				;}
    break;

  case 11:
#line 401 "pl_gram.y"
    {
					(yyval.str) = pstrdup((yyvsp[(1) - (1)].keyword));
				;}
    break;

  case 14:
#line 410 "pl_gram.y"
    {
						PLpgSQL_stmt_block *new;

						new = palloc0(sizeof(PLpgSQL_stmt_block));

						new->cmd_type	= PLPGSQL_STMT_BLOCK;
						new->lineno		= plpgsql_location_to_lineno((yylsp[(2) - (6)]));
						new->stmtid		= ++plpgsql_curr_compile->nstatements;
						new->label		= (yyvsp[(1) - (6)].declhdr).label;
						new->n_initvars = (yyvsp[(1) - (6)].declhdr).n_initvars;
						new->initvarnos = (yyvsp[(1) - (6)].declhdr).initvarnos;
						new->body		= (yyvsp[(3) - (6)].list);
						new->exceptions	= (yyvsp[(4) - (6)].exception_block);

						check_labels((yyvsp[(1) - (6)].declhdr).label, (yyvsp[(6) - (6)].str), (yylsp[(6) - (6)]));
						plpgsql_ns_pop();

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 15:
#line 433 "pl_gram.y"
    {
						/* done with decls, so resume identifier lookup */
						plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_NORMAL;
						(yyval.declhdr).label	  = (yyvsp[(1) - (1)].str);
						(yyval.declhdr).n_initvars = 0;
						(yyval.declhdr).initvarnos = NULL;
					;}
    break;

  case 16:
#line 441 "pl_gram.y"
    {
						plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_NORMAL;
						(yyval.declhdr).label	  = (yyvsp[(1) - (2)].str);
						(yyval.declhdr).n_initvars = 0;
						(yyval.declhdr).initvarnos = NULL;
					;}
    break;

  case 17:
#line 448 "pl_gram.y"
    {
						plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_NORMAL;
						(yyval.declhdr).label	  = (yyvsp[(1) - (3)].str);
						/* Remember variables declared in decl_stmts */
						(yyval.declhdr).n_initvars = plpgsql_add_initdatums(&((yyval.declhdr).initvarnos));
					;}
    break;

  case 18:
#line 457 "pl_gram.y"
    {
						/* Forget any variables created before block */
						plpgsql_add_initdatums(NULL);
						/*
						 * Disable scanner lookup of identifiers while
						 * we process the decl_stmts
						 */
						plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_DECLARE;
					;}
    break;

  case 22:
#line 474 "pl_gram.y"
    {
						/* We allow useless extra DECLAREs */
					;}
    break;

  case 23:
#line 478 "pl_gram.y"
    {
						/*
						 * Throw a helpful error if user tries to put block
						 * label just before BEGIN, instead of before DECLARE.
						 */
						ereport(ERROR,
								(errcode(ERRCODE_SYNTAX_ERROR),
								 errmsg("block label must be placed before DECLARE, not after"),
								 parser_errposition((yylsp[(1) - (3)]))));
					;}
    break;

  case 24:
#line 491 "pl_gram.y"
    {
						PLpgSQL_variable	*var;

						/*
						 * If a collation is supplied, insert it into the
						 * datatype.  We assume decl_datatype always returns
						 * a freshly built struct not shared with other
						 * variables.
						 */
						if (OidIsValid((yyvsp[(4) - (6)].oid)))
						{
							if (!OidIsValid((yyvsp[(3) - (6)].dtype)->collation))
								ereport(ERROR,
										(errcode(ERRCODE_DATATYPE_MISMATCH),
										 errmsg("collations are not supported by type %s",
												format_type_be((yyvsp[(3) - (6)].dtype)->typoid)),
										 parser_errposition((yylsp[(4) - (6)]))));
							(yyvsp[(3) - (6)].dtype)->collation = (yyvsp[(4) - (6)].oid);
						}

						var = plpgsql_build_variable((yyvsp[(1) - (6)].varname).name, (yyvsp[(1) - (6)].varname).lineno,
													 (yyvsp[(3) - (6)].dtype), true);
						var->isconst = (yyvsp[(2) - (6)].boolean);
						var->notnull = (yyvsp[(5) - (6)].boolean);
						var->default_val = (yyvsp[(6) - (6)].expr);

						/*
						 * The combination of NOT NULL without an initializer
						 * can't work, so let's reject it at compile time.
						 */
						if (var->notnull && var->default_val == NULL)
							ereport(ERROR,
									(errcode(ERRCODE_NULL_VALUE_NOT_ALLOWED),
									 errmsg("variable \"%s\" must have a default value, since it's declared NOT NULL",
											var->refname),
									 parser_errposition((yylsp[(5) - (6)]))));
					;}
    break;

  case 25:
#line 529 "pl_gram.y"
    {
						plpgsql_ns_additem((yyvsp[(4) - (5)].nsitem)->itemtype,
										   (yyvsp[(4) - (5)].nsitem)->itemno, (yyvsp[(1) - (5)].varname).name);
					;}
    break;

  case 26:
#line 534 "pl_gram.y"
    { plpgsql_ns_push((yyvsp[(1) - (3)].varname).name, PLPGSQL_LABEL_OTHER); ;}
    break;

  case 27:
#line 536 "pl_gram.y"
    {
						PLpgSQL_var *new;

						/* pop local namespace for cursor args */
						plpgsql_ns_pop();

						new = (PLpgSQL_var *)
							plpgsql_build_variable((yyvsp[(1) - (7)].varname).name, (yyvsp[(1) - (7)].varname).lineno,
												   plpgsql_build_datatype(REFCURSOROID,
																		  -1,
																		  InvalidOid,
																		  NULL),
												   true);

						new->cursor_explicit_expr = (yyvsp[(7) - (7)].expr);
						if ((yyvsp[(5) - (7)].datum) == NULL)
							new->cursor_explicit_argrow = -1;
						else
							new->cursor_explicit_argrow = (yyvsp[(5) - (7)].datum)->dno;
						new->cursor_options = CURSOR_OPT_FAST_PLAN | (yyvsp[(2) - (7)].ival);
					;}
    break;

  case 28:
#line 560 "pl_gram.y"
    {
						(yyval.ival) = 0;
					;}
    break;

  case 29:
#line 564 "pl_gram.y"
    {
						(yyval.ival) = CURSOR_OPT_NO_SCROLL;
					;}
    break;

  case 30:
#line 568 "pl_gram.y"
    {
						(yyval.ival) = CURSOR_OPT_SCROLL;
					;}
    break;

  case 31:
#line 574 "pl_gram.y"
    {
						(yyval.expr) = read_sql_stmt();
					;}
    break;

  case 32:
#line 580 "pl_gram.y"
    {
						(yyval.datum) = NULL;
					;}
    break;

  case 33:
#line 584 "pl_gram.y"
    {
						PLpgSQL_row *new;
						int			i;
						ListCell   *l;

						new = palloc0(sizeof(PLpgSQL_row));
						new->dtype = PLPGSQL_DTYPE_ROW;
						new->refname = "(unnamed row)";
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->rowtupdesc = NULL;
						new->nfields = list_length((yyvsp[(2) - (3)].list));
						new->fieldnames = palloc(new->nfields * sizeof(char *));
						new->varnos = palloc(new->nfields * sizeof(int));

						i = 0;
						foreach (l, (yyvsp[(2) - (3)].list))
						{
							PLpgSQL_variable *arg = (PLpgSQL_variable *) lfirst(l);
							Assert(!arg->isconst);
							new->fieldnames[i] = arg->refname;
							new->varnos[i] = arg->dno;
							i++;
						}
						list_free((yyvsp[(2) - (3)].list));

						plpgsql_adddatum((PLpgSQL_datum *) new);
						(yyval.datum) = (PLpgSQL_datum *) new;
					;}
    break;

  case 34:
#line 615 "pl_gram.y"
    {
						(yyval.list) = list_make1((yyvsp[(1) - (1)].datum));
					;}
    break;

  case 35:
#line 619 "pl_gram.y"
    {
						(yyval.list) = lappend((yyvsp[(1) - (3)].list), (yyvsp[(3) - (3)].datum));
					;}
    break;

  case 36:
#line 625 "pl_gram.y"
    {
						(yyval.datum) = (PLpgSQL_datum *)
							plpgsql_build_variable((yyvsp[(1) - (2)].varname).name, (yyvsp[(1) - (2)].varname).lineno,
												   (yyvsp[(2) - (2)].dtype), true);
					;}
    break;

  case 39:
#line 636 "pl_gram.y"
    {
						PLpgSQL_nsitem *nsi;

						nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
												(yyvsp[(1) - (1)].word).ident, NULL, NULL,
												NULL);
						if (nsi == NULL)
							ereport(ERROR,
									(errcode(ERRCODE_UNDEFINED_OBJECT),
									 errmsg("variable \"%s\" does not exist",
											(yyvsp[(1) - (1)].word).ident),
									 parser_errposition((yylsp[(1) - (1)]))));
						(yyval.nsitem) = nsi;
					;}
    break;

  case 40:
#line 651 "pl_gram.y"
    {
						PLpgSQL_nsitem *nsi;

						nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
												(yyvsp[(1) - (1)].keyword), NULL, NULL,
												NULL);
						if (nsi == NULL)
							ereport(ERROR,
									(errcode(ERRCODE_UNDEFINED_OBJECT),
									 errmsg("variable \"%s\" does not exist",
											(yyvsp[(1) - (1)].keyword)),
									 parser_errposition((yylsp[(1) - (1)]))));
						(yyval.nsitem) = nsi;
					;}
    break;

  case 41:
#line 666 "pl_gram.y"
    {
						PLpgSQL_nsitem *nsi;

						if (list_length((yyvsp[(1) - (1)].cword).idents) == 2)
							nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
													strVal(linitial((yyvsp[(1) - (1)].cword).idents)),
													strVal(lsecond((yyvsp[(1) - (1)].cword).idents)),
													NULL,
													NULL);
						else if (list_length((yyvsp[(1) - (1)].cword).idents) == 3)
							nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
													strVal(linitial((yyvsp[(1) - (1)].cword).idents)),
													strVal(lsecond((yyvsp[(1) - (1)].cword).idents)),
													strVal(lthird((yyvsp[(1) - (1)].cword).idents)),
													NULL);
						else
							nsi = NULL;
						if (nsi == NULL)
							ereport(ERROR,
									(errcode(ERRCODE_UNDEFINED_OBJECT),
									 errmsg("variable \"%s\" does not exist",
											NameListToString((yyvsp[(1) - (1)].cword).idents)),
									 parser_errposition((yylsp[(1) - (1)]))));
						(yyval.nsitem) = nsi;
					;}
    break;

  case 42:
#line 694 "pl_gram.y"
    {
						(yyval.varname).name = (yyvsp[(1) - (1)].word).ident;
						(yyval.varname).lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						/*
						 * Check to make sure name isn't already declared
						 * in the current block.
						 */
						if (plpgsql_ns_lookup(plpgsql_ns_top(), true,
											  (yyvsp[(1) - (1)].word).ident, NULL, NULL,
											  NULL) != NULL)
							yyerror("duplicate declaration");

						if (plpgsql_curr_compile->extra_warnings & PLPGSQL_XCHECK_SHADOWVAR ||
							plpgsql_curr_compile->extra_errors & PLPGSQL_XCHECK_SHADOWVAR)
						{
							PLpgSQL_nsitem *nsi;
							nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
													(yyvsp[(1) - (1)].word).ident, NULL, NULL, NULL);
							if (nsi != NULL)
								ereport(plpgsql_curr_compile->extra_errors & PLPGSQL_XCHECK_SHADOWVAR ? ERROR : WARNING,
										(errcode(ERRCODE_DUPLICATE_ALIAS),
										 errmsg("variable \"%s\" shadows a previously defined variable",
												(yyvsp[(1) - (1)].word).ident),
										 parser_errposition((yylsp[(1) - (1)]))));
						}

					;}
    break;

  case 43:
#line 722 "pl_gram.y"
    {
						(yyval.varname).name = pstrdup((yyvsp[(1) - (1)].keyword));
						(yyval.varname).lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						/*
						 * Check to make sure name isn't already declared
						 * in the current block.
						 */
						if (plpgsql_ns_lookup(plpgsql_ns_top(), true,
											  (yyvsp[(1) - (1)].keyword), NULL, NULL,
											  NULL) != NULL)
							yyerror("duplicate declaration");

						if (plpgsql_curr_compile->extra_warnings & PLPGSQL_XCHECK_SHADOWVAR ||
							plpgsql_curr_compile->extra_errors & PLPGSQL_XCHECK_SHADOWVAR)
						{
							PLpgSQL_nsitem *nsi;
							nsi = plpgsql_ns_lookup(plpgsql_ns_top(), false,
													(yyvsp[(1) - (1)].keyword), NULL, NULL, NULL);
							if (nsi != NULL)
								ereport(plpgsql_curr_compile->extra_errors & PLPGSQL_XCHECK_SHADOWVAR ? ERROR : WARNING,
										(errcode(ERRCODE_DUPLICATE_ALIAS),
										 errmsg("variable \"%s\" shadows a previously defined variable",
												(yyvsp[(1) - (1)].keyword)),
										 parser_errposition((yylsp[(1) - (1)]))));
						}

					;}
    break;

  case 44:
#line 752 "pl_gram.y"
    { (yyval.boolean) = false; ;}
    break;

  case 45:
#line 754 "pl_gram.y"
    { (yyval.boolean) = true; ;}
    break;

  case 46:
#line 758 "pl_gram.y"
    {
						/*
						 * If there's a lookahead token, read_datatype() will
						 * consume it, and then we must tell bison to forget
						 * it.
						 */
						(yyval.dtype) = read_datatype(yychar);
						yyclearin;
					;}
    break;

  case 47:
#line 770 "pl_gram.y"
    { (yyval.oid) = InvalidOid; ;}
    break;

  case 48:
#line 772 "pl_gram.y"
    {
						(yyval.oid) = get_collation_oid(list_make1(makeString((yyvsp[(2) - (2)].word).ident)),
											   false);
					;}
    break;

  case 49:
#line 777 "pl_gram.y"
    {
						(yyval.oid) = get_collation_oid(list_make1(makeString(pstrdup((yyvsp[(2) - (2)].keyword)))),
											   false);
					;}
    break;

  case 50:
#line 782 "pl_gram.y"
    {
						(yyval.oid) = get_collation_oid((yyvsp[(2) - (2)].cword).idents, false);
					;}
    break;

  case 51:
#line 788 "pl_gram.y"
    { (yyval.boolean) = false; ;}
    break;

  case 52:
#line 790 "pl_gram.y"
    { (yyval.boolean) = true; ;}
    break;

  case 53:
#line 794 "pl_gram.y"
    { (yyval.expr) = NULL; ;}
    break;

  case 54:
#line 796 "pl_gram.y"
    {
						(yyval.expr) = read_sql_expression(';', ";");
					;}
    break;

  case 59:
#line 815 "pl_gram.y"
    { (yyval.list) = NIL; ;}
    break;

  case 60:
#line 817 "pl_gram.y"
    {
						/* don't bother linking null statements into list */
						if ((yyvsp[(2) - (2)].stmt) == NULL)
							(yyval.list) = (yyvsp[(1) - (2)].list);
						else
							(yyval.list) = lappend((yyvsp[(1) - (2)].list), (yyvsp[(2) - (2)].stmt));
					;}
    break;

  case 61:
#line 827 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (2)].stmt); ;}
    break;

  case 62:
#line 829 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 63:
#line 831 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 64:
#line 833 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 65:
#line 835 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 66:
#line 837 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 67:
#line 839 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 68:
#line 841 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 69:
#line 843 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 70:
#line 845 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 71:
#line 847 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 72:
#line 849 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 73:
#line 851 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 74:
#line 853 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 75:
#line 855 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 76:
#line 857 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 77:
#line 859 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 78:
#line 861 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 79:
#line 863 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 80:
#line 865 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 81:
#line 867 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 82:
#line 869 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 83:
#line 871 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 84:
#line 873 "pl_gram.y"
    { (yyval.stmt) = (yyvsp[(1) - (1)].stmt); ;}
    break;

  case 85:
#line 877 "pl_gram.y"
    {
						PLpgSQL_stmt_perform *new;
						int			startloc;

						new = palloc0(sizeof(PLpgSQL_stmt_perform));
						new->cmd_type = PLPGSQL_STMT_PERFORM;
						new->lineno   = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						plpgsql_push_back_token(K_PERFORM);

						/*
						 * Since PERFORM isn't legal SQL, we have to cheat to
						 * the extent of substituting "SELECT" for "PERFORM"
						 * in the parsed text.  It does not seem worth
						 * inventing a separate parse mode for this one case.
						 * We can't do syntax-checking until after we make the
						 * substitution.
						 */
						new->expr = read_sql_construct(';', 0, 0, ";",
													   RAW_PARSE_DEFAULT,
													   false, false,
													   &startloc, NULL);
						/* overwrite "perform" ... */
						memcpy(new->expr->query, " SELECT", 7);
						/* left-justify to get rid of the leading space */
						memmove(new->expr->query, new->expr->query + 1,
								strlen(new->expr->query));
						/* offset syntax error position to account for that */
						check_sql_expr(new->expr->query, new->expr->parseMode,
									   startloc + 1);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 86:
#line 913 "pl_gram.y"
    {
						PLpgSQL_stmt_call *new;

						new = palloc0(sizeof(PLpgSQL_stmt_call));
						new->cmd_type = PLPGSQL_STMT_CALL;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						plpgsql_push_back_token(K_CALL);
						new->expr = read_sql_stmt();
						new->is_call = true;

						/* Remember we may need a procedure resource owner */
						plpgsql_curr_compile->requires_procedure_resowner = true;

						(yyval.stmt) = (PLpgSQL_stmt *) new;

					;}
    break;

  case 87:
#line 931 "pl_gram.y"
    {
						/* use the same structures as for CALL, for simplicity */
						PLpgSQL_stmt_call *new;

						new = palloc0(sizeof(PLpgSQL_stmt_call));
						new->cmd_type = PLPGSQL_STMT_CALL;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						plpgsql_push_back_token(K_DO);
						new->expr = read_sql_stmt();
						new->is_call = false;

						/* Remember we may need a procedure resource owner */
						plpgsql_curr_compile->requires_procedure_resowner = true;

						(yyval.stmt) = (PLpgSQL_stmt *) new;

					;}
    break;

  case 88:
#line 952 "pl_gram.y"
    {
						PLpgSQL_stmt_assign *new;
						RawParseMode pmode;

						/* see how many names identify the datum */
						switch ((yyvsp[(1) - (1)].wdatum).ident ? 1 : list_length((yyvsp[(1) - (1)].wdatum).idents))
						{
							case 1:
								pmode = RAW_PARSE_PLPGSQL_ASSIGN1;
								break;
							case 2:
								pmode = RAW_PARSE_PLPGSQL_ASSIGN2;
								break;
							case 3:
								pmode = RAW_PARSE_PLPGSQL_ASSIGN3;
								break;
							default:
								elog(ERROR, "unexpected number of names");
								pmode = 0; /* keep compiler quiet */
						}

						check_assignable((yyvsp[(1) - (1)].wdatum).datum, (yylsp[(1) - (1)]));
						new = palloc0(sizeof(PLpgSQL_stmt_assign));
						new->cmd_type = PLPGSQL_STMT_ASSIGN;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->varno = (yyvsp[(1) - (1)].wdatum).datum->dno;
						/* Push back the head name to include it in the stmt */
						plpgsql_push_back_token(T_DATUM);
						new->expr = read_sql_construct(';', 0, 0, ";",
													   pmode,
													   false, true,
													   NULL, NULL);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 89:
#line 991 "pl_gram.y"
    {
						PLpgSQL_stmt_getdiag *new;
						ListCell	   *lc;

						new = palloc0(sizeof(PLpgSQL_stmt_getdiag));
						new->cmd_type = PLPGSQL_STMT_GETDIAG;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (5)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->is_stacked = (yyvsp[(2) - (5)].boolean);
						new->diag_items = (yyvsp[(4) - (5)].list);

						/*
						 * Check information items are valid for area option.
						 */
						foreach(lc, new->diag_items)
						{
							PLpgSQL_diag_item *ditem = (PLpgSQL_diag_item *) lfirst(lc);

							switch (ditem->kind)
							{
								/* these fields are disallowed in stacked case */
								case PLPGSQL_GETDIAG_ROW_COUNT:
								case PLPGSQL_GETDIAG_ROUTINE_OID:
									if (new->is_stacked)
										ereport(ERROR,
												(errcode(ERRCODE_SYNTAX_ERROR),
												 errmsg("diagnostics item %s is not allowed in GET STACKED DIAGNOSTICS",
														plpgsql_getdiag_kindname(ditem->kind)),
												 parser_errposition((yylsp[(1) - (5)]))));
									break;
								/* these fields are disallowed in current case */
								case PLPGSQL_GETDIAG_ERROR_CONTEXT:
								case PLPGSQL_GETDIAG_ERROR_DETAIL:
								case PLPGSQL_GETDIAG_ERROR_HINT:
								case PLPGSQL_GETDIAG_RETURNED_SQLSTATE:
								case PLPGSQL_GETDIAG_COLUMN_NAME:
								case PLPGSQL_GETDIAG_CONSTRAINT_NAME:
								case PLPGSQL_GETDIAG_DATATYPE_NAME:
								case PLPGSQL_GETDIAG_MESSAGE_TEXT:
								case PLPGSQL_GETDIAG_TABLE_NAME:
								case PLPGSQL_GETDIAG_SCHEMA_NAME:
									if (!new->is_stacked)
										ereport(ERROR,
												(errcode(ERRCODE_SYNTAX_ERROR),
												 errmsg("diagnostics item %s is not allowed in GET CURRENT DIAGNOSTICS",
														plpgsql_getdiag_kindname(ditem->kind)),
												 parser_errposition((yylsp[(1) - (5)]))));
									break;
								/* these fields are allowed in either case */
								case PLPGSQL_GETDIAG_CONTEXT:
									break;
								default:
									elog(ERROR, "unrecognized diagnostic item kind: %d",
										 ditem->kind);
									break;
							}
						}

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 90:
#line 1054 "pl_gram.y"
    {
						(yyval.boolean) = false;
					;}
    break;

  case 91:
#line 1058 "pl_gram.y"
    {
						(yyval.boolean) = false;
					;}
    break;

  case 92:
#line 1062 "pl_gram.y"
    {
						(yyval.boolean) = true;
					;}
    break;

  case 93:
#line 1068 "pl_gram.y"
    {
						(yyval.list) = lappend((yyvsp[(1) - (3)].list), (yyvsp[(3) - (3)].diagitem));
					;}
    break;

  case 94:
#line 1072 "pl_gram.y"
    {
						(yyval.list) = list_make1((yyvsp[(1) - (1)].diagitem));
					;}
    break;

  case 95:
#line 1078 "pl_gram.y"
    {
						PLpgSQL_diag_item *new;

						new = palloc(sizeof(PLpgSQL_diag_item));
						new->target = (yyvsp[(1) - (3)].datum)->dno;
						new->kind = (yyvsp[(3) - (3)].ival);

						(yyval.diagitem) = new;
					;}
    break;

  case 96:
#line 1090 "pl_gram.y"
    {
						int			tok = yylex();

						if (tok_is_keyword(tok, &yylval,
										   K_ROW_COUNT, "row_count"))
							(yyval.ival) = PLPGSQL_GETDIAG_ROW_COUNT;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_ROUTINE_OID, "pg_routine_oid"))
							(yyval.ival) = PLPGSQL_GETDIAG_ROUTINE_OID;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_CONTEXT, "pg_context"))
							(yyval.ival) = PLPGSQL_GETDIAG_CONTEXT;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_EXCEPTION_DETAIL, "pg_exception_detail"))
							(yyval.ival) = PLPGSQL_GETDIAG_ERROR_DETAIL;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_EXCEPTION_HINT, "pg_exception_hint"))
							(yyval.ival) = PLPGSQL_GETDIAG_ERROR_HINT;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_EXCEPTION_CONTEXT, "pg_exception_context"))
							(yyval.ival) = PLPGSQL_GETDIAG_ERROR_CONTEXT;
						else if (tok_is_keyword(tok, &yylval,
												K_COLUMN_NAME, "column_name"))
							(yyval.ival) = PLPGSQL_GETDIAG_COLUMN_NAME;
						else if (tok_is_keyword(tok, &yylval,
												K_CONSTRAINT_NAME, "constraint_name"))
							(yyval.ival) = PLPGSQL_GETDIAG_CONSTRAINT_NAME;
						else if (tok_is_keyword(tok, &yylval,
												K_PG_DATATYPE_NAME, "pg_datatype_name"))
							(yyval.ival) = PLPGSQL_GETDIAG_DATATYPE_NAME;
						else if (tok_is_keyword(tok, &yylval,
												K_MESSAGE_TEXT, "message_text"))
							(yyval.ival) = PLPGSQL_GETDIAG_MESSAGE_TEXT;
						else if (tok_is_keyword(tok, &yylval,
												K_TABLE_NAME, "table_name"))
							(yyval.ival) = PLPGSQL_GETDIAG_TABLE_NAME;
						else if (tok_is_keyword(tok, &yylval,
												K_SCHEMA_NAME, "schema_name"))
							(yyval.ival) = PLPGSQL_GETDIAG_SCHEMA_NAME;
						else if (tok_is_keyword(tok, &yylval,
												K_RETURNED_SQLSTATE, "returned_sqlstate"))
							(yyval.ival) = PLPGSQL_GETDIAG_RETURNED_SQLSTATE;
						else
							yyerror("unrecognized GET DIAGNOSTICS item");
					;}
    break;

  case 97:
#line 1138 "pl_gram.y"
    {
						/*
						 * In principle we should support a getdiag_target
						 * that is an array element, but for now we don't, so
						 * just throw an error if next token is '['.
						 */
						if ((yyvsp[(1) - (1)].wdatum).datum->dtype == PLPGSQL_DTYPE_ROW ||
							(yyvsp[(1) - (1)].wdatum).datum->dtype == PLPGSQL_DTYPE_REC ||
							plpgsql_peek() == '[')
							ereport(ERROR,
									(errcode(ERRCODE_SYNTAX_ERROR),
									 errmsg("\"%s\" is not a scalar variable",
											NameOfDatum(&((yyvsp[(1) - (1)].wdatum)))),
									 parser_errposition((yylsp[(1) - (1)]))));
						check_assignable((yyvsp[(1) - (1)].wdatum).datum, (yylsp[(1) - (1)]));
						(yyval.datum) = (yyvsp[(1) - (1)].wdatum).datum;
					;}
    break;

  case 98:
#line 1156 "pl_gram.y"
    {
						/* just to give a better message than "syntax error" */
						word_is_not_variable(&((yyvsp[(1) - (1)].word)), (yylsp[(1) - (1)]));
					;}
    break;

  case 99:
#line 1161 "pl_gram.y"
    {
						/* just to give a better message than "syntax error" */
						cword_is_not_variable(&((yyvsp[(1) - (1)].cword)), (yylsp[(1) - (1)]));
					;}
    break;

  case 100:
#line 1168 "pl_gram.y"
    {
						PLpgSQL_stmt_if *new;

						new = palloc0(sizeof(PLpgSQL_stmt_if));
						new->cmd_type = PLPGSQL_STMT_IF;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (8)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->cond = (yyvsp[(2) - (8)].expr);
						new->then_body = (yyvsp[(3) - (8)].list);
						new->elsif_list = (yyvsp[(4) - (8)].list);
						new->else_body = (yyvsp[(5) - (8)].list);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 101:
#line 1185 "pl_gram.y"
    {
						(yyval.list) = NIL;
					;}
    break;

  case 102:
#line 1189 "pl_gram.y"
    {
						PLpgSQL_if_elsif *new;

						new = palloc0(sizeof(PLpgSQL_if_elsif));
						new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (4)]));
						new->cond = (yyvsp[(3) - (4)].expr);
						new->stmts = (yyvsp[(4) - (4)].list);

						(yyval.list) = lappend((yyvsp[(1) - (4)].list), new);
					;}
    break;

  case 103:
#line 1202 "pl_gram.y"
    {
						(yyval.list) = NIL;
					;}
    break;

  case 104:
#line 1206 "pl_gram.y"
    {
						(yyval.list) = (yyvsp[(2) - (2)].list);
					;}
    break;

  case 105:
#line 1212 "pl_gram.y"
    {
						(yyval.stmt) = make_case((yylsp[(1) - (7)]), (yyvsp[(2) - (7)].expr), (yyvsp[(3) - (7)].list), (yyvsp[(4) - (7)].list));
					;}
    break;

  case 106:
#line 1218 "pl_gram.y"
    {
						PLpgSQL_expr *expr = NULL;
						int			tok = yylex();

						if (tok != K_WHEN)
						{
							plpgsql_push_back_token(tok);
							expr = read_sql_expression(K_WHEN, "WHEN");
						}
						plpgsql_push_back_token(K_WHEN);
						(yyval.expr) = expr;
					;}
    break;

  case 107:
#line 1233 "pl_gram.y"
    {
						(yyval.list) = lappend((yyvsp[(1) - (2)].list), (yyvsp[(2) - (2)].casewhen));
					;}
    break;

  case 108:
#line 1237 "pl_gram.y"
    {
						(yyval.list) = list_make1((yyvsp[(1) - (1)].casewhen));
					;}
    break;

  case 109:
#line 1243 "pl_gram.y"
    {
						PLpgSQL_case_when *new = palloc(sizeof(PLpgSQL_case_when));

						new->lineno	= plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->expr = (yyvsp[(2) - (3)].expr);
						new->stmts = (yyvsp[(3) - (3)].list);
						(yyval.casewhen) = new;
					;}
    break;

  case 110:
#line 1254 "pl_gram.y"
    {
						(yyval.list) = NIL;
					;}
    break;

  case 111:
#line 1258 "pl_gram.y"
    {
						/*
						 * proc_sect could return an empty list, but we
						 * must distinguish that from not having ELSE at all.
						 * Simplest fix is to return a list with one NULL
						 * pointer, which make_case() must take care of.
						 */
						if ((yyvsp[(2) - (2)].list) != NIL)
							(yyval.list) = (yyvsp[(2) - (2)].list);
						else
							(yyval.list) = list_make1(NULL);
					;}
    break;

  case 112:
#line 1273 "pl_gram.y"
    {
						PLpgSQL_stmt_loop *new;

						new = palloc0(sizeof(PLpgSQL_stmt_loop));
						new->cmd_type = PLPGSQL_STMT_LOOP;
						new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (3)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->label = (yyvsp[(1) - (3)].str);
						new->body = (yyvsp[(3) - (3)].loop_body).stmts;

						check_labels((yyvsp[(1) - (3)].str), (yyvsp[(3) - (3)].loop_body).end_label, (yyvsp[(3) - (3)].loop_body).end_label_location);
						plpgsql_ns_pop();

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 113:
#line 1291 "pl_gram.y"
    {
						PLpgSQL_stmt_while *new;

						new = palloc0(sizeof(PLpgSQL_stmt_while));
						new->cmd_type = PLPGSQL_STMT_WHILE;
						new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (4)]));
						new->stmtid	= ++plpgsql_curr_compile->nstatements;
						new->label = (yyvsp[(1) - (4)].str);
						new->cond = (yyvsp[(3) - (4)].expr);
						new->body = (yyvsp[(4) - (4)].loop_body).stmts;

						check_labels((yyvsp[(1) - (4)].str), (yyvsp[(4) - (4)].loop_body).end_label, (yyvsp[(4) - (4)].loop_body).end_label_location);
						plpgsql_ns_pop();

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 114:
#line 1310 "pl_gram.y"
    {
						/* This runs after we've scanned the loop body */
						if ((yyvsp[(3) - (4)].stmt)->cmd_type == PLPGSQL_STMT_FORI)
						{
							PLpgSQL_stmt_fori *new;

							new = (PLpgSQL_stmt_fori *) (yyvsp[(3) - (4)].stmt);
							new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (4)]));
							new->label = (yyvsp[(1) - (4)].str);
							new->body = (yyvsp[(4) - (4)].loop_body).stmts;
							(yyval.stmt) = (PLpgSQL_stmt *) new;
						}
						else
						{
							PLpgSQL_stmt_forq *new;

							Assert((yyvsp[(3) - (4)].stmt)->cmd_type == PLPGSQL_STMT_FORS ||
								   (yyvsp[(3) - (4)].stmt)->cmd_type == PLPGSQL_STMT_FORC ||
								   (yyvsp[(3) - (4)].stmt)->cmd_type == PLPGSQL_STMT_DYNFORS);
							/* forq is the common supertype of all three */
							new = (PLpgSQL_stmt_forq *) (yyvsp[(3) - (4)].stmt);
							new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (4)]));
							new->label = (yyvsp[(1) - (4)].str);
							new->body = (yyvsp[(4) - (4)].loop_body).stmts;
							(yyval.stmt) = (PLpgSQL_stmt *) new;
						}

						check_labels((yyvsp[(1) - (4)].str), (yyvsp[(4) - (4)].loop_body).end_label, (yyvsp[(4) - (4)].loop_body).end_label_location);
						/* close namespace started in opt_loop_label */
						plpgsql_ns_pop();
					;}
    break;

  case 115:
#line 1344 "pl_gram.y"
    {
						int			tok = yylex();
						int			tokloc = yylloc;

						if (tok == K_EXECUTE)
						{
							/* EXECUTE means it's a dynamic FOR loop */
							PLpgSQL_stmt_dynfors *new;
							PLpgSQL_expr *expr;
							int			term;

							expr = read_sql_expression2(K_LOOP, K_USING,
														"LOOP or USING",
														&term);

							new = palloc0(sizeof(PLpgSQL_stmt_dynfors));
							new->cmd_type = PLPGSQL_STMT_DYNFORS;
							new->stmtid = ++plpgsql_curr_compile->nstatements;
							if ((yyvsp[(1) - (2)].forvariable).row)
							{
								new->var = (PLpgSQL_variable *) (yyvsp[(1) - (2)].forvariable).row;
								check_assignable((yyvsp[(1) - (2)].forvariable).row, (yylsp[(1) - (2)]));
							}
							else if ((yyvsp[(1) - (2)].forvariable).scalar)
							{
								/* convert single scalar to list */
								new->var = (PLpgSQL_variable *)
									make_scalar_list1((yyvsp[(1) - (2)].forvariable).name, (yyvsp[(1) - (2)].forvariable).scalar,
													  (yyvsp[(1) - (2)].forvariable).lineno, (yylsp[(1) - (2)]));
								/* make_scalar_list1 did check_assignable */
							}
							else
							{
								ereport(ERROR,
										(errcode(ERRCODE_DATATYPE_MISMATCH),
										 errmsg("loop variable of loop over rows must be a record variable or list of scalar variables"),
										 parser_errposition((yylsp[(1) - (2)]))));
							}
							new->query = expr;

							if (term == K_USING)
							{
								do
								{
									expr = read_sql_expression2(',', K_LOOP,
																", or LOOP",
																&term);
									new->params = lappend(new->params, expr);
								} while (term == ',');
							}

							(yyval.stmt) = (PLpgSQL_stmt *) new;
						}
						else if (tok == T_DATUM &&
								 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_VAR &&
								 ((PLpgSQL_var *) yylval.wdatum.datum)->datatype->typoid == REFCURSOROID)
						{
							/* It's FOR var IN cursor */
							PLpgSQL_stmt_forc *new;
							PLpgSQL_var	*cursor = (PLpgSQL_var *) yylval.wdatum.datum;

							new = (PLpgSQL_stmt_forc *) palloc0(sizeof(PLpgSQL_stmt_forc));
							new->cmd_type = PLPGSQL_STMT_FORC;
							new->stmtid = ++plpgsql_curr_compile->nstatements;
							new->curvar = cursor->dno;

							/* Should have had a single variable name */
							if ((yyvsp[(1) - (2)].forvariable).scalar && (yyvsp[(1) - (2)].forvariable).row)
								ereport(ERROR,
										(errcode(ERRCODE_SYNTAX_ERROR),
										 errmsg("cursor FOR loop must have only one target variable"),
										 parser_errposition((yylsp[(1) - (2)]))));

							/* can't use an unbound cursor this way */
							if (cursor->cursor_explicit_expr == NULL)
								ereport(ERROR,
										(errcode(ERRCODE_SYNTAX_ERROR),
										 errmsg("cursor FOR loop must use a bound cursor variable"),
										 parser_errposition(tokloc)));

							/* collect cursor's parameters if any */
							new->argquery = read_cursor_args(cursor,
															 K_LOOP);

							/* create loop's private RECORD variable */
							new->var = (PLpgSQL_variable *)
								plpgsql_build_record((yyvsp[(1) - (2)].forvariable).name,
													 (yyvsp[(1) - (2)].forvariable).lineno,
													 NULL,
													 RECORDOID,
													 true);

							(yyval.stmt) = (PLpgSQL_stmt *) new;
						}
						else
						{
							PLpgSQL_expr *expr1;
							int			expr1loc;
							bool		reverse = false;

							/*
							 * We have to distinguish between two
							 * alternatives: FOR var IN a .. b and FOR
							 * var IN query. Unfortunately this is
							 * tricky, since the query in the second
							 * form needn't start with a SELECT
							 * keyword.  We use the ugly hack of
							 * looking for two periods after the first
							 * token. We also check for the REVERSE
							 * keyword, which means it must be an
							 * integer loop.
							 */
							if (tok_is_keyword(tok, &yylval,
											   K_REVERSE, "reverse"))
								reverse = true;
							else
								plpgsql_push_back_token(tok);

							/*
							 * Read tokens until we see either a ".."
							 * or a LOOP.  The text we read may be either
							 * an expression or a whole SQL statement, so
							 * we need to invoke read_sql_construct directly,
							 * and tell it not to check syntax yet.
							 */
							expr1 = read_sql_construct(DOT_DOT,
													   K_LOOP,
													   0,
													   "LOOP",
													   RAW_PARSE_DEFAULT,
													   true,
													   false,
													   &expr1loc,
													   &tok);

							if (tok == DOT_DOT)
							{
								/* Saw "..", so it must be an integer loop */
								PLpgSQL_expr *expr2;
								PLpgSQL_expr *expr_by;
								PLpgSQL_var	*fvar;
								PLpgSQL_stmt_fori *new;

								/*
								 * Relabel first expression as an expression;
								 * then we can check its syntax.
								 */
								expr1->parseMode = RAW_PARSE_PLPGSQL_EXPR;
								check_sql_expr(expr1->query, expr1->parseMode,
											   expr1loc);

								/* Read and check the second one */
								expr2 = read_sql_expression2(K_LOOP, K_BY,
															 "LOOP",
															 &tok);

								/* Get the BY clause if any */
								if (tok == K_BY)
									expr_by = read_sql_expression(K_LOOP,
																  "LOOP");
								else
									expr_by = NULL;

								/* Should have had a single variable name */
								if ((yyvsp[(1) - (2)].forvariable).scalar && (yyvsp[(1) - (2)].forvariable).row)
									ereport(ERROR,
											(errcode(ERRCODE_SYNTAX_ERROR),
											 errmsg("integer FOR loop must have only one target variable"),
											 parser_errposition((yylsp[(1) - (2)]))));

								/* create loop's private variable */
								fvar = (PLpgSQL_var *)
									plpgsql_build_variable((yyvsp[(1) - (2)].forvariable).name,
														   (yyvsp[(1) - (2)].forvariable).lineno,
														   plpgsql_build_datatype(INT4OID,
																				  -1,
																				  InvalidOid,
																				  NULL),
														   true);

								new = palloc0(sizeof(PLpgSQL_stmt_fori));
								new->cmd_type = PLPGSQL_STMT_FORI;
								new->stmtid	= ++plpgsql_curr_compile->nstatements;
								new->var = fvar;
								new->reverse = reverse;
								new->lower = expr1;
								new->upper = expr2;
								new->step = expr_by;

								(yyval.stmt) = (PLpgSQL_stmt *) new;
							}
							else
							{
								/*
								 * No "..", so it must be a query loop.
								 */
								PLpgSQL_stmt_fors *new;

								if (reverse)
									ereport(ERROR,
											(errcode(ERRCODE_SYNTAX_ERROR),
											 errmsg("cannot specify REVERSE in query FOR loop"),
											 parser_errposition(tokloc)));

								/* Check syntax as a regular query */
								check_sql_expr(expr1->query, expr1->parseMode,
											   expr1loc);

								new = palloc0(sizeof(PLpgSQL_stmt_fors));
								new->cmd_type = PLPGSQL_STMT_FORS;
								new->stmtid = ++plpgsql_curr_compile->nstatements;
								if ((yyvsp[(1) - (2)].forvariable).row)
								{
									new->var = (PLpgSQL_variable *) (yyvsp[(1) - (2)].forvariable).row;
									check_assignable((yyvsp[(1) - (2)].forvariable).row, (yylsp[(1) - (2)]));
								}
								else if ((yyvsp[(1) - (2)].forvariable).scalar)
								{
									/* convert single scalar to list */
									new->var = (PLpgSQL_variable *)
										make_scalar_list1((yyvsp[(1) - (2)].forvariable).name, (yyvsp[(1) - (2)].forvariable).scalar,
														  (yyvsp[(1) - (2)].forvariable).lineno, (yylsp[(1) - (2)]));
									/* make_scalar_list1 did check_assignable */
								}
								else
								{
									ereport(ERROR,
											(errcode(ERRCODE_SYNTAX_ERROR),
											 errmsg("loop variable of loop over rows must be a record variable or list of scalar variables"),
											 parser_errposition((yylsp[(1) - (2)]))));
								}

								new->query = expr1;
								(yyval.stmt) = (PLpgSQL_stmt *) new;
							}
						}
					;}
    break;

  case 116:
#line 1602 "pl_gram.y"
    {
						(yyval.forvariable).name = NameOfDatum(&((yyvsp[(1) - (1)].wdatum)));
						(yyval.forvariable).lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						if ((yyvsp[(1) - (1)].wdatum).datum->dtype == PLPGSQL_DTYPE_ROW ||
							(yyvsp[(1) - (1)].wdatum).datum->dtype == PLPGSQL_DTYPE_REC)
						{
							(yyval.forvariable).scalar = NULL;
							(yyval.forvariable).row = (yyvsp[(1) - (1)].wdatum).datum;
						}
						else
						{
							int			tok;

							(yyval.forvariable).scalar = (yyvsp[(1) - (1)].wdatum).datum;
							(yyval.forvariable).row = NULL;
							/* check for comma-separated list */
							tok = yylex();
							plpgsql_push_back_token(tok);
							if (tok == ',')
								(yyval.forvariable).row = (PLpgSQL_datum *)
									read_into_scalar_list((yyval.forvariable).name,
														  (yyval.forvariable).scalar,
														  (yylsp[(1) - (1)]));
						}
					;}
    break;

  case 117:
#line 1628 "pl_gram.y"
    {
						int			tok;

						(yyval.forvariable).name = (yyvsp[(1) - (1)].word).ident;
						(yyval.forvariable).lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						(yyval.forvariable).scalar = NULL;
						(yyval.forvariable).row = NULL;
						/* check for comma-separated list */
						tok = yylex();
						plpgsql_push_back_token(tok);
						if (tok == ',')
							word_is_not_variable(&((yyvsp[(1) - (1)].word)), (yylsp[(1) - (1)]));
					;}
    break;

  case 118:
#line 1642 "pl_gram.y"
    {
						/* just to give a better message than "syntax error" */
						cword_is_not_variable(&((yyvsp[(1) - (1)].cword)), (yylsp[(1) - (1)]));
					;}
    break;

  case 119:
#line 1649 "pl_gram.y"
    {
						PLpgSQL_stmt_foreach_a *new;

						new = palloc0(sizeof(PLpgSQL_stmt_foreach_a));
						new->cmd_type = PLPGSQL_STMT_FOREACH_A;
						new->lineno = plpgsql_location_to_lineno((yylsp[(2) - (8)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->label = (yyvsp[(1) - (8)].str);
						new->slice = (yyvsp[(4) - (8)].ival);
						new->expr = (yyvsp[(7) - (8)].expr);
						new->body = (yyvsp[(8) - (8)].loop_body).stmts;

						if ((yyvsp[(3) - (8)].forvariable).row)
						{
							new->varno = (yyvsp[(3) - (8)].forvariable).row->dno;
							check_assignable((yyvsp[(3) - (8)].forvariable).row, (yylsp[(3) - (8)]));
						}
						else if ((yyvsp[(3) - (8)].forvariable).scalar)
						{
							new->varno = (yyvsp[(3) - (8)].forvariable).scalar->dno;
							check_assignable((yyvsp[(3) - (8)].forvariable).scalar, (yylsp[(3) - (8)]));
						}
						else
						{
							ereport(ERROR,
									(errcode(ERRCODE_SYNTAX_ERROR),
									 errmsg("loop variable of FOREACH must be a known variable or list of variables"),
											 parser_errposition((yylsp[(3) - (8)]))));
						}

						check_labels((yyvsp[(1) - (8)].str), (yyvsp[(8) - (8)].loop_body).end_label, (yyvsp[(8) - (8)].loop_body).end_label_location);
						plpgsql_ns_pop();

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 120:
#line 1687 "pl_gram.y"
    {
						(yyval.ival) = 0;
					;}
    break;

  case 121:
#line 1691 "pl_gram.y"
    {
						(yyval.ival) = (yyvsp[(2) - (2)].ival);
					;}
    break;

  case 122:
#line 1697 "pl_gram.y"
    {
						PLpgSQL_stmt_exit *new;

						new = palloc0(sizeof(PLpgSQL_stmt_exit));
						new->cmd_type = PLPGSQL_STMT_EXIT;
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->is_exit = (yyvsp[(1) - (3)].boolean);
						new->lineno	= plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->label = (yyvsp[(2) - (3)].str);
						new->cond = (yyvsp[(3) - (3)].expr);

						if ((yyvsp[(2) - (3)].str))
						{
							/* We have a label, so verify it exists */
							PLpgSQL_nsitem *label;

							label = plpgsql_ns_lookup_label(plpgsql_ns_top(), (yyvsp[(2) - (3)].str));
							if (label == NULL)
								ereport(ERROR,
										(errcode(ERRCODE_SYNTAX_ERROR),
										 errmsg("there is no label \"%s\" "
												"attached to any block or loop enclosing this statement",
												(yyvsp[(2) - (3)].str)),
										 parser_errposition((yylsp[(2) - (3)]))));
							/* CONTINUE only allows loop labels */
							if (label->itemno != PLPGSQL_LABEL_LOOP && !new->is_exit)
								ereport(ERROR,
										(errcode(ERRCODE_SYNTAX_ERROR),
										 errmsg("block label \"%s\" cannot be used in CONTINUE",
												(yyvsp[(2) - (3)].str)),
										 parser_errposition((yylsp[(2) - (3)]))));
						}
						else
						{
							/*
							 * No label, so make sure there is some loop (an
							 * unlabeled EXIT does not match a block, so this
							 * is the same test for both EXIT and CONTINUE)
							 */
							if (plpgsql_ns_find_nearest_loop(plpgsql_ns_top()) == NULL)
								ereport(ERROR,
										(errcode(ERRCODE_SYNTAX_ERROR),
										 new->is_exit ?
										 errmsg("EXIT cannot be used outside a loop, unless it has a label") :
										 errmsg("CONTINUE cannot be used outside a loop"),
										 parser_errposition((yylsp[(1) - (3)]))));
						}

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 123:
#line 1750 "pl_gram.y"
    {
						(yyval.boolean) = true;
					;}
    break;

  case 124:
#line 1754 "pl_gram.y"
    {
						(yyval.boolean) = false;
					;}
    break;

  case 125:
#line 1760 "pl_gram.y"
    {
						int			tok;

						tok = yylex();
						if (tok == 0)
							yyerror("unexpected end of function definition");

						if (tok_is_keyword(tok, &yylval,
										   K_NEXT, "next"))
						{
							(yyval.stmt) = make_return_next_stmt((yylsp[(1) - (1)]));
						}
						else if (tok_is_keyword(tok, &yylval,
												K_QUERY, "query"))
						{
							(yyval.stmt) = make_return_query_stmt((yylsp[(1) - (1)]));
						}
						else
						{
							plpgsql_push_back_token(tok);
							(yyval.stmt) = make_return_stmt((yylsp[(1) - (1)]));
						}
					;}
    break;

  case 126:
#line 1786 "pl_gram.y"
    {
						PLpgSQL_stmt_raise *new;
						int			tok;

						new = palloc(sizeof(PLpgSQL_stmt_raise));

						new->cmd_type = PLPGSQL_STMT_RAISE;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid	= ++plpgsql_curr_compile->nstatements;
						new->elog_level = ERROR;	/* default */
						new->condname = NULL;
						new->message = NULL;
						new->params = NIL;
						new->options = NIL;

						tok = yylex();
						if (tok == 0)
							yyerror("unexpected end of function definition");

						/*
						 * We could have just RAISE, meaning to re-throw
						 * the current error.
						 */
						if (tok != ';')
						{
							/*
							 * First is an optional elog severity level.
							 */
							if (tok_is_keyword(tok, &yylval,
											   K_EXCEPTION, "exception"))
							{
								new->elog_level = ERROR;
								tok = yylex();
							}
							else if (tok_is_keyword(tok, &yylval,
													K_WARNING, "warning"))
							{
								new->elog_level = WARNING;
								tok = yylex();
							}
							else if (tok_is_keyword(tok, &yylval,
													K_NOTICE, "notice"))
							{
								new->elog_level = NOTICE;
								tok = yylex();
							}
							else if (tok_is_keyword(tok, &yylval,
													K_INFO, "info"))
							{
								new->elog_level = INFO;
								tok = yylex();
							}
							else if (tok_is_keyword(tok, &yylval,
													K_LOG, "log"))
							{
								new->elog_level = LOG;
								tok = yylex();
							}
							else if (tok_is_keyword(tok, &yylval,
													K_DEBUG, "debug"))
							{
								new->elog_level = DEBUG1;
								tok = yylex();
							}
							if (tok == 0)
								yyerror("unexpected end of function definition");

							/*
							 * Next we can have a condition name, or
							 * equivalently SQLSTATE 'xxxxx', or a string
							 * literal that is the old-style message format,
							 * or USING to start the option list immediately.
							 */
							if (tok == SCONST)
							{
								/* old style message and parameters */
								new->message = yylval.str;
								/*
								 * We expect either a semi-colon, which
								 * indicates no parameters, or a comma that
								 * begins the list of parameter expressions,
								 * or USING to begin the options list.
								 */
								tok = yylex();
								if (tok != ',' && tok != ';' && tok != K_USING)
									yyerror("syntax error");

								while (tok == ',')
								{
									PLpgSQL_expr *expr;

									expr = read_sql_construct(',', ';', K_USING,
															  ", or ; or USING",
															  RAW_PARSE_PLPGSQL_EXPR,
															  true, true,
															  NULL, &tok);
									new->params = lappend(new->params, expr);
								}
							}
							else if (tok != K_USING)
							{
								/* must be condition name or SQLSTATE */
								if (tok_is_keyword(tok, &yylval,
												   K_SQLSTATE, "sqlstate"))
								{
									/* next token should be a string literal */
									char	   *sqlstatestr;

									if (yylex() != SCONST)
										yyerror("syntax error");
									sqlstatestr = yylval.str;

									if (strlen(sqlstatestr) != 5)
										yyerror("invalid SQLSTATE code");
									if (strspn(sqlstatestr, "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ") != 5)
										yyerror("invalid SQLSTATE code");
									new->condname = sqlstatestr;
								}
								else
								{
									if (tok == T_WORD)
										new->condname = yylval.word.ident;
									else if (plpgsql_token_is_unreserved_keyword(tok))
										new->condname = pstrdup(yylval.keyword);
									else
										yyerror("syntax error");
									plpgsql_recognize_err_condition(new->condname,
																	false);
								}
								tok = yylex();
								if (tok != ';' && tok != K_USING)
									yyerror("syntax error");
							}

							if (tok == K_USING)
								new->options = read_raise_options();
						}

						check_raise_parameters(new);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 127:
#line 1931 "pl_gram.y"
    {
						PLpgSQL_stmt_assert	*new;
						int			tok;

						new = palloc(sizeof(PLpgSQL_stmt_assert));

						new->cmd_type = PLPGSQL_STMT_ASSERT;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;

						new->cond = read_sql_expression2(',', ';',
														 ", or ;",
														 &tok);

						if (tok == ',')
							new->message = read_sql_expression(';', ";");
						else
							new->message = NULL;

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 128:
#line 1955 "pl_gram.y"
    {
						(yyval.loop_body).stmts = (yyvsp[(1) - (5)].list);
						(yyval.loop_body).end_label = (yyvsp[(4) - (5)].str);
						(yyval.loop_body).end_label_location = (yylsp[(4) - (5)]);
					;}
    break;

  case 129:
#line 1973 "pl_gram.y"
    {
						(yyval.stmt) = make_execsql_stmt(K_IMPORT, (yylsp[(1) - (1)]), NULL);
					;}
    break;

  case 130:
#line 1977 "pl_gram.y"
    {
						(yyval.stmt) = make_execsql_stmt(K_INSERT, (yylsp[(1) - (1)]), NULL);
					;}
    break;

  case 131:
#line 1981 "pl_gram.y"
    {
						(yyval.stmt) = make_execsql_stmt(K_MERGE, (yylsp[(1) - (1)]), NULL);
					;}
    break;

  case 132:
#line 1985 "pl_gram.y"
    {
						int			tok;

						tok = yylex();
						plpgsql_push_back_token(tok);
						if (tok == '=' || tok == COLON_EQUALS ||
							tok == '[' || tok == '.')
							word_is_not_variable(&((yyvsp[(1) - (1)].word)), (yylsp[(1) - (1)]));
						(yyval.stmt) = make_execsql_stmt(T_WORD, (yylsp[(1) - (1)]), &((yyvsp[(1) - (1)].word)));
					;}
    break;

  case 133:
#line 1996 "pl_gram.y"
    {
						int			tok;

						tok = yylex();
						plpgsql_push_back_token(tok);
						if (tok == '=' || tok == COLON_EQUALS ||
							tok == '[' || tok == '.')
							cword_is_not_variable(&((yyvsp[(1) - (1)].cword)), (yylsp[(1) - (1)]));
						(yyval.stmt) = make_execsql_stmt(T_CWORD, (yylsp[(1) - (1)]), NULL);
					;}
    break;

  case 134:
#line 2009 "pl_gram.y"
    {
						PLpgSQL_stmt_dynexecute *new;
						PLpgSQL_expr *expr;
						int			endtoken;

						expr = read_sql_construct(K_INTO, K_USING, ';',
												  "INTO or USING or ;",
												  RAW_PARSE_PLPGSQL_EXPR,
												  true, true,
												  NULL, &endtoken);

						new = palloc(sizeof(PLpgSQL_stmt_dynexecute));
						new->cmd_type = PLPGSQL_STMT_DYNEXECUTE;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->query = expr;
						new->into = false;
						new->strict = false;
						new->target = NULL;
						new->params = NIL;

						/*
						 * We loop to allow the INTO and USING clauses to
						 * appear in either order, since people easily get
						 * that wrong.  This coding also prevents "INTO foo"
						 * from getting absorbed into a USING expression,
						 * which is *really* confusing.
						 */
						for (;;)
						{
							if (endtoken == K_INTO)
							{
								if (new->into)			/* multiple INTO */
									yyerror("syntax error");
								new->into = true;
								read_into_target(&new->target, &new->strict);
								endtoken = yylex();
							}
							else if (endtoken == K_USING)
							{
								if (new->params)		/* multiple USING */
									yyerror("syntax error");
								do
								{
									expr = read_sql_construct(',', ';', K_INTO,
															  ", or ; or INTO",
															  RAW_PARSE_PLPGSQL_EXPR,
															  true, true,
															  NULL, &endtoken);
									new->params = lappend(new->params, expr);
								} while (endtoken == ',');
							}
							else if (endtoken == ';')
								break;
							else
								yyerror("syntax error");
						}

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 135:
#line 2073 "pl_gram.y"
    {
						PLpgSQL_stmt_open *new;
						int			tok;

						new = palloc0(sizeof(PLpgSQL_stmt_open));
						new->cmd_type = PLPGSQL_STMT_OPEN;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (2)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->curvar = (yyvsp[(2) - (2)].var)->dno;
						new->cursor_options = CURSOR_OPT_FAST_PLAN;

						if ((yyvsp[(2) - (2)].var)->cursor_explicit_expr == NULL)
						{
							/* be nice if we could use opt_scrollable here */
							tok = yylex();
							if (tok_is_keyword(tok, &yylval,
											   K_NO, "no"))
							{
								tok = yylex();
								if (tok_is_keyword(tok, &yylval,
												   K_SCROLL, "scroll"))
								{
									new->cursor_options |= CURSOR_OPT_NO_SCROLL;
									tok = yylex();
								}
							}
							else if (tok_is_keyword(tok, &yylval,
													K_SCROLL, "scroll"))
							{
								new->cursor_options |= CURSOR_OPT_SCROLL;
								tok = yylex();
							}

							if (tok != K_FOR)
								yyerror("syntax error, expected \"FOR\"");

							tok = yylex();
							if (tok == K_EXECUTE)
							{
								int			endtoken;

								new->dynquery =
									read_sql_expression2(K_USING, ';',
														 "USING or ;",
														 &endtoken);

								/* If we found "USING", collect argument(s) */
								if (endtoken == K_USING)
								{
									PLpgSQL_expr *expr;

									do
									{
										expr = read_sql_expression2(',', ';',
																	", or ;",
																	&endtoken);
										new->params = lappend(new->params,
															  expr);
									} while (endtoken == ',');
								}
							}
							else
							{
								plpgsql_push_back_token(tok);
								new->query = read_sql_stmt();
							}
						}
						else
						{
							/* predefined cursor query, so read args */
							new->argquery = read_cursor_args((yyvsp[(2) - (2)].var), ';');
						}

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 136:
#line 2151 "pl_gram.y"
    {
						PLpgSQL_stmt_fetch *fetch = (yyvsp[(2) - (4)].fetch);
						PLpgSQL_variable *target;

						/* We have already parsed everything through the INTO keyword */
						read_into_target(&target, NULL);

						if (yylex() != ';')
							yyerror("syntax error");

						/*
						 * We don't allow multiple rows in PL/pgSQL's FETCH
						 * statement, only in MOVE.
						 */
						if (fetch->returns_multiple_rows)
							ereport(ERROR,
									(errcode(ERRCODE_FEATURE_NOT_SUPPORTED),
									 errmsg("FETCH statement cannot return multiple rows"),
									 parser_errposition((yylsp[(1) - (4)]))));

						fetch->lineno = plpgsql_location_to_lineno((yylsp[(1) - (4)]));
						fetch->target	= target;
						fetch->curvar	= (yyvsp[(3) - (4)].var)->dno;
						fetch->is_move	= false;

						(yyval.stmt) = (PLpgSQL_stmt *) fetch;
					;}
    break;

  case 137:
#line 2181 "pl_gram.y"
    {
						PLpgSQL_stmt_fetch *fetch = (yyvsp[(2) - (4)].fetch);

						fetch->lineno = plpgsql_location_to_lineno((yylsp[(1) - (4)]));
						fetch->curvar = (yyvsp[(3) - (4)].var)->dno;
						fetch->is_move = true;

						(yyval.stmt) = (PLpgSQL_stmt *) fetch;
					;}
    break;

  case 138:
#line 2193 "pl_gram.y"
    {
						(yyval.fetch) = read_fetch_direction();
					;}
    break;

  case 139:
#line 2199 "pl_gram.y"
    {
						PLpgSQL_stmt_close *new;

						new = palloc(sizeof(PLpgSQL_stmt_close));
						new->cmd_type = PLPGSQL_STMT_CLOSE;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->curvar = (yyvsp[(2) - (3)].var)->dno;

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 140:
#line 2213 "pl_gram.y"
    {
						/* We do not bother building a node for NULL */
						(yyval.stmt) = NULL;
					;}
    break;

  case 141:
#line 2220 "pl_gram.y"
    {
						PLpgSQL_stmt_commit *new;

						new = palloc(sizeof(PLpgSQL_stmt_commit));
						new->cmd_type = PLPGSQL_STMT_COMMIT;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->chain = (yyvsp[(2) - (3)].ival);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 142:
#line 2234 "pl_gram.y"
    {
						PLpgSQL_stmt_rollback *new;

						new = palloc(sizeof(PLpgSQL_stmt_rollback));
						new->cmd_type = PLPGSQL_STMT_ROLLBACK;
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (3)]));
						new->stmtid = ++plpgsql_curr_compile->nstatements;
						new->chain = (yyvsp[(2) - (3)].ival);

						(yyval.stmt) = (PLpgSQL_stmt *) new;
					;}
    break;

  case 143:
#line 2248 "pl_gram.y"
    { (yyval.ival) = true; ;}
    break;

  case 144:
#line 2249 "pl_gram.y"
    { (yyval.ival) = false; ;}
    break;

  case 145:
#line 2250 "pl_gram.y"
    { (yyval.ival) = false; ;}
    break;

  case 146:
#line 2255 "pl_gram.y"
    {
						/*
						 * In principle we should support a cursor_variable
						 * that is an array element, but for now we don't, so
						 * just throw an error if next token is '['.
						 */
						if ((yyvsp[(1) - (1)].wdatum).datum->dtype != PLPGSQL_DTYPE_VAR ||
							plpgsql_peek() == '[')
							ereport(ERROR,
									(errcode(ERRCODE_DATATYPE_MISMATCH),
									 errmsg("cursor variable must be a simple variable"),
									 parser_errposition((yylsp[(1) - (1)]))));

						if (((PLpgSQL_var *) (yyvsp[(1) - (1)].wdatum).datum)->datatype->typoid != REFCURSOROID)
							ereport(ERROR,
									(errcode(ERRCODE_DATATYPE_MISMATCH),
									 errmsg("variable \"%s\" must be of type cursor or refcursor",
											((PLpgSQL_var *) (yyvsp[(1) - (1)].wdatum).datum)->refname),
									 parser_errposition((yylsp[(1) - (1)]))));
						(yyval.var) = (PLpgSQL_var *) (yyvsp[(1) - (1)].wdatum).datum;
					;}
    break;

  case 147:
#line 2277 "pl_gram.y"
    {
						/* just to give a better message than "syntax error" */
						word_is_not_variable(&((yyvsp[(1) - (1)].word)), (yylsp[(1) - (1)]));
					;}
    break;

  case 148:
#line 2282 "pl_gram.y"
    {
						/* just to give a better message than "syntax error" */
						cword_is_not_variable(&((yyvsp[(1) - (1)].cword)), (yylsp[(1) - (1)]));
					;}
    break;

  case 149:
#line 2289 "pl_gram.y"
    { (yyval.exception_block) = NULL; ;}
    break;

  case 150:
#line 2291 "pl_gram.y"
    {
						/*
						 * We use a mid-rule action to add these
						 * special variables to the namespace before
						 * parsing the WHEN clauses themselves.  The
						 * scope of the names extends to the end of the
						 * current block.
						 */
						int			lineno = plpgsql_location_to_lineno((yylsp[(1) - (1)]));
						PLpgSQL_exception_block *new = palloc(sizeof(PLpgSQL_exception_block));
						PLpgSQL_variable *var;

						var = plpgsql_build_variable("sqlstate", lineno,
													 plpgsql_build_datatype(TEXTOID,
																			-1,
																			plpgsql_curr_compile->fn_input_collation,
																			NULL),
													 true);
						var->isconst = true;
						new->sqlstate_varno = var->dno;

						var = plpgsql_build_variable("sqlerrm", lineno,
													 plpgsql_build_datatype(TEXTOID,
																			-1,
																			plpgsql_curr_compile->fn_input_collation,
																			NULL),
													 true);
						var->isconst = true;
						new->sqlerrm_varno = var->dno;

						(yyval.exception_block) = new;
					;}
    break;

  case 151:
#line 2324 "pl_gram.y"
    {
						PLpgSQL_exception_block *new = (yyvsp[(2) - (3)].exception_block);
						new->exc_list = (yyvsp[(3) - (3)].list);

						(yyval.exception_block) = new;
					;}
    break;

  case 152:
#line 2333 "pl_gram.y"
    {
							(yyval.list) = lappend((yyvsp[(1) - (2)].list), (yyvsp[(2) - (2)].exception));
						;}
    break;

  case 153:
#line 2337 "pl_gram.y"
    {
							(yyval.list) = list_make1((yyvsp[(1) - (1)].exception));
						;}
    break;

  case 154:
#line 2343 "pl_gram.y"
    {
						PLpgSQL_exception *new;

						new = palloc0(sizeof(PLpgSQL_exception));
						new->lineno = plpgsql_location_to_lineno((yylsp[(1) - (4)]));
						new->conditions = (yyvsp[(2) - (4)].condition);
						new->action = (yyvsp[(4) - (4)].list);

						(yyval.exception) = new;
					;}
    break;

  case 155:
#line 2356 "pl_gram.y"
    {
							PLpgSQL_condition	*old;

							for (old = (yyvsp[(1) - (3)].condition); old->next != NULL; old = old->next)
								/* skip */ ;
							old->next = (yyvsp[(3) - (3)].condition);
							(yyval.condition) = (yyvsp[(1) - (3)].condition);
						;}
    break;

  case 156:
#line 2365 "pl_gram.y"
    {
							(yyval.condition) = (yyvsp[(1) - (1)].condition);
						;}
    break;

  case 157:
#line 2371 "pl_gram.y"
    {
							if (strcmp((yyvsp[(1) - (1)].str), "sqlstate") != 0)
							{
								(yyval.condition) = plpgsql_parse_err_condition((yyvsp[(1) - (1)].str));
							}
							else
							{
								PLpgSQL_condition *new;
								char   *sqlstatestr;

								/* next token should be a string literal */
								if (yylex() != SCONST)
									yyerror("syntax error");
								sqlstatestr = yylval.str;

								if (strlen(sqlstatestr) != 5)
									yyerror("invalid SQLSTATE code");
								if (strspn(sqlstatestr, "0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ") != 5)
									yyerror("invalid SQLSTATE code");

								new = palloc(sizeof(PLpgSQL_condition));
								new->sqlerrstate =
									MAKE_SQLSTATE(sqlstatestr[0],
												  sqlstatestr[1],
												  sqlstatestr[2],
												  sqlstatestr[3],
												  sqlstatestr[4]);
								new->condname = sqlstatestr;
								new->next = NULL;

								(yyval.condition) = new;
							}
						;}
    break;

  case 158:
#line 2407 "pl_gram.y"
    { (yyval.expr) = read_sql_expression(';', ";"); ;}
    break;

  case 159:
#line 2411 "pl_gram.y"
    { (yyval.expr) = read_sql_expression(K_THEN, "THEN"); ;}
    break;

  case 160:
#line 2415 "pl_gram.y"
    { (yyval.expr) = read_sql_expression(K_LOOP, "LOOP"); ;}
    break;

  case 161:
#line 2419 "pl_gram.y"
    {
						plpgsql_ns_push(NULL, PLPGSQL_LABEL_BLOCK);
						(yyval.str) = NULL;
					;}
    break;

  case 162:
#line 2424 "pl_gram.y"
    {
						plpgsql_ns_push((yyvsp[(2) - (3)].str), PLPGSQL_LABEL_BLOCK);
						(yyval.str) = (yyvsp[(2) - (3)].str);
					;}
    break;

  case 163:
#line 2431 "pl_gram.y"
    {
						plpgsql_ns_push(NULL, PLPGSQL_LABEL_LOOP);
						(yyval.str) = NULL;
					;}
    break;

  case 164:
#line 2436 "pl_gram.y"
    {
						plpgsql_ns_push((yyvsp[(2) - (3)].str), PLPGSQL_LABEL_LOOP);
						(yyval.str) = (yyvsp[(2) - (3)].str);
					;}
    break;

  case 165:
#line 2443 "pl_gram.y"
    {
						(yyval.str) = NULL;
					;}
    break;

  case 166:
#line 2447 "pl_gram.y"
    {
						/* label validity will be checked by outer production */
						(yyval.str) = (yyvsp[(1) - (1)].str);
					;}
    break;

  case 167:
#line 2454 "pl_gram.y"
    { (yyval.expr) = NULL; ;}
    break;

  case 168:
#line 2456 "pl_gram.y"
    { (yyval.expr) = (yyvsp[(2) - (2)].expr); ;}
    break;

  case 169:
#line 2463 "pl_gram.y"
    {
						(yyval.str) = (yyvsp[(1) - (1)].word).ident;
					;}
    break;

  case 170:
#line 2467 "pl_gram.y"
    {
						(yyval.str) = pstrdup((yyvsp[(1) - (1)].keyword));
					;}
    break;

  case 171:
#line 2471 "pl_gram.y"
    {
						if ((yyvsp[(1) - (1)].wdatum).ident == NULL) /* composite name not OK */
							yyerror("syntax error");
						(yyval.str) = (yyvsp[(1) - (1)].wdatum).ident;
					;}
    break;


/* Line 1267 of yacc.c.  */
#line 4735 "pl_gram.c"
      default: break;
    }
  YY_SYMBOL_PRINT ("-> $$ =", yyr1[yyn], &yyval, &yyloc);

  YYPOPSTACK (yylen);
  yylen = 0;
  YY_STACK_PRINT (yyss, yyssp);

  *++yyvsp = yyval;
  *++yylsp = yyloc;

  /* Now `shift' the result of the reduction.  Determine what state
     that goes to, based on the state we popped back to and the rule
     number reduced by.  */

  yyn = yyr1[yyn];

  yystate = yypgoto[yyn - YYNTOKENS] + *yyssp;
  if (0 <= yystate && yystate <= YYLAST && yycheck[yystate] == *yyssp)
    yystate = yytable[yystate];
  else
    yystate = yydefgoto[yyn - YYNTOKENS];

  goto yynewstate;


/*------------------------------------.
| yyerrlab -- here on detecting error |
`------------------------------------*/
yyerrlab:
  /* If not already recovering from an error, report this error.  */
  if (!yyerrstatus)
    {
      ++yynerrs;
#if ! YYERROR_VERBOSE
      yyerror (YY_("syntax error"));
#else
      {
	YYSIZE_T yysize = yysyntax_error (0, yystate, yychar);
	if (yymsg_alloc < yysize && yymsg_alloc < YYSTACK_ALLOC_MAXIMUM)
	  {
	    YYSIZE_T yyalloc = 2 * yysize;
	    if (! (yysize <= yyalloc && yyalloc <= YYSTACK_ALLOC_MAXIMUM))
	      yyalloc = YYSTACK_ALLOC_MAXIMUM;
	    if (yymsg != yymsgbuf)
	      YYSTACK_FREE (yymsg);
	    yymsg = (char *) YYSTACK_ALLOC (yyalloc);
	    if (yymsg)
	      yymsg_alloc = yyalloc;
	    else
	      {
		yymsg = yymsgbuf;
		yymsg_alloc = sizeof yymsgbuf;
	      }
	  }

	if (0 < yysize && yysize <= yymsg_alloc)
	  {
	    (void) yysyntax_error (yymsg, yystate, yychar);
	    yyerror (yymsg);
	  }
	else
	  {
	    yyerror (YY_("syntax error"));
	    if (yysize != 0)
	      goto yyexhaustedlab;
	  }
      }
#endif
    }

  yyerror_range[0] = yylloc;

  if (yyerrstatus == 3)
    {
      /* If just tried and failed to reuse look-ahead token after an
	 error, discard it.  */

      if (yychar <= YYEOF)
	{
	  /* Return failure if at end of input.  */
	  if (yychar == YYEOF)
	    YYABORT;
	}
      else
	{
	  yydestruct ("Error: discarding",
		      yytoken, &yylval, &yylloc);
	  yychar = YYEMPTY;
	}
    }

  /* Else will try to reuse look-ahead token after shifting the error
     token.  */
  goto yyerrlab1;


/*---------------------------------------------------.
| yyerrorlab -- error raised explicitly by YYERROR.  |
`---------------------------------------------------*/
yyerrorlab:

  /* Pacify compilers like GCC when the user code never invokes
     YYERROR and the label yyerrorlab therefore never appears in user
     code.  */
  if (/*CONSTCOND*/ 0)
     goto yyerrorlab;

  yyerror_range[0] = yylsp[1-yylen];
  /* Do not reclaim the symbols of the rule which action triggered
     this YYERROR.  */
  YYPOPSTACK (yylen);
  yylen = 0;
  YY_STACK_PRINT (yyss, yyssp);
  yystate = *yyssp;
  goto yyerrlab1;


/*-------------------------------------------------------------.
| yyerrlab1 -- common code for both syntax error and YYERROR.  |
`-------------------------------------------------------------*/
yyerrlab1:
  yyerrstatus = 3;	/* Each real token shifted decrements this.  */

  for (;;)
    {
      yyn = yypact[yystate];
      if (yyn != YYPACT_NINF)
	{
	  yyn += YYTERROR;
	  if (0 <= yyn && yyn <= YYLAST && yycheck[yyn] == YYTERROR)
	    {
	      yyn = yytable[yyn];
	      if (0 < yyn)
		break;
	    }
	}

      /* Pop the current state because it cannot handle the error token.  */
      if (yyssp == yyss)
	YYABORT;

      yyerror_range[0] = *yylsp;
      yydestruct ("Error: popping",
		  yystos[yystate], yyvsp, yylsp);
      YYPOPSTACK (1);
      yystate = *yyssp;
      YY_STACK_PRINT (yyss, yyssp);
    }

  if (yyn == YYFINAL)
    YYACCEPT;

  *++yyvsp = yylval;

  yyerror_range[1] = yylloc;
  /* Using YYLLOC is tempting, but would change the location of
     the look-ahead.  YYLOC is available though.  */
  YYLLOC_DEFAULT (yyloc, (yyerror_range - 1), 2);
  *++yylsp = yyloc;

  /* Shift the error token.  */
  YY_SYMBOL_PRINT ("Shifting", yystos[yyn], yyvsp, yylsp);

  yystate = yyn;
  goto yynewstate;


/*-------------------------------------.
| yyacceptlab -- YYACCEPT comes here.  |
`-------------------------------------*/
yyacceptlab:
  yyresult = 0;
  goto yyreturn;

/*-----------------------------------.
| yyabortlab -- YYABORT comes here.  |
`-----------------------------------*/
yyabortlab:
  yyresult = 1;
  goto yyreturn;

#ifndef yyoverflow
/*-------------------------------------------------.
| yyexhaustedlab -- memory exhaustion comes here.  |
`-------------------------------------------------*/
yyexhaustedlab:
  yyerror (YY_("memory exhausted"));
  yyresult = 2;
  /* Fall through.  */
#endif

yyreturn:
  if (yychar != YYEOF && yychar != YYEMPTY)
     yydestruct ("Cleanup: discarding lookahead",
		 yytoken, &yylval, &yylloc);
  /* Do not reclaim the symbols of the rule which action triggered
     this YYABORT or YYACCEPT.  */
  YYPOPSTACK (yylen);
  YY_STACK_PRINT (yyss, yyssp);
  while (yyssp != yyss)
    {
      yydestruct ("Cleanup: popping",
		  yystos[*yyssp], yyvsp, yylsp);
      YYPOPSTACK (1);
    }
#ifndef yyoverflow
  if (yyss != yyssa)
    YYSTACK_FREE (yyss);
#endif
#if YYERROR_VERBOSE
  if (yymsg != yymsgbuf)
    YYSTACK_FREE (yymsg);
#endif
  /* Make sure YYID is used.  */
  return YYID (yyresult);
}


#line 2563 "pl_gram.y"


/*
 * Check whether a token represents an "unreserved keyword".
 * We have various places where we want to recognize a keyword in preference
 * to a variable name, but not reserve that keyword in other contexts.
 * Hence, this kluge.
 */
static bool
tok_is_keyword(int token, union YYSTYPE *lval,
			   int kw_token, const char *kw_str)
{
	if (token == kw_token)
	{
		/* Normal case, was recognized by scanner (no conflicting variable) */
		return true;
	}
	else if (token == T_DATUM)
	{
		/*
		 * It's a variable, so recheck the string name.  Note we will not
		 * match composite names (hence an unreserved word followed by "."
		 * will not be recognized).
		 */
		if (!lval->wdatum.quoted && lval->wdatum.ident != NULL &&
			strcmp(lval->wdatum.ident, kw_str) == 0)
			return true;
	}
	return false;				/* not the keyword */
}

/*
 * Convenience routine to complain when we expected T_DATUM and got T_WORD,
 * ie, unrecognized variable.
 */
static void
word_is_not_variable(PLword *word, int location)
{
	ereport(ERROR,
			(errcode(ERRCODE_SYNTAX_ERROR),
			 errmsg("\"%s\" is not a known variable",
					word->ident),
			 parser_errposition(location)));
}

/* Same, for a CWORD */
static void
cword_is_not_variable(PLcword *cword, int location)
{
	ereport(ERROR,
			(errcode(ERRCODE_SYNTAX_ERROR),
			 errmsg("\"%s\" is not a known variable",
					NameListToString(cword->idents)),
			 parser_errposition(location)));
}

/*
 * Convenience routine to complain when we expected T_DATUM and got
 * something else.  "tok" must be the current token, since we also
 * look at yylval and yylloc.
 */
static void
current_token_is_not_variable(int tok)
{
	if (tok == T_WORD)
		word_is_not_variable(&(yylval.word), yylloc);
	else if (tok == T_CWORD)
		cword_is_not_variable(&(yylval.cword), yylloc);
	else
		yyerror("syntax error");
}

/* Convenience routine to read an expression with one possible terminator */
static PLpgSQL_expr *
read_sql_expression(int until, const char *expected)
{
	return read_sql_construct(until, 0, 0, expected,
							  RAW_PARSE_PLPGSQL_EXPR,
							  true, true, NULL, NULL);
}

/* Convenience routine to read an expression with two possible terminators */
static PLpgSQL_expr *
read_sql_expression2(int until, int until2, const char *expected,
					 int *endtoken)
{
	return read_sql_construct(until, until2, 0, expected,
							  RAW_PARSE_PLPGSQL_EXPR,
							  true, true, NULL, endtoken);
}

/* Convenience routine to read a SQL statement that must end with ';' */
static PLpgSQL_expr *
read_sql_stmt(void)
{
	return read_sql_construct(';', 0, 0, ";",
							  RAW_PARSE_DEFAULT,
							  false, true, NULL, NULL);
}

/*
 * Read a SQL construct and build a PLpgSQL_expr for it.
 *
 * until:		token code for expected terminator
 * until2:		token code for alternate terminator (pass 0 if none)
 * until3:		token code for another alternate terminator (pass 0 if none)
 * expected:	text to use in complaining that terminator was not found
 * parsemode:	raw_parser() mode to use
 * isexpression: whether to say we're reading an "expression" or a "statement"
 * valid_sql:   whether to check the syntax of the expr
 * startloc:	if not NULL, location of first token is stored at *startloc
 * endtoken:	if not NULL, ending token is stored at *endtoken
 *				(this is only interesting if until2 or until3 isn't zero)
 */
static PLpgSQL_expr *
read_sql_construct(int until,
				   int until2,
				   int until3,
				   const char *expected,
				   RawParseMode parsemode,
				   bool isexpression,
				   bool valid_sql,
				   int *startloc,
				   int *endtoken)
{
	int			tok;
	StringInfoData ds;
	IdentifierLookup save_IdentifierLookup;
	int			startlocation = -1;
	int			endlocation = -1;
	int			parenlevel = 0;
	PLpgSQL_expr *expr;

	initStringInfo(&ds);

	/* special lookup mode for identifiers within the SQL text */
	save_IdentifierLookup = plpgsql_IdentifierLookup;
	plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_EXPR;

	for (;;)
	{
		tok = yylex();
		if (startlocation < 0)			/* remember loc of first token */
			startlocation = yylloc;
		if (tok == until && parenlevel == 0)
			break;
		if (tok == until2 && parenlevel == 0)
			break;
		if (tok == until3 && parenlevel == 0)
			break;
		if (tok == '(' || tok == '[')
			parenlevel++;
		else if (tok == ')' || tok == ']')
		{
			parenlevel--;
			if (parenlevel < 0)
				yyerror("mismatched parentheses");
		}
		/*
		 * End of function definition is an error, and we don't expect to
		 * hit a semicolon either (unless it's the until symbol, in which
		 * case we should have fallen out above).
		 */
		if (tok == 0 || tok == ';')
		{
			if (parenlevel != 0)
				yyerror("mismatched parentheses");
			if (isexpression)
				ereport(ERROR,
						(errcode(ERRCODE_SYNTAX_ERROR),
						 errmsg("missing \"%s\" at end of SQL expression",
								expected),
						 parser_errposition(yylloc)));
			else
				ereport(ERROR,
						(errcode(ERRCODE_SYNTAX_ERROR),
						 errmsg("missing \"%s\" at end of SQL statement",
								expected),
						 parser_errposition(yylloc)));
		}
		/* Remember end+1 location of last accepted token */
		endlocation = yylloc + plpgsql_token_length();
	}

	plpgsql_IdentifierLookup = save_IdentifierLookup;

	if (startloc)
		*startloc = startlocation;
	if (endtoken)
		*endtoken = tok;

	/* give helpful complaint about empty input */
	if (startlocation >= endlocation)
	{
		if (isexpression)
			yyerror("missing expression");
		else
			yyerror("missing SQL statement");
	}

	/*
	 * We save only the text from startlocation to endlocation-1.  This
	 * suppresses the "until" token as well as any whitespace or comments
	 * following the last accepted token.  (We used to strip such trailing
	 * whitespace by hand, but that causes problems if there's a "-- comment"
	 * in front of said whitespace.)
	 */
	plpgsql_append_source_text(&ds, startlocation, endlocation);

	expr = palloc0(sizeof(PLpgSQL_expr));
	expr->query = pstrdup(ds.data);
	expr->parseMode = parsemode;
	expr->plan = NULL;
	expr->paramnos = NULL;
	expr->target_param = -1;
	expr->ns = plpgsql_ns_top();
	pfree(ds.data);

	if (valid_sql)
		check_sql_expr(expr->query, expr->parseMode, startlocation);

	return expr;
}

/*
 * Read a datatype declaration, consuming the current lookahead token if any.
 * Returns a PLpgSQL_type struct.
 */
static PLpgSQL_type *
read_datatype(int tok)
{
	StringInfoData ds;
	char	   *type_name;
	int			startlocation;
	PLpgSQL_type *result = NULL;
	int			parenlevel = 0;

	/* Should only be called while parsing DECLARE sections */
	Assert(plpgsql_IdentifierLookup == IDENTIFIER_LOOKUP_DECLARE);

	/* Often there will be a lookahead token, but if not, get one */
	if (tok == YYEMPTY)
		tok = yylex();

	/* The current token is the start of what we'll pass to parse_datatype */
	startlocation = yylloc;

	/*
	 * If we have a simple or composite identifier, check for %TYPE and
	 * %ROWTYPE constructs.
	 */
	if (tok == T_WORD)
	{
		char	   *dtname = yylval.word.ident;

		tok = yylex();
		if (tok == '%')
		{
			tok = yylex();
			if (tok_is_keyword(tok, &yylval,
							   K_TYPE, "type"))
				result = plpgsql_parse_wordtype(dtname);
			else if (tok_is_keyword(tok, &yylval,
									K_ROWTYPE, "rowtype"))
				result = plpgsql_parse_wordrowtype(dtname);
		}
	}
	else if (plpgsql_token_is_unreserved_keyword(tok))
	{
		char	   *dtname = pstrdup(yylval.keyword);

		tok = yylex();
		if (tok == '%')
		{
			tok = yylex();
			if (tok_is_keyword(tok, &yylval,
							   K_TYPE, "type"))
				result = plpgsql_parse_wordtype(dtname);
			else if (tok_is_keyword(tok, &yylval,
									K_ROWTYPE, "rowtype"))
				result = plpgsql_parse_wordrowtype(dtname);
		}
	}
	else if (tok == T_CWORD)
	{
		List	   *dtnames = yylval.cword.idents;

		tok = yylex();
		if (tok == '%')
		{
			tok = yylex();
			if (tok_is_keyword(tok, &yylval,
							   K_TYPE, "type"))
				result = plpgsql_parse_cwordtype(dtnames);
			else if (tok_is_keyword(tok, &yylval,
									K_ROWTYPE, "rowtype"))
				result = plpgsql_parse_cwordrowtype(dtnames);
		}
	}

	/*
	 * If we recognized a %TYPE or %ROWTYPE construct, see if it is followed
	 * by array decoration: [ ARRAY ] [ '[' [ iconst ] ']' [ ... ] ]
	 *
	 * Like the core parser, we ignore the specific numbers and sizes of
	 * dimensions; arrays of different dimensionality are still the same type
	 * in Postgres.
	 */
	if (result)
	{
		bool		is_array = false;

		tok = yylex();
		if (tok_is_keyword(tok, &yylval,
						   K_ARRAY, "array"))
		{
			is_array = true;
			tok = yylex();
		}
		while (tok == '[')
		{
			is_array = true;
			tok = yylex();
			if (tok == ICONST)
				tok = yylex();
			if (tok != ']')
				yyerror("syntax error, expected \"]\"");
			tok = yylex();
		}
		plpgsql_push_back_token(tok);

		if (is_array)
			result = plpgsql_build_datatype_arrayof(result);

		return result;
	}

	/*
	 * Not %TYPE or %ROWTYPE, so scan to the end of the datatype declaration,
	 * which could include typmod or array decoration.  We are not very picky
	 * here, instead relying on parse_datatype to complain about garbage.  But
	 * we must count parens to handle typmods within cursor_arg correctly.
	 */
	while (tok != ';')
	{
		if (tok == 0)
		{
			if (parenlevel != 0)
				yyerror("mismatched parentheses");
			else
				yyerror("incomplete data type declaration");
		}
		/* Possible followers for datatype in a declaration */
		if (tok == K_COLLATE || tok == K_NOT ||
			tok == '=' || tok == COLON_EQUALS || tok == K_DEFAULT)
			break;
		/* Possible followers for datatype in a cursor_arg list */
		if ((tok == ',' || tok == ')') && parenlevel == 0)
			break;
		if (tok == '(')
			parenlevel++;
		else if (tok == ')')
			parenlevel--;

		tok = yylex();
	}

	/* set up ds to contain complete typename text */
	initStringInfo(&ds);
	plpgsql_append_source_text(&ds, startlocation, yylloc);
	type_name = ds.data;

	if (type_name[0] == '\0')
		yyerror("missing data type declaration");

	result = parse_datatype(type_name, startlocation);

	pfree(ds.data);

	plpgsql_push_back_token(tok);

	return result;
}

/*
 * Read a generic SQL statement.  We have already read its first token;
 * firsttoken is that token's code and location its starting location.
 * If firsttoken == T_WORD, pass its yylval value as "word", else pass NULL.
 */
static PLpgSQL_stmt *
make_execsql_stmt(int firsttoken, int location, PLword *word)
{
	StringInfoData ds;
	IdentifierLookup save_IdentifierLookup;
	PLpgSQL_stmt_execsql *execsql;
	PLpgSQL_expr *expr;
	PLpgSQL_variable *target = NULL;
	int			tok;
	int			prev_tok;
	bool		have_into = false;
	bool		have_strict = false;
	int			into_start_loc = -1;
	int			into_end_loc = -1;
	int			paren_depth = 0;
	int			begin_depth = 0;
	bool		in_routine_definition = false;
	int			token_count = 0;
	char		tokens[4];		/* records the first few tokens */

	initStringInfo(&ds);

	memset(tokens, 0, sizeof(tokens));

	/* special lookup mode for identifiers within the SQL text */
	save_IdentifierLookup = plpgsql_IdentifierLookup;
	plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_EXPR;

	/*
	 * Scan to the end of the SQL command.  Identify any INTO-variables
	 * clause lurking within it, and parse that via read_into_target().
	 *
	 * The end of the statement is defined by a semicolon ... except that
	 * semicolons within parentheses or BEGIN/END blocks don't terminate a
	 * statement.  We follow psql's lead in not recognizing BEGIN/END except
	 * after CREATE [OR REPLACE] {FUNCTION|PROCEDURE}.  END can also appear
	 * within a CASE construct, so we treat CASE/END like BEGIN/END.
	 *
	 * Because INTO is sometimes used in the main SQL grammar, we have to be
	 * careful not to take any such usage of INTO as a PL/pgSQL INTO clause.
	 * There are currently three such cases:
	 *
	 * 1. SELECT ... INTO.  We don't care, we just override that with the
	 * PL/pgSQL definition.
	 *
	 * 2. INSERT INTO.  This is relatively easy to recognize since the words
	 * must appear adjacently; but we can't assume INSERT starts the command,
	 * because it can appear in CREATE RULE or WITH.  Unfortunately, INSERT is
	 * *not* fully reserved, so that means there is a chance of a false match;
	 * but it's not very likely.
	 *
	 * 3. IMPORT FOREIGN SCHEMA ... INTO.  This is not allowed in CREATE RULE
	 * or WITH, so we just check for IMPORT as the command's first token.
	 * (If IMPORT FOREIGN SCHEMA returned data someone might wish to capture
	 * with an INTO-variables clause, we'd have to work much harder here.)
	 *
	 * Fortunately, INTO is a fully reserved word in the main grammar, so
	 * at least we need not worry about it appearing as an identifier.
	 *
	 * Any future additional uses of INTO in the main grammar will doubtless
	 * break this logic again ... beware!
	 */
	tok = firsttoken;
	if (tok == T_WORD && strcmp(word->ident, "create") == 0)
		tokens[token_count] = 'c';
	token_count++;

	for (;;)
	{
		prev_tok = tok;
		tok = yylex();
		if (have_into && into_end_loc < 0)
			into_end_loc = yylloc;		/* token after the INTO part */
		/* Detect CREATE [OR REPLACE] {FUNCTION|PROCEDURE} */
		if (tokens[0] == 'c' && token_count < sizeof(tokens))
		{
			if (tok == K_OR)
				tokens[token_count] = 'o';
			else if (tok == T_WORD &&
					 strcmp(yylval.word.ident, "replace") == 0)
				tokens[token_count] = 'r';
			else if (tok == T_WORD &&
					 strcmp(yylval.word.ident, "function") == 0)
				tokens[token_count] = 'f';
			else if (tok == T_WORD &&
					 strcmp(yylval.word.ident, "procedure") == 0)
				tokens[token_count] = 'f';	/* treat same as "function" */
			if (tokens[1] == 'f' ||
				(tokens[1] == 'o' && tokens[2] == 'r' && tokens[3] == 'f'))
				in_routine_definition = true;
			token_count++;
		}
		/* Track paren nesting (needed for CREATE RULE syntax) */
		if (tok == '(')
			paren_depth++;
		else if (tok == ')' && paren_depth > 0)
			paren_depth--;
		/* We need track BEGIN/END nesting only in a routine definition */
		if (in_routine_definition && paren_depth == 0)
		{
			if (tok == K_BEGIN || tok == K_CASE)
				begin_depth++;
			else if (tok == K_END && begin_depth > 0)
				begin_depth--;
		}
		/* Command-ending semicolon? */
		if (tok == ';' && paren_depth == 0 && begin_depth == 0)
			break;
		if (tok == 0)
			yyerror("unexpected end of function definition");
		if (tok == K_INTO)
		{
			if (prev_tok == K_INSERT)
				continue;		/* INSERT INTO is not an INTO-target */
			if (prev_tok == K_MERGE)
				continue;		/* MERGE INTO is not an INTO-target */
			if (firsttoken == K_IMPORT)
				continue;		/* IMPORT ... INTO is not an INTO-target */
			if (have_into)
				yyerror("INTO specified more than once");
			have_into = true;
			into_start_loc = yylloc;
			plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_NORMAL;
			read_into_target(&target, &have_strict);
			plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_EXPR;
		}
	}

	plpgsql_IdentifierLookup = save_IdentifierLookup;

	if (have_into)
	{
		/*
		 * Insert an appropriate number of spaces corresponding to the
		 * INTO text, so that locations within the redacted SQL statement
		 * still line up with those in the original source text.
		 */
		plpgsql_append_source_text(&ds, location, into_start_loc);
		appendStringInfoSpaces(&ds, into_end_loc - into_start_loc);
		plpgsql_append_source_text(&ds, into_end_loc, yylloc);
	}
	else
		plpgsql_append_source_text(&ds, location, yylloc);

	/* trim any trailing whitespace, for neatness */
	while (ds.len > 0 && scanner_isspace(ds.data[ds.len - 1]))
		ds.data[--ds.len] = '\0';

	expr = palloc0(sizeof(PLpgSQL_expr));
	expr->query = pstrdup(ds.data);
	expr->parseMode = RAW_PARSE_DEFAULT;
	expr->plan = NULL;
	expr->paramnos = NULL;
	expr->target_param = -1;
	expr->ns = plpgsql_ns_top();
	pfree(ds.data);

	check_sql_expr(expr->query, expr->parseMode, location);

	execsql = palloc0(sizeof(PLpgSQL_stmt_execsql));
	execsql->cmd_type = PLPGSQL_STMT_EXECSQL;
	execsql->lineno = plpgsql_location_to_lineno(location);
	execsql->stmtid = ++plpgsql_curr_compile->nstatements;
	execsql->sqlstmt = expr;
	execsql->into = have_into;
	execsql->strict = have_strict;
	execsql->target = target;

	return (PLpgSQL_stmt *) execsql;
}


/*
 * Read FETCH or MOVE direction clause (everything through FROM/IN).
 */
static PLpgSQL_stmt_fetch *
read_fetch_direction(void)
{
	PLpgSQL_stmt_fetch *fetch;
	int			tok;
	bool		check_FROM = true;

	/*
	 * We create the PLpgSQL_stmt_fetch struct here, but only fill in
	 * the fields arising from the optional direction clause
	 */
	fetch = (PLpgSQL_stmt_fetch *) palloc0(sizeof(PLpgSQL_stmt_fetch));
	fetch->cmd_type = PLPGSQL_STMT_FETCH;
	fetch->stmtid = ++plpgsql_curr_compile->nstatements;
	/* set direction defaults: */
	fetch->direction = FETCH_FORWARD;
	fetch->how_many = 1;
	fetch->expr = NULL;
	fetch->returns_multiple_rows = false;

	tok = yylex();
	if (tok == 0)
		yyerror("unexpected end of function definition");

	if (tok_is_keyword(tok, &yylval,
					   K_NEXT, "next"))
	{
		/* use defaults */
	}
	else if (tok_is_keyword(tok, &yylval,
							K_PRIOR, "prior"))
	{
		fetch->direction = FETCH_BACKWARD;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_FIRST, "first"))
	{
		fetch->direction = FETCH_ABSOLUTE;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_LAST, "last"))
	{
		fetch->direction = FETCH_ABSOLUTE;
		fetch->how_many  = -1;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_ABSOLUTE, "absolute"))
	{
		fetch->direction = FETCH_ABSOLUTE;
		fetch->expr = read_sql_expression2(K_FROM, K_IN,
										   "FROM or IN",
										   NULL);
		check_FROM = false;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_RELATIVE, "relative"))
	{
		fetch->direction = FETCH_RELATIVE;
		fetch->expr = read_sql_expression2(K_FROM, K_IN,
										   "FROM or IN",
										   NULL);
		check_FROM = false;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_ALL, "all"))
	{
		fetch->how_many = FETCH_ALL;
		fetch->returns_multiple_rows = true;
	}
	else if (tok_is_keyword(tok, &yylval,
							K_FORWARD, "forward"))
	{
		complete_direction(fetch, &check_FROM);
	}
	else if (tok_is_keyword(tok, &yylval,
							K_BACKWARD, "backward"))
	{
		fetch->direction = FETCH_BACKWARD;
		complete_direction(fetch, &check_FROM);
	}
	else if (tok == K_FROM || tok == K_IN)
	{
		/* empty direction */
		check_FROM = false;
	}
	else if (tok == T_DATUM)
	{
		/* Assume there's no direction clause and tok is a cursor name */
		plpgsql_push_back_token(tok);
		check_FROM = false;
	}
	else
	{
		/*
		 * Assume it's a count expression with no preceding keyword.
		 * Note: we allow this syntax because core SQL does, but it's
		 * ambiguous with the case of an omitted direction clause; for
		 * instance, "MOVE n IN c" will fail if n is a variable, because the
		 * preceding else-arm will trigger.  Perhaps this can be improved
		 * someday, but it hardly seems worth a lot of work.
		 */
		plpgsql_push_back_token(tok);
		fetch->expr = read_sql_expression2(K_FROM, K_IN,
										   "FROM or IN",
										   NULL);
		fetch->returns_multiple_rows = true;
		check_FROM = false;
	}

	/* check FROM or IN keyword after direction's specification */
	if (check_FROM)
	{
		tok = yylex();
		if (tok != K_FROM && tok != K_IN)
			yyerror("expected FROM or IN");
	}

	return fetch;
}

/*
 * Process remainder of FETCH/MOVE direction after FORWARD or BACKWARD.
 * Allows these cases:
 *   FORWARD expr,  FORWARD ALL,  FORWARD
 *   BACKWARD expr, BACKWARD ALL, BACKWARD
 */
static void
complete_direction(PLpgSQL_stmt_fetch *fetch,  bool *check_FROM)
{
	int			tok;

	tok = yylex();
	if (tok == 0)
		yyerror("unexpected end of function definition");

	if (tok == K_FROM || tok == K_IN)
	{
		*check_FROM = false;
		return;
	}

	if (tok == K_ALL)
	{
		fetch->how_many = FETCH_ALL;
		fetch->returns_multiple_rows = true;
		*check_FROM = true;
		return;
	}

	plpgsql_push_back_token(tok);
	fetch->expr = read_sql_expression2(K_FROM, K_IN,
									   "FROM or IN",
									   NULL);
	fetch->returns_multiple_rows = true;
	*check_FROM = false;
}


static PLpgSQL_stmt *
make_return_stmt(int location)
{
	PLpgSQL_stmt_return *new;

	new = palloc0(sizeof(PLpgSQL_stmt_return));
	new->cmd_type = PLPGSQL_STMT_RETURN;
	new->lineno = plpgsql_location_to_lineno(location);
	new->stmtid  = ++plpgsql_curr_compile->nstatements;
	new->expr = NULL;
	new->retvarno = -1;

	if (plpgsql_curr_compile->fn_retset)
	{
		if (yylex() != ';')
			ereport(ERROR,
					(errcode(ERRCODE_DATATYPE_MISMATCH),
					 errmsg("RETURN cannot have a parameter in function returning set"),
					 errhint("Use RETURN NEXT or RETURN QUERY."),
					 parser_errposition(yylloc)));
	}
	else if (plpgsql_curr_compile->fn_rettype == VOIDOID)
	{
		if (yylex() != ';')
		{
			if (plpgsql_curr_compile->fn_prokind == PROKIND_PROCEDURE)
				ereport(ERROR,
						(errcode(ERRCODE_SYNTAX_ERROR),
						 errmsg("RETURN cannot have a parameter in a procedure"),
						 parser_errposition(yylloc)));
			else
				ereport(ERROR,
						(errcode(ERRCODE_DATATYPE_MISMATCH),
						 errmsg("RETURN cannot have a parameter in function returning void"),
						 parser_errposition(yylloc)));
		}
	}
	else if (plpgsql_curr_compile->out_param_varno >= 0)
	{
		if (yylex() != ';')
			ereport(ERROR,
					(errcode(ERRCODE_DATATYPE_MISMATCH),
					 errmsg("RETURN cannot have a parameter in function with OUT parameters"),
					 parser_errposition(yylloc)));
		new->retvarno = plpgsql_curr_compile->out_param_varno;
	}
	else
	{
		/*
		 * We want to special-case simple variable references for efficiency.
		 * So peek ahead to see if that's what we have.
		 */
		int			tok = yylex();

		if (tok == T_DATUM && plpgsql_peek() == ';' &&
			(yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_VAR ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_PROMISE ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_ROW ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_REC))
		{
			new->retvarno = yylval.wdatum.datum->dno;
			/* eat the semicolon token that we only peeked at above */
			tok = yylex();
			Assert(tok == ';');
		}
		else
		{
			/*
			 * Not (just) a variable name, so treat as expression.
			 *
			 * Note that a well-formed expression is _required_ here;
			 * anything else is a compile-time error.
			 */
			plpgsql_push_back_token(tok);
			new->expr = read_sql_expression(';', ";");
		}
	}

	return (PLpgSQL_stmt *) new;
}


static PLpgSQL_stmt *
make_return_next_stmt(int location)
{
	PLpgSQL_stmt_return_next *new;

	if (!plpgsql_curr_compile->fn_retset)
		ereport(ERROR,
				(errcode(ERRCODE_DATATYPE_MISMATCH),
				 errmsg("cannot use RETURN NEXT in a non-SETOF function"),
				 parser_errposition(location)));

	new = palloc0(sizeof(PLpgSQL_stmt_return_next));
	new->cmd_type = PLPGSQL_STMT_RETURN_NEXT;
	new->lineno = plpgsql_location_to_lineno(location);
	new->stmtid = ++plpgsql_curr_compile->nstatements;
	new->expr = NULL;
	new->retvarno = -1;

	if (plpgsql_curr_compile->out_param_varno >= 0)
	{
		if (yylex() != ';')
			ereport(ERROR,
					(errcode(ERRCODE_DATATYPE_MISMATCH),
					 errmsg("RETURN NEXT cannot have a parameter in function with OUT parameters"),
					 parser_errposition(yylloc)));
		new->retvarno = plpgsql_curr_compile->out_param_varno;
	}
	else
	{
		/*
		 * We want to special-case simple variable references for efficiency.
		 * So peek ahead to see if that's what we have.
		 */
		int			tok = yylex();

		if (tok == T_DATUM && plpgsql_peek() == ';' &&
			(yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_VAR ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_PROMISE ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_ROW ||
			 yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_REC))
		{
			new->retvarno = yylval.wdatum.datum->dno;
			/* eat the semicolon token that we only peeked at above */
			tok = yylex();
			Assert(tok == ';');
		}
		else
		{
			/*
			 * Not (just) a variable name, so treat as expression.
			 *
			 * Note that a well-formed expression is _required_ here;
			 * anything else is a compile-time error.
			 */
			plpgsql_push_back_token(tok);
			new->expr = read_sql_expression(';', ";");
		}
	}

	return (PLpgSQL_stmt *) new;
}


static PLpgSQL_stmt *
make_return_query_stmt(int location)
{
	PLpgSQL_stmt_return_query *new;
	int			tok;

	if (!plpgsql_curr_compile->fn_retset)
		ereport(ERROR,
				(errcode(ERRCODE_DATATYPE_MISMATCH),
				 errmsg("cannot use RETURN QUERY in a non-SETOF function"),
				 parser_errposition(location)));

	new = palloc0(sizeof(PLpgSQL_stmt_return_query));
	new->cmd_type = PLPGSQL_STMT_RETURN_QUERY;
	new->lineno = plpgsql_location_to_lineno(location);
	new->stmtid = ++plpgsql_curr_compile->nstatements;

	/* check for RETURN QUERY EXECUTE */
	if ((tok = yylex()) != K_EXECUTE)
	{
		/* ordinary static query */
		plpgsql_push_back_token(tok);
		new->query = read_sql_stmt();
	}
	else
	{
		/* dynamic SQL */
		int			term;

		new->dynquery = read_sql_expression2(';', K_USING, "; or USING",
											 &term);
		if (term == K_USING)
		{
			do
			{
				PLpgSQL_expr *expr;

				expr = read_sql_expression2(',', ';', ", or ;", &term);
				new->params = lappend(new->params, expr);
			} while (term == ',');
		}
	}

	return (PLpgSQL_stmt *) new;
}


/* convenience routine to fetch the name of a T_DATUM */
static char *
NameOfDatum(PLwdatum *wdatum)
{
	if (wdatum->ident)
		return wdatum->ident;
	Assert(wdatum->idents != NIL);
	return NameListToString(wdatum->idents);
}

static void
check_assignable(PLpgSQL_datum *datum, int location)
{
	switch (datum->dtype)
	{
		case PLPGSQL_DTYPE_VAR:
		case PLPGSQL_DTYPE_PROMISE:
		case PLPGSQL_DTYPE_REC:
			if (((PLpgSQL_variable *) datum)->isconst)
				ereport(ERROR,
						(errcode(ERRCODE_ERROR_IN_ASSIGNMENT),
						 errmsg("variable \"%s\" is declared CONSTANT",
								((PLpgSQL_variable *) datum)->refname),
						 parser_errposition(location)));
			break;
		case PLPGSQL_DTYPE_ROW:
			/* always assignable; member vars were checked at compile time */
			break;
		case PLPGSQL_DTYPE_RECFIELD:
			/* assignable if parent record is */
			check_assignable(plpgsql_Datums[((PLpgSQL_recfield *) datum)->recparentno],
							 location);
			break;
		default:
			elog(ERROR, "unrecognized dtype: %d", datum->dtype);
			break;
	}
}

/*
 * Read the argument of an INTO clause.  On entry, we have just read the
 * INTO keyword.
 */
static void
read_into_target(PLpgSQL_variable **target, bool *strict)
{
	int			tok;

	/* Set default results */
	*target = NULL;
	if (strict)
		*strict = false;

	tok = yylex();
	if (strict && tok == K_STRICT)
	{
		*strict = true;
		tok = yylex();
	}

	/*
	 * Currently, a row or record variable can be the single INTO target,
	 * but not a member of a multi-target list.  So we throw error if there
	 * is a comma after it, because that probably means the user tried to
	 * write a multi-target list.  If this ever gets generalized, we should
	 * probably refactor read_into_scalar_list so it handles all cases.
	 */
	switch (tok)
	{
		case T_DATUM:
			if (yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_ROW ||
				yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_REC)
			{
				check_assignable(yylval.wdatum.datum, yylloc);
				*target = (PLpgSQL_variable *) yylval.wdatum.datum;

				if ((tok = yylex()) == ',')
					ereport(ERROR,
							(errcode(ERRCODE_SYNTAX_ERROR),
							 errmsg("record variable cannot be part of multiple-item INTO list"),
							 parser_errposition(yylloc)));
				plpgsql_push_back_token(tok);
			}
			else
			{
				*target = (PLpgSQL_variable *)
					read_into_scalar_list(NameOfDatum(&(yylval.wdatum)),
										  yylval.wdatum.datum, yylloc);
			}
			break;

		default:
			/* just to give a better message than "syntax error" */
			current_token_is_not_variable(tok);
	}
}

/*
 * Given the first datum and name in the INTO list, continue to read
 * comma-separated scalar variables until we run out. Then construct
 * and return a fake "row" variable that represents the list of
 * scalars.
 */
static PLpgSQL_row *
read_into_scalar_list(char *initial_name,
					  PLpgSQL_datum *initial_datum,
					  int initial_location)
{
	int			nfields;
	char	   *fieldnames[1024];
	int			varnos[1024];
	PLpgSQL_row	*row;
	int			tok;

	check_assignable(initial_datum, initial_location);
	fieldnames[0] = initial_name;
	varnos[0] = initial_datum->dno;
	nfields = 1;

	while ((tok = yylex()) == ',')
	{
		/* Check for array overflow */
		if (nfields >= 1024)
			ereport(ERROR,
					(errcode(ERRCODE_PROGRAM_LIMIT_EXCEEDED),
					 errmsg("too many INTO variables specified"),
					 parser_errposition(yylloc)));

		tok = yylex();
		switch (tok)
		{
			case T_DATUM:
				check_assignable(yylval.wdatum.datum, yylloc);
				if (yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_ROW ||
					yylval.wdatum.datum->dtype == PLPGSQL_DTYPE_REC)
					ereport(ERROR,
							(errcode(ERRCODE_SYNTAX_ERROR),
							 errmsg("\"%s\" is not a scalar variable",
									NameOfDatum(&(yylval.wdatum))),
							 parser_errposition(yylloc)));
				fieldnames[nfields] = NameOfDatum(&(yylval.wdatum));
				varnos[nfields++]	= yylval.wdatum.datum->dno;
				break;

			default:
				/* just to give a better message than "syntax error" */
				current_token_is_not_variable(tok);
		}
	}

	/*
	 * We read an extra, non-comma token from yylex(), so push it
	 * back onto the input stream
	 */
	plpgsql_push_back_token(tok);

	row = palloc0(sizeof(PLpgSQL_row));
	row->dtype = PLPGSQL_DTYPE_ROW;
	row->refname = "(unnamed row)";
	row->lineno = plpgsql_location_to_lineno(initial_location);
	row->rowtupdesc = NULL;
	row->nfields = nfields;
	row->fieldnames = palloc(sizeof(char *) * nfields);
	row->varnos = palloc(sizeof(int) * nfields);
	while (--nfields >= 0)
	{
		row->fieldnames[nfields] = fieldnames[nfields];
		row->varnos[nfields] = varnos[nfields];
	}

	plpgsql_adddatum((PLpgSQL_datum *) row);

	return row;
}

/*
 * Convert a single scalar into a "row" list.  This is exactly
 * like read_into_scalar_list except we never consume any input.
 *
 * Note: lineno could be computed from location, but since callers
 * have it at hand already, we may as well pass it in.
 */
static PLpgSQL_row *
make_scalar_list1(char *initial_name,
				  PLpgSQL_datum *initial_datum,
				  int lineno, int location)
{
	PLpgSQL_row *row;

	check_assignable(initial_datum, location);

	row = palloc0(sizeof(PLpgSQL_row));
	row->dtype = PLPGSQL_DTYPE_ROW;
	row->refname = "(unnamed row)";
	row->lineno = lineno;
	row->rowtupdesc = NULL;
	row->nfields = 1;
	row->fieldnames = palloc(sizeof(char *));
	row->varnos = palloc(sizeof(int));
	row->fieldnames[0] = initial_name;
	row->varnos[0] = initial_datum->dno;

	plpgsql_adddatum((PLpgSQL_datum *) row);

	return row;
}

/*
 * When the PL/pgSQL parser expects to see a SQL statement, it is very
 * liberal in what it accepts; for example, we often assume an
 * unrecognized keyword is the beginning of a SQL statement. This
 * avoids the need to duplicate parts of the SQL grammar in the
 * PL/pgSQL grammar, but it means we can accept wildly malformed
 * input. To try and catch some of the more obviously invalid input,
 * we run the strings we expect to be SQL statements through the main
 * SQL parser.
 *
 * We only invoke the raw parser (not the analyzer); this doesn't do
 * any database access and does not check any semantic rules, it just
 * checks for basic syntactic correctness. We do this here, rather
 * than after parsing has finished, because a malformed SQL statement
 * may cause the PL/pgSQL parser to become confused about statement
 * borders. So it is best to bail out as early as we can.
 *
 * It is assumed that "stmt" represents a copy of the function source text
 * beginning at offset "location".  We use this assumption to transpose
 * any error cursor position back to the function source text.
 * If no error cursor is provided, we'll just point at "location".
 */
static void
check_sql_expr(const char *stmt, RawParseMode parseMode, int location)
{
	sql_error_callback_arg cbarg;
	ErrorContextCallback  syntax_errcontext;
	MemoryContext oldCxt;

	if (!plpgsql_check_syntax)
		return;

	cbarg.location = location;

	syntax_errcontext.callback = plpgsql_sql_error_callback;
	syntax_errcontext.arg = &cbarg;
	syntax_errcontext.previous = error_context_stack;
	error_context_stack = &syntax_errcontext;

	oldCxt = MemoryContextSwitchTo(plpgsql_compile_tmp_cxt);
	(void) raw_parser(stmt, parseMode);
	MemoryContextSwitchTo(oldCxt);

	/* Restore former ereport callback */
	error_context_stack = syntax_errcontext.previous;
}

static void
plpgsql_sql_error_callback(void *arg)
{
	sql_error_callback_arg *cbarg = (sql_error_callback_arg *) arg;
	int			errpos;

	/*
	 * First, set up internalerrposition to point to the start of the
	 * statement text within the function text.  Note this converts
	 * location (a byte offset) to a character number.
	 */
	parser_errposition(cbarg->location);

	/*
	 * If the core parser provided an error position, transpose it.
	 * Note we are dealing with 1-based character numbers at this point.
	 */
	errpos = geterrposition();
	if (errpos > 0)
	{
		int			myerrpos = getinternalerrposition();

		if (myerrpos > 0)		/* safety check */
			internalerrposition(myerrpos + errpos - 1);
	}

	/* In any case, flush errposition --- we want internalerrposition only */
	errposition(0);
}

/*
 * Parse a SQL datatype name and produce a PLpgSQL_type structure.
 *
 * The heavy lifting is done elsewhere.  Here we are only concerned
 * with setting up an errcontext link that will let us give an error
 * cursor pointing into the plpgsql function source, if necessary.
 * This is handled the same as in check_sql_expr(), and we likewise
 * expect that the given string is a copy from the source text.
 */
static PLpgSQL_type *
parse_datatype(const char *string, int location)
{
	TypeName   *typeName;
	Oid			type_id;
	int32		typmod;
	sql_error_callback_arg cbarg;
	ErrorContextCallback  syntax_errcontext;

	cbarg.location = location;

	syntax_errcontext.callback = plpgsql_sql_error_callback;
	syntax_errcontext.arg = &cbarg;
	syntax_errcontext.previous = error_context_stack;
	error_context_stack = &syntax_errcontext;

	/* Let the main parser try to parse it under standard SQL rules */
	typeName = typeStringToTypeName(string, NULL);
	typenameTypeIdAndMod(NULL, typeName, &type_id, &typmod);

	/* Restore former ereport callback */
	error_context_stack = syntax_errcontext.previous;

	/* Okay, build a PLpgSQL_type data structure for it */
	return plpgsql_build_datatype(type_id, typmod,
								  plpgsql_curr_compile->fn_input_collation,
								  typeName);
}

/*
 * Check block starting and ending labels match.
 */
static void
check_labels(const char *start_label, const char *end_label, int end_location)
{
	if (end_label)
	{
		if (!start_label)
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("end label \"%s\" specified for unlabeled block",
							end_label),
					 parser_errposition(end_location)));

		if (strcmp(start_label, end_label) != 0)
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("end label \"%s\" differs from block's label \"%s\"",
							end_label, start_label),
					 parser_errposition(end_location)));
	}
}

/*
 * Read the arguments (if any) for a cursor, followed by the until token
 *
 * If cursor has no args, just swallow the until token and return NULL.
 * If it does have args, we expect to see "( arg [, arg ...] )" followed
 * by the until token, where arg may be a plain expression, or a named
 * parameter assignment of the form argname := expr. Consume all that and
 * return a SELECT query that evaluates the expression(s) (without the outer
 * parens).
 */
static PLpgSQL_expr *
read_cursor_args(PLpgSQL_var *cursor, int until)
{
	PLpgSQL_expr *expr;
	PLpgSQL_row *row;
	int			tok;
	int			argc;
	char	  **argv;
	StringInfoData ds;
	bool		any_named = false;

	tok = yylex();
	if (cursor->cursor_explicit_argrow < 0)
	{
		/* No arguments expected */
		if (tok == '(')
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("cursor \"%s\" has no arguments",
							cursor->refname),
					 parser_errposition(yylloc)));

		if (tok != until)
			yyerror("syntax error");

		return NULL;
	}

	/* Else better provide arguments */
	if (tok != '(')
		ereport(ERROR,
				(errcode(ERRCODE_SYNTAX_ERROR),
				 errmsg("cursor \"%s\" has arguments",
						cursor->refname),
				 parser_errposition(yylloc)));

	/*
	 * Read the arguments, one by one.
	 */
	row = (PLpgSQL_row *) plpgsql_Datums[cursor->cursor_explicit_argrow];
	argv = (char **) palloc0(row->nfields * sizeof(char *));

	for (argc = 0; argc < row->nfields; argc++)
	{
		PLpgSQL_expr *item;
		int			endtoken;
		int			argpos;
		int			tok1,
					tok2;
		int			arglocation;

		/* Check if it's a named parameter: "param := value" */
		plpgsql_peek2(&tok1, &tok2, &arglocation, NULL);
		if (tok1 == IDENT && tok2 == COLON_EQUALS)
		{
			char   *argname;
			IdentifierLookup save_IdentifierLookup;

			/* Read the argument name, ignoring any matching variable */
			save_IdentifierLookup = plpgsql_IdentifierLookup;
			plpgsql_IdentifierLookup = IDENTIFIER_LOOKUP_DECLARE;
			yylex();
			argname = yylval.str;
			plpgsql_IdentifierLookup = save_IdentifierLookup;

			/* Match argument name to cursor arguments */
			for (argpos = 0; argpos < row->nfields; argpos++)
			{
				if (strcmp(row->fieldnames[argpos], argname) == 0)
					break;
			}
			if (argpos == row->nfields)
				ereport(ERROR,
						(errcode(ERRCODE_SYNTAX_ERROR),
						 errmsg("cursor \"%s\" has no argument named \"%s\"",
								cursor->refname, argname),
						 parser_errposition(yylloc)));

			/*
			 * Eat the ":=". We already peeked, so the error should never
			 * happen.
			 */
			tok2 = yylex();
			if (tok2 != COLON_EQUALS)
				yyerror("syntax error");

			any_named = true;
		}
		else
			argpos = argc;

		if (argv[argpos] != NULL)
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("value for parameter \"%s\" of cursor \"%s\" specified more than once",
							row->fieldnames[argpos], cursor->refname),
					 parser_errposition(arglocation)));

		/*
		 * Read the value expression. To provide the user with meaningful
		 * parse error positions, we check the syntax immediately, instead of
		 * checking the final expression that may have the arguments
		 * reordered.
		 */
		item = read_sql_construct(',', ')', 0,
								  ",\" or \")",
								  RAW_PARSE_PLPGSQL_EXPR,
								  true, true,
								  NULL, &endtoken);

		argv[argpos] = item->query;

		if (endtoken == ')' && !(argc == row->nfields - 1))
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("not enough arguments for cursor \"%s\"",
							cursor->refname),
					 parser_errposition(yylloc)));

		if (endtoken == ',' && (argc == row->nfields - 1))
			ereport(ERROR,
					(errcode(ERRCODE_SYNTAX_ERROR),
					 errmsg("too many arguments for cursor \"%s\"",
							cursor->refname),
					 parser_errposition(yylloc)));
	}

	/* Make positional argument list */
	initStringInfo(&ds);
	for (argc = 0; argc < row->nfields; argc++)
	{
		Assert(argv[argc] != NULL);

		/*
		 * Because named notation allows permutated argument lists, include
		 * the parameter name for meaningful runtime errors.
		 */
		appendStringInfoString(&ds, argv[argc]);
		if (any_named)
			appendStringInfo(&ds, " AS %s",
							 quote_identifier(row->fieldnames[argc]));
		if (argc < row->nfields - 1)
			appendStringInfoString(&ds, ", ");
	}

	expr = palloc0(sizeof(PLpgSQL_expr));
	expr->query = pstrdup(ds.data);
	expr->parseMode = RAW_PARSE_PLPGSQL_EXPR;
	expr->plan = NULL;
	expr->paramnos = NULL;
	expr->target_param = -1;
	expr->ns = plpgsql_ns_top();
	pfree(ds.data);

	/* Next we'd better find the until token */
	tok = yylex();
	if (tok != until)
		yyerror("syntax error");

	return expr;
}

/*
 * Parse RAISE ... USING options
 */
static List *
read_raise_options(void)
{
	List	   *result = NIL;

	for (;;)
	{
		PLpgSQL_raise_option *opt;
		int			tok;

		if ((tok = yylex()) == 0)
			yyerror("unexpected end of function definition");

		opt = (PLpgSQL_raise_option *) palloc(sizeof(PLpgSQL_raise_option));

		if (tok_is_keyword(tok, &yylval,
						   K_ERRCODE, "errcode"))
			opt->opt_type = PLPGSQL_RAISEOPTION_ERRCODE;
		else if (tok_is_keyword(tok, &yylval,
								K_MESSAGE, "message"))
			opt->opt_type = PLPGSQL_RAISEOPTION_MESSAGE;
		else if (tok_is_keyword(tok, &yylval,
								K_DETAIL, "detail"))
			opt->opt_type = PLPGSQL_RAISEOPTION_DETAIL;
		else if (tok_is_keyword(tok, &yylval,
								K_HINT, "hint"))
			opt->opt_type = PLPGSQL_RAISEOPTION_HINT;
		else if (tok_is_keyword(tok, &yylval,
								K_COLUMN, "column"))
			opt->opt_type = PLPGSQL_RAISEOPTION_COLUMN;
		else if (tok_is_keyword(tok, &yylval,
								K_CONSTRAINT, "constraint"))
			opt->opt_type = PLPGSQL_RAISEOPTION_CONSTRAINT;
		else if (tok_is_keyword(tok, &yylval,
								K_DATATYPE, "datatype"))
			opt->opt_type = PLPGSQL_RAISEOPTION_DATATYPE;
		else if (tok_is_keyword(tok, &yylval,
								K_TABLE, "table"))
			opt->opt_type = PLPGSQL_RAISEOPTION_TABLE;
		else if (tok_is_keyword(tok, &yylval,
								K_SCHEMA, "schema"))
			opt->opt_type = PLPGSQL_RAISEOPTION_SCHEMA;
		else
			yyerror("unrecognized RAISE statement option");

		tok = yylex();
		if (tok != '=' && tok != COLON_EQUALS)
			yyerror("syntax error, expected \"=\"");

		opt->expr = read_sql_expression2(',', ';', ", or ;", &tok);

		result = lappend(result, opt);

		if (tok == ';')
			break;
	}

	return result;
}

/*
 * Check that the number of parameter placeholders in the message matches the
 * number of parameters passed to it, if a message was given.
 */
static void
check_raise_parameters(PLpgSQL_stmt_raise *stmt)
{
	char	   *cp;
	int			expected_nparams = 0;

	if (stmt->message == NULL)
		return;

	for (cp = stmt->message; *cp; cp++)
	{
		if (cp[0] == '%')
		{
			/* ignore literal % characters */
			if (cp[1] == '%')
				cp++;
			else
				expected_nparams++;
		}
	}

	if (expected_nparams < list_length(stmt->params))
		ereport(ERROR,
				(errcode(ERRCODE_SYNTAX_ERROR),
				errmsg("too many parameters specified for RAISE")));
	if (expected_nparams > list_length(stmt->params))
		ereport(ERROR,
				(errcode(ERRCODE_SYNTAX_ERROR),
				errmsg("too few parameters specified for RAISE")));
}

/*
 * Fix up CASE statement
 */
static PLpgSQL_stmt *
make_case(int location, PLpgSQL_expr *t_expr,
		  List *case_when_list, List *else_stmts)
{
	PLpgSQL_stmt_case *new;

	new = palloc(sizeof(PLpgSQL_stmt_case));
	new->cmd_type = PLPGSQL_STMT_CASE;
	new->lineno = plpgsql_location_to_lineno(location);
	new->stmtid = ++plpgsql_curr_compile->nstatements;
	new->t_expr = t_expr;
	new->t_varno = 0;
	new->case_when_list = case_when_list;
	new->have_else = (else_stmts != NIL);
	/* Get rid of list-with-NULL hack */
	if (list_length(else_stmts) == 1 && linitial(else_stmts) == NULL)
		new->else_stmts = NIL;
	else
		new->else_stmts = else_stmts;

	/*
	 * When test expression is present, we create a var for it and then
	 * convert all the WHEN expressions to "VAR IN (original_expression)".
	 * This is a bit klugy, but okay since we haven't yet done more than
	 * read the expressions as text.  (Note that previous parsing won't
	 * have complained if the WHEN ... THEN expression contained multiple
	 * comma-separated values.)
	 */
	if (t_expr)
	{
		char		varname[32];
		PLpgSQL_var *t_var;
		ListCell   *l;

		/* use a name unlikely to collide with any user names */
		snprintf(varname, sizeof(varname), "__Case__Variable_%d__",
				 plpgsql_nDatums);

		/*
		 * We don't yet know the result datatype of t_expr.  Build the
		 * variable as if it were INT4; we'll fix this at runtime if needed.
		 */
		t_var = (PLpgSQL_var *)
			plpgsql_build_variable(varname, new->lineno,
								   plpgsql_build_datatype(INT4OID,
														  -1,
														  InvalidOid,
														  NULL),
								   true);
		new->t_varno = t_var->dno;

		foreach(l, case_when_list)
		{
			PLpgSQL_case_when *cwt = (PLpgSQL_case_when *) lfirst(l);
			PLpgSQL_expr *expr = cwt->expr;
			StringInfoData ds;

			/* We expect to have expressions not statements */
			Assert(expr->parseMode == RAW_PARSE_PLPGSQL_EXPR);

			/* Do the string hacking */
			initStringInfo(&ds);

			appendStringInfo(&ds, "\"%s\" IN (%s)",
							 varname, expr->query);

			pfree(expr->query);
			expr->query = pstrdup(ds.data);
			/* Adjust expr's namespace to include the case variable */
			expr->ns = plpgsql_ns_top();

			pfree(ds.data);
		}
	}

	return (PLpgSQL_stmt *) new;
}

