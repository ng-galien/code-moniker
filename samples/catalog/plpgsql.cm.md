---
name: plpgsql
title: PL/pgSQL syntax-tree inspection
lang: plpgsql
blurb: Inspect injected PL/pgSQL and nested SQL without inventing a separate graph-rule namespace
learn_kind: language
learn_path: languages/sql/plpgsql
learn_order: 10
tags: sql,plpgsql,postgresql,injections,syntax-tree,mcp
learn_aliases: sql-plpgsql
published: true
---

# PL/pgSQL syntax-tree inspection

Code Moniker exposes PostgreSQL through two complementary surfaces:

- indexed database definitions and relations use the `sql.*` graph-rule
  namespace;
- on-demand parsing accepts `plpgsql` and SQL documents expose injected
  PL/pgSQL bodies plus their nested SQL statements and expressions.

There is no autonomous `plpgsql.*` graph-rule namespace. Use the SQL parent for
schema, routine, read/write, call, and dependency policies.

Parse a standalone block through the MCP tool:

```text
code_moniker_read
  language:"plpgsql"
  source:"DECLARE total numeric; BEGIN SELECT sum(amount) INTO total FROM invoice; RETURN total; END;"
```

For an indexed `.sql` file, request its syntax tree instead:

```text
code_moniker_read
  uri:"db/routines.sql"
  ast:true
  max_depth:8
  max_nodes:120
```

Injected roots report `language`, an `entry_point` (`block`, `script`,
`statement`, or `expression`), and their own `has_error` state. A malformed
nested expression can therefore be reported without treating the whole host
document as invalid. Function, procedure, and `DO` bodies are supported; this
scenario also keeps one graph rule executable.

```toml cm:rules
default_rules = false

[[sql.function.where]]
id = "function-snake-case"
expr = "name =~ ^[a-z_][a-z0-9_]*$"
message = "SQL function `{name}` should use snake_case."
```

```sql cm:file=db/routines.sql
CREATE FUNCTION public."InvoiceTotal"() RETURNS numeric
LANGUAGE plpgsql AS $$
DECLARE total numeric;
BEGIN
	SELECT sum(amount) INTO total FROM public.invoice;
	RETURN total;
END;
$$;

DO $$
BEGIN
	PERFORM public."InvoiceTotal"();
END;
$$ LANGUAGE plpgsql;
```

```cm:expect
sql.function.function-snake-case @ db/routines.sql:L1-L8
```
