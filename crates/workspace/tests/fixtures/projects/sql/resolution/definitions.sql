CREATE FUNCTION public.finish()
RETURNS void
LANGUAGE sql
AS $$ SELECT 1 $$;

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

CREATE FUNCTION public."lowercase"()
RETURNS int
LANGUAGE sql
AS $$ SELECT 1 $$;

CREATE FUNCTION public."MixedCase"()
RETURNS int
LANGUAGE sql
AS $$ SELECT 1 $$;
