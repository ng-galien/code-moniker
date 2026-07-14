---
name: directives
title: Layout, requirement, and segment directives
summary: Use vertical_layout for declaration order, require for correlated-existence, and has_segment for moniker location.
---

# Layout, Requirement, And Segment Directives

Three predicates go beyond comparing one projection:

- `vertical_layout(<domain>, <policy>)` checks declaration **order** within a
  def. `public_first` wants public members before private ones;
  `private_after_first_use` keeps a private member close to its first caller,
  with an optional `max_gap = N`.
- `require('<uri-pattern>')` asserts that a **derived** URI exists. The pattern
  is rendered in the current item context, with `{name}` and `{name.snake}`
  placeholders. A regex segment such as `method:/^build/` tolerates the call
  arity that the extractor records (`build()`).
- `has_segment('<kind>', '<name>')` is sugar for
  `moniker ~ '**/<kind>:<name>/**'`; it tests **where** a symbol lives.

```toml cm:rules
default_rules = false

[[java.class.where]]
id        = "public-methods-first"
rationale = "vertical_layout with public_first reports a class that declares a private method before its public surface."
expr      = "vertical_layout(method, public_first)"

[[java.class.where]]
id        = "needs-build-method"
rationale = "require renders {name} into a child path, so each class must declare its own build method. The rule passes only when that local def exists."
expr      = "require('**/class:{name}/method:/^build/')"

[[java.class.where]]
id        = "no-legacy-package"
rationale = "has_segment('package', 'legacy') is true for any class under the legacy package, so this rule reports them."
expr      = "NOT has_segment('package', 'legacy')"
```

`Account` is ordered public-first, declares a `build` method, and lives under
`app`, so it passes all three. `Gadget` declares a private `helper` before its
public `draw` and has no `build`. `Old` lives under the `legacy` package.

```java cm:file=src/main/java/app/Account.java
package app;

public class Account {
	public int build() {
		return 0;
	}

	public int balance() {
		return 0;
	}
}
```

```java cm:file=src/main/java/app/Gadget.java
package app;

public class Gadget {
	private int helper() {
		return 0;
	}

	public int draw() {
		return helper();
	}
}
```

```java cm:file=src/main/java/legacy/Old.java
package legacy;

public class Old {
	public int build() {
		return 1;
	}
}
```

```cm:expect
java.class.needs-build-method @ src/main/java/app/Gadget.java:L3-L11
java.class.public-methods-first @ src/main/java/app/Gadget.java:L3-L11
java.class.no-legacy-package @ src/main/java/legacy/Old.java:L3-L7
```
