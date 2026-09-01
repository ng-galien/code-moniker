---
name: spring
title: Spring guide
lang: java
blurb: Choose focused executable guidance for stereotypes, MVC, persistence, transactions, or tests
learn_kind: framework
learn_path: languages/java/spring
learn_order: 10
tags: java,spring,mvc,injection,persistence,transactions,aop,testing
published: true
---

# Spring guide

Spring checks in Code Moniker are structural heuristics over declarations and
references, not a semantic Spring container model. Start with one focused child
and adapt its package, naming, and annotation assumptions to the project.

- Stereotypes and injection covers direct simple-name annotations and field
  injection.
- MVC and layering covers explicit source-to-target dependencies.
- Persistence distinguishes direct `@Repository` evidence from unannotated
  Spring Data interfaces.
- Transactions and proxies separates a direct proxy bypass from broad audit
  signals.
- Spring testing covers Spring slices and full-context tests; general JUnit
  conventions remain under Java testing.
- The complete reference pack is broader than these focused pages and must be
  reviewed independently before use; it is not their automatic union.

This index keeps one executable direct-annotation example:

```toml cm:rules
default_rules = false

[[java.class.where]]
id = "controller-suffix"
expr = "any(out_refs, kind = 'annotates' AND target.name =~ ^(Controller|RestController)$) => name =~ Controller$"
message = "Spring controller `{name}` should end with Controller."
```

```java cm:file=src/main/java/com/acme/web/OrdersEndpoint.java
package com.acme.web;

import org.springframework.web.bind.annotation.RestController;

@RestController
public class OrdersEndpoint {}
```

```cm:expect
java.class.controller-suffix @ src/main/java/com/acme/web/OrdersEndpoint.java:L5-L6
```
