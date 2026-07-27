CREATE FUNCTION public.finish()
RETURNS void
LANGUAGE sql
AS $$ SELECT 1 $$;

CREATE TYPE public.order_state AS ENUM ('new', 'done');

CREATE FUNCTION public.accept_state(value public.order_state)
RETURNS public.order_state
LANGUAGE sql
AS $$ SELECT value $$;

CREATE PROCEDURE public.refresh()
LANGUAGE sql
AS $$ SELECT 1 $$;

CREATE FUNCTION public.pick(value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT value $$;

CREATE FUNCTION public.pick(left_value int, right_value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT left_value + right_value $$;

CREATE FUNCTION private.pick(value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT value $$;

CREATE FUNCTION public.choose(value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT value $$;

CREATE FUNCTION public.choose(value text)
RETURNS text
LANGUAGE sql
AS $$ SELECT value $$;

CREATE FUNCTION private.choose(value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT value $$;

CREATE FUNCTION public.call_choose_int(value int)
RETURNS int
LANGUAGE sql
AS $$ SELECT public.choose(value) $$;

CREATE FUNCTION public.call_choose_text(value text)
RETURNS text
LANGUAGE sql
AS $$ SELECT public.choose(value) $$;

CREATE FUNCTION public.call_choose_unknown()
RETURNS text
LANGUAGE sql
AS $$ SELECT public.choose(NULL) $$;

CREATE FUNCTION public.call_choose_search_path(value int)
RETURNS int
LANGUAGE sql
SET search_path = public, pg_temp
AS $$ SELECT choose(value) $$;

CREATE FUNCTION public."lowercase"()
RETURNS int
LANGUAGE sql
AS $$ SELECT 1 $$;

-- Quoted mixed case is intentional: the linkage scenario verifies SQL identifier case semantics.
-- code-moniker: ignore[sql.function.name-snakecase]
CREATE FUNCTION public."MixedCase"()
RETURNS int
LANGUAGE sql
AS $$ SELECT 1 $$;
