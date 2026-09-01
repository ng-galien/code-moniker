---
name: c
title: C starter conventions
lang: c
blurb: C function naming with an explicit view of the available graph vocabulary
learn_kind: language
learn_path: languages/c
learn_order: 70
tags: c,naming,functions,headers
published: true
---

# C starter conventions

C files use the `c.*` rule namespace. Inspect the extracted namespaces, types,
callables, values, references, and visibilities before choosing a policy:

```sh
code-moniker langs c
```

The following project convention keeps function names in snake_case. It is a
copyable policy example, not a claim that every C codebase follows this style.

```toml cm:rules
default_rules = false

[[c.func.where]]
id = "function-snake-case"
expr = "name =~ ^[a-z_][a-z0-9_]*$"
message = "C function `{name}` should use snake_case."
```

```c cm:file=src/account.c
int LoadAccount(void) {
	return 0;
}
```

```cm:expect
c.func.function-snake-case @ src/account.c:L1-L3
```
