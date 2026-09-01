---
name: javascript
title: JavaScript starter pack
lang: js
blurb: JavaScript naming and module boundaries with policies independent from TypeScript
learn_kind: language
learn_path: languages/javascript
learn_order: 20
tags: javascript,js,mjs,cjs,naming,modules
learn_aliases: js
published: true
---

# JavaScript starter pack

JavaScript uses the `js.*` rule namespace for `.js`, `.mjs`, and `.cjs` files.
It shares parsing, package resolution, and linkage with TypeScript, TSX, and
JSX, but it does not inherit their rules. This pack keeps all extracted functions
camelCase and prevents domain modules from importing infrastructure modules.

```toml cm:rules
default_rules = false

[aliases]
src_domain = "source ~ '**/dir:domain/**'"
tgt_infrastructure = "target ~ '**/dir:infrastructure/**'"

[[js.function.where]]
id = "function-camel-case"
expr = "name =~ ^[a-z][A-Za-z0-9]*$"
message = "JavaScript function `{name}` should be camelCase."

[[refs.where]]
id = "javascript-domain-avoids-infrastructure"
expr = "$src_domain => NOT $tgt_infrastructure"
message = "JavaScript domain code must not import infrastructure directly."
```

```js cm:file=src/infrastructure/accountStore.js
export function saveAccount(account) {
	return account;
}
```

```js cm:file=src/domain/account.js
import { saveAccount } from "../infrastructure/accountStore.js";

export function LoadAccount(account) {
	return saveAccount(account);
}
```

```cm:expect
js.function.function-camel-case @ src/domain/account.js:L3-L5
refs.javascript-domain-avoids-infrastructure @ src/domain/account.js:L1
refs.javascript-domain-avoids-infrastructure @ src/domain/account.js:L4
```
