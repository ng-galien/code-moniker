---
name: java
title: Java language guide
lang: java
blurb: Start with Java rule namespaces, then open Spring, testing, qualified types, or layer boundaries
learn_kind: language
learn_path: languages/java
learn_order: 30
tags: java,naming,imports,fqn,spring,layering
published: true
---

# Java language guide

Java graph rules use the `java.*` namespace. Begin with extracted declarations
and references, then choose the narrower child that matches the question:

- Spring contains opt-in framework heuristics and states what they do not model.
- Java testing keeps JUnit naming separate from Spring-specific test slices.
- Qualified types explains package-qualified names, explicit-import ambiguity,
  and nested types.
- Layer boundaries demonstrates direct Java dependency checks.

Inspect Java's executable vocabulary before adapting a recipe:

```sh
code-moniker langs java
```

The small rule below is language-level rather than framework-specific.

```toml cm:rules
default_rules = false

[[java.class.where]]
id = "class-pascal-case"
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Java class `{name}` should use PascalCase."
```

```java cm:file=src/main/java/com/acme/order/order_service.java
package com.acme.order;

public class order_service {}
```

```cm:expect
java.class.class-pascal-case @ src/main/java/com/acme/order/order_service.java:L3
```
