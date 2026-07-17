SELECT public.finish();
CALL public.refresh();
SELECT public.pick(1, 2);
SELECT pick(1);
SELECT public.lowercase();
SELECT public."MixedCase"();
SELECT public.mixedcase();
SELECT missing_runtime_function();
