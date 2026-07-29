---
name: workspace-path
title: Transitive workspace paths
lang: java
blurb: Enforce architecture across a hot index with bounded, confidence-aware paths
published: true
default_rules: false
---

# Transitive workspace paths

`workspace.path` checks the resolved graph for a complete workspace generation.
Unlike `refs.where`, which sees one direct reference at a time, a path rule can
protect a boundary across several calls and return the concrete witness when
the boundary is crossed.

The same rules run on an ephemeral linked snapshot with the direct CLI, or on
the selected hot index through a daemon or the MCP `rules` tool. Daemon and MCP
responses name the `daemon_index` corpus and the exact generation they used.

Run this self-contained fixture with:

```sh
code-moniker check . --scenario samples/catalog/workspace-path.cm.md
```

To experiment on a real hot workspace, copy the TOML block to a scratch rules
file and call MCP `code_moniker_rules` with `action:"run"`,
`rules:"<scratch.toml>"`, and `report:true`. That request reloads the rules but
keeps the source corpus pinned to the reported `daemon_index` generation.

```toml cm:rules
default_rules = false

[workspace]
min_linkage_coverage = 100

[[workspace.path]]
id = "application-must-not-reach-infrastructure"
severity = "error"
from = "uri ~ '**/package:application/**' AND kind = 'method' AND name =~ ^place"
to = "uri ~ '**/package:infrastructure/**' AND kind = 'method' AND name =~ ^save"
expect = "no_path"
relation = ["calls", "method_call"]
max_depth = 8
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "Application reaches infrastructure through {path}."
rationale = "Application code should depend on domain ports rather than concrete infrastructure."

[[workspace.path]]
id = "controller-reaches-domain-policy"
severity = "error"
from = "uri ~ '**/package:presentation/**' AND kind = 'method' AND name =~ ^submit"
to = "uri ~ '**/package:domain/**' AND kind = 'method' AND name =~ ^validate"
expect = "reachable"
relation = ["calls", "method_call"]
max_depth = 8
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "The controller cannot reach the domain policy."
rationale = "A delivery entry point must reach the use case and its domain policy."

[[workspace.path]]
id = "domain-must-not-reach-presentation"
severity = "error"
from = "uri ~ '**/package:domain/**' AND kind = 'method'"
to = "uri ~ '**/package:presentation/**' AND kind = 'method'"
expect = "no_path"
relation = ["calls", "method_call"]
max_depth = 8
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "Domain reaches presentation through {path}."
rationale = "The domain must remain independent from delivery mechanisms."

[[workspace.path]]
id = "controller-paths-cross-domain-policy"
severity = "error"
from = "uri ~ '**/package:presentation/**' AND kind = 'method' AND name =~ ^submit"
to = "uri ~ '**/package:infrastructure/**' AND kind = 'method' AND name =~ ^save"
via = "uri ~ '**/package:domain/**' AND kind = 'method' AND name =~ ^validate"
expect = "all_paths_via"
relation = ["calls", "method_call"]
max_depth = 8
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "A controller-to-infrastructure path bypasses the domain policy: {path}."
rationale = "Every reachable delivery-to-storage path must cross the selected domain policy."

[[workspace.path]]
id = "bypass-must-cross-domain-policy"
severity = "error"
from = "uri ~ '**/package:presentation/**' AND kind = 'method' AND name =~ ^bypass"
to = "uri ~ '**/package:infrastructure/**' AND kind = 'method' AND name =~ ^save"
via = "uri ~ '**/package:domain/**' AND kind = 'method' AND name =~ ^validate"
expect = "all_paths_via"
relation = ["calls", "method_call"]
max_depth = 8
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "A controller-to-infrastructure path bypasses the domain policy: {path}."
rationale = "The failing witness is the concrete path that remains after removing the boundary."

[[workspace.path]]
id = "short-budget-is-inconclusive"
severity = "warn"
from = "uri ~ '**/package:presentation/**' AND kind = 'method' AND name =~ ^submit"
to = "uri ~ '**/package:infrastructure/**' AND kind = 'method' AND name =~ ^save"
expect = "reachable"
relation = ["calls", "method_call"]
max_depth = 1
max_symbols = 1000
max_edges = 5000
max_pairs = 100
message = "The bounded search could not prove the expected path."
rationale = "A traversal budget must produce inconclusive, never a false pass."
```

## A transitive architecture violation

The presentation controller calls the application use case:

```java cm:file=src/main/java/com/acme/presentation/OrderController.java
package com.acme.presentation;

import com.acme.application.PlaceOrder;
import com.acme.infrastructure.SqlOrders;

public final class OrderController {
	public static void submit() {
		PlaceOrder.place();
	}

	public static void bypass() {
		SqlOrders.save();
	}
}
```

The application delegates to a domain policy:

```java cm:file=src/main/java/com/acme/application/PlaceOrder.java
package com.acme.application;

import com.acme.domain.OrderPolicy;

public final class PlaceOrder {
	public static void place() {
		OrderPolicy.validate();
	}
}
```

The domain policy improperly calls concrete infrastructure. The forbidden
application-to-infrastructure dependency is therefore transitive rather than
one direct reference:

```java cm:file=src/main/java/com/acme/domain/OrderPolicy.java
package com.acme.domain;

import com.acme.infrastructure.SqlOrders;

public final class OrderPolicy {
	public static void validate() {
		SqlOrders.save();
	}
}
```

```java cm:file=src/main/java/com/acme/infrastructure/SqlOrders.java
package com.acme.infrastructure;

public final class SqlOrders {
	public static void save() {}
}
```

With `--format json --report`, the first rule fails with the minimal
`PlaceOrder.place -> OrderPolicy.validate -> SqlOrders.save` witness. The
controller-to-domain and protected-boundary rules pass, the explicit bypass
fails with its direct call as witness, the reverse domain-to-presentation rule
passes after a complete search, and the one-hop budget is reported as
`inconclusive`.

```cm:expect
workspace.path.application-must-not-reach-infrastructure @ src/main/java/com/acme/application/PlaceOrder.java:L6-L8
workspace.path.bypass-must-cross-domain-policy @ src/main/java/com/acme/presentation/OrderController.java:L11-L13
verdict workspace.path.application-must-not-reach-infrastructure = fail
verdict workspace.path.controller-reaches-domain-policy = pass
verdict workspace.path.domain-must-not-reach-presentation = pass
verdict workspace.path.controller-paths-cross-domain-policy = pass
verdict workspace.path.bypass-must-cross-domain-policy = fail
verdict workspace.path.short-budget-is-inconclusive = inconclusive
```
