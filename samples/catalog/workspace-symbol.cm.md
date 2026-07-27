---
name: workspace-symbol
lang: java
blurb: Workspace-wide placement rules over the symbol inventory
published: true
default_rules: false
---

# Workspace symbol placement

Workspace rules inspect every active symbol in the snapshot, independently of
the file in which it was extracted. This rule requires repository types to
live below an `infra` directory.

```toml cm:rules
[[workspace.symbol.where]]
id        = "repositories-under-infra"
severity  = "warn"
expr      = "(shape = 'type' AND name =~ Repository$) => (uri ~ '**/dir:infra/**' OR uri ~ '**/package:infra/**')"
message   = "Repository types must live below infra."
rationale = "Repository placement is a workspace-wide architecture invariant."
```

This repository is correctly placed:

```java cm:file=src/main/java/com/acme/infra/GoodRepository.java
package com.acme.infra;

public class GoodRepository {}
```

This repository violates the workspace rule:

```java cm:file=src/main/java/com/acme/domain/BadRepository.java
package com.acme.domain;

public class BadRepository {}
```

```cm:expect
workspace.symbol.repositories-under-infra @ src/main/java/com/acme/domain/BadRepository.java:L3
```
