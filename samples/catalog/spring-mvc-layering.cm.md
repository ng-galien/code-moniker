---
name: spring-mvc-layering
title: Spring MVC and layering
lang: java
blurb: Keep controllers away from persistence using explicit dependency evidence
learn_kind: framework
learn_path: languages/java/spring/mvc-and-layering
learn_order: 20
tags: java,spring,mvc,controllers,persistence,layering
published: true
---

# Spring MVC and layering

Code Moniker does not infer a complete MVC model. This recipe treats files in
`web/`, `api/`, or `controller/` as presentation sources and files in
`repository/`, `persistence/`, or `infrastructure/` as persistence targets.
The finding is an observed graph edge, not a claim about Spring runtime wiring.

```toml cm:rules
default_rules = false

[aliases]
src_controller = "source ~ '**/package:/^(web|api|controller)$/**'"
tgt_persistence = "target ~ '**/package:/^(repository|persistence|infrastructure)$/**'"

[[java.refs.where]]
id = "controller-no-persistence-direct"
expr = "$src_controller => NOT $tgt_persistence"
message = "A Spring MVC controller should delegate through an application service."
```

```java cm:file=src/main/java/com/acme/repository/OrderRepository.java
package com.acme.repository;

public interface OrderRepository {}
```

```java cm:file=src/main/java/com/acme/web/OrderController.java
package com.acme.web;

import com.acme.repository.OrderRepository;

public class OrderController {}
```

```cm:expect
java.refs.controller-no-persistence-direct @ src/main/java/com/acme/web/OrderController.java:L3
```
