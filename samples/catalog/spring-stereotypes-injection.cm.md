---
name: spring-stereotypes-injection
title: Spring stereotypes and injection
lang: java
blurb: Audit direct stereotype annotations and field injection without claiming container semantics
learn_kind: framework
learn_path: languages/java/spring/stereotypes-and-injection
learn_order: 10
tags: java,spring,stereotypes,injection,autowired
published: true
---

# Spring stereotypes and injection

These rules match direct `annotates` references by simple target name. They do
not expand composed or meta-annotated stereotypes. The injection rule covers a
field directly annotated `@Autowired`; it does not identify `@Inject`,
`@Resource`, constructor parameters, Lombok-generated constructors, or runtime
wiring. This focused example demonstrates `@RestController` and `@Autowired`;
the reference pack contains additional direct `@Service`, `@Repository`, and
`@Configuration` policies with the same simple-name limitation.

```toml cm:rules
default_rules = false

[[java.class.where]]
id = "controller-suffix"
expr = "any(out_refs, kind = 'annotates' AND target.name =~ ^(Controller|RestController)$) => name =~ Controller$"
message = "Spring controller `{name}` should end with Controller."

[[java.field.where]]
id = "no-autowired-field"
expr = "none(out_refs, kind = 'annotates' AND target.name = 'Autowired')"
message = "Prefer explicit constructor injection to an @Autowired field."
```

```java cm:file=src/main/java/com/acme/web/OrdersEndpoint.java
package com.acme.web;

import org.springframework.beans.factory.annotation.Autowired;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class OrdersEndpoint {
	@Autowired
	private OrderService service;
}
```

```cm:expect
java.class.controller-suffix @ src/main/java/com/acme/web/OrdersEndpoint.java:L6-L10
java.field.no-autowired-field @ src/main/java/com/acme/web/OrdersEndpoint.java:L9
```
