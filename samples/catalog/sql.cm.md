---
name: sql
lang: sql
blurb: snake_case SQL objects, v_-prefixed views, and a public schema that keeps out of private
published: true
---

# SQL / PL/pgSQL starter pack

The SQL sample enforces snake_case names for tables, functions, and
procedures, a `v_` prefix for views, and a schema boundary: objects in the
`public` schema must not reach into the `private` schema directly.

```toml cm:rules
default_rules = false

[aliases]
public_schema = "moniker ~ '**/schema:public/**'"
private_schema = "moniker ~ '**/schema:private/**'"
audit_schema = "moniker ~ '**/schema:audit/**'"

src_public = "source ~ '**/schema:public/**'"
tgt_private = "target ~ '**/schema:private/**'"

[[sql.table.where]]
id = "table-snakecase"
rationale = "Snake case keeps SQL object names readable without quoting, which makes migrations and queries easier to write."
expr = "name =~ ^[a-z_][a-z0-9_]*$"
message = "Table `{name}` must use snake_case."

[[sql.view.where]]
id = "view-prefix"
rationale = "A view prefix tells readers that they are looking at a projection, not a stored table."
expr = "name =~ ^v_[a-z0-9_]+$"
message = "View `{name}` must start with v_."

[[sql.function.where]]
id = "function-snakecase"
rationale = "Database functions are easier to review when their names are plain SQL identifiers and their bodies stay compact."
expr = "name =~ ^[a-z_][a-z0-9_]*$ AND lines <= 120"
message = "Function `{name}` must use snake_case and stay under 120 lines."

[[sql.procedure.where]]
id = "procedure-snakecase"
rationale = "Procedures are called from scripts and applications. Snake case avoids quoting surprises."
expr = "name =~ ^[a-z_][a-z0-9_]*$"
message = "Procedure `{name}` must use snake_case."

[[sql.refs.where]]
id = "public-no-private"
rationale = "The public schema is an API. Keep private schema details behind stable database objects."
expr = "$src_public => NOT $tgt_private"
message = "Public schema objects must not depend directly on private schema objects."
```

The private schema is the protected core — everything in it is snake_case
and self-contained:

```sql cm:file=db/private_core.sql
CREATE TABLE private.secrets (
	id bigint PRIMARY KEY,
	owner_id bigint NOT NULL,
	token text NOT NULL
);

CREATE FUNCTION private.fetch_secret(p_owner bigint) RETURNS text
LANGUAGE plpgsql AS $$
BEGIN
	RETURN (SELECT token FROM private.secrets WHERE owner_id = p_owner LIMIT 1);
END;
$$;
```

The public API file breaks the naming rules — a quoted CamelCase table, a
view without the `v_` prefix, a CamelCase function — and `v_owner_secrets`
calls straight into the private schema instead of going through a sanctioned
interface:

```sql cm:file=db/public_api.sql
CREATE TABLE public."UserAccounts" (
	id bigint PRIMARY KEY,
	email text NOT NULL
);

CREATE TABLE public.user_sessions (
	id bigint PRIMARY KEY,
	account_id bigint NOT NULL
);

CREATE VIEW public.active_sessions AS
SELECT id, account_id FROM public.user_sessions;

CREATE VIEW public.v_account_emails AS
SELECT id, email FROM public."UserAccounts";

CREATE FUNCTION public.GetAccountEmail(p_id bigint) RETURNS text
LANGUAGE plpgsql AS $$
BEGIN
	RETURN (SELECT email FROM public."UserAccounts" WHERE id = p_id);
END;
$$;

CREATE VIEW public.v_owner_secrets AS
SELECT owner_id, private.fetch_secret(owner_id) AS token
FROM public.user_sessions;

CREATE PROCEDURE public.purge_sessions()
LANGUAGE plpgsql AS $$
BEGIN
	DELETE FROM public.user_sessions;
END;
$$;
```

A note on `procedure-snakecase`: the SQL extractor canonicalizes
`CREATE PROCEDURE` statements as `function` symbols (see `purge_sessions`
above, which is checked by `function-snakecase` instead), so no `procedure`
symbol ever exists and the rule cannot fire today.

```cm:expect
! sql.procedure.procedure-snakecase the SQL extractor emits CREATE PROCEDURE as function symbols, so procedure defs never exist
sql.table.table-snakecase @ db/public_api.sql:L1-L4
sql.view.view-prefix @ db/public_api.sql:L11-L12
sql.function.function-snakecase @ db/public_api.sql:L17-L22
sql.refs.public-no-private @ db/public_api.sql:L25
```
