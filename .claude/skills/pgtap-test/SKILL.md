---
name: pgtap-test
description: Scaffold a new pgTAP test file under test/sql/ with the project's standard boilerplate (BEGIN, CREATE EXTENSION pgtap + code_moniker, plan, finish, ROLLBACK). Pick the next free NN_ prefix automatically. Use when adding SQL-level tests for a new feature on the moniker / code_graph surface.
disable-model-invocation: true
---

# pgtap-test

Create a new test file `test/sql/<NN>_<slug>.sql` ready for pgTAP assertions.

## Inputs

- `<slug>` — short underscore-separated name, e.g. `index_opclass`, `subtree_query`. The user supplies it.
- `<intent>` — one-sentence description of what's being tested (becomes the file's leading comment).
- `<plan_count>` — initial number of assertions; placeholder, easy to update later.

## Steps

1. List existing files in `test/sql/` and pick the next two-digit prefix (00 → 01 → 02 …).
2. Write the file with the canonical scaffold below.
3. Don't run the test — that's the user's job after they fill it in.

## Scaffold

```sql
-- <intent>

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(<plan_count>);

-- TODO: assertions here. Use is(), ok(), has_function(), has_type(),
-- throws_ok(), etc. Each assertion's third arg states the invariant.

SELECT * FROM finish();

ROLLBACK;
```

## Project conventions

- One `plan(N)` per file ; keep N up to date.
- Each assertion's description states the **invariant**, not "it should work" or "this returns the right thing".
- `BEGIN ... ROLLBACK` envelope so the test leaves no DB state behind.
- File-level comment at the top is one or two sentences stating *what surface* the file exercises (e.g. "Btree and hash opclasses on `moniker`."), not "Phase X tests" — narration of the change is forbidden by CLAUDE.md.
- Helper plpgsql functions defined with `CREATE OR REPLACE FUNCTION` inside the transaction are fine ; they roll back with the rest.
