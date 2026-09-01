---
name: architecture
title: Architecture and design patterns
blurb: Choose direct boundaries, transitive workspace paths, or a named architecture pattern
learn_kind: pattern
learn_path: architecture
tags: architecture,boundaries,dependencies,workspace,patterns
published: true
---

# Architecture and design patterns

Architecture checks begin with the relationship being constrained. Use
language or generic `refs` rules for direct dependencies; use workspace paths
when the question is transitive; open a named child only when its assumptions
match the project.

Direct and transitive checks are deliberately separate:

```sh
code-moniker rules learn java-layer-boundaries
```

```sh
code-moniker rules learn workspace-path
```

This minimal executable boundary prohibits a direct API-to-persistence
reference. It does not prove an architecture style by itself.

```toml cm:rules
default_rules = false

[aliases]
src_api = "source ~ '**/package:api/**'"
tgt_persistence = "target ~ '**/package:persistence/**'"

[[java.refs.where]]
id = "api-no-persistence-direct"
expr = "$src_api => NOT $tgt_persistence"
message = "API code should not depend directly on persistence."
```

```java cm:file=src/main/java/com/acme/persistence/OrderStore.java
package com.acme.persistence;

public class OrderStore {}
```

```java cm:file=src/main/java/com/acme/api/OrderController.java
package com.acme.api;

import com.acme.persistence.OrderStore;

public class OrderController {
}
```

```cm:expect
java.refs.api-no-persistence-direct @ src/main/java/com/acme/api/OrderController.java:L3
```
