---
name: spring-persistence
title: Spring persistence boundaries
lang: java
blurb: Separate direct repository stereotypes from Spring Data interfaces and explicit layer edges
learn_kind: framework
learn_path: languages/java/spring/persistence
learn_order: 25
tags: java,spring,persistence,repository,spring-data,jpa
published: true
---

# Spring persistence boundaries

This recipe checks classes directly annotated `@Repository`. It can enforce a
suffix and package on that explicit evidence. A typical Spring Data interface
extends `Repository`, `CrudRepository`, or `JpaRepository` without carrying the
annotation itself; that interface is intentionally clean here because Code
Moniker does not expand Spring Data inheritance into a synthetic stereotype.

Use explicit `refs` boundaries for controller-to-persistence or
domain-to-adapter edges, as shown in the MVC child. Use the Spring testing child
for `@DataJpaTest` naming.

```sh
code-moniker rules learn spring-testing
```

```toml cm:rules
default_rules = false

[aliases]
repository_pkg = "moniker ~ '**/package:/^(repository|persistence|infrastructure)$/**'"

[[java.class.where]]
id = "repository-shape"
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Repository')
  => name =~ Repository$ AND $repository_pkg
"""
message = "Direct @Repository class `{name}` should use the repository suffix and package."
```

```java cm:file=src/main/java/com/acme/billing/OrderStore.java
package com.acme.billing;

import org.springframework.stereotype.Repository;

@Repository
public class OrderStore {}
```

```java cm:file=src/main/java/com/acme/repository/SpringDataOrders.java
package com.acme.repository;

import org.springframework.data.jpa.repository.JpaRepository;

public interface SpringDataOrders extends JpaRepository<OrderEntity, Long> {}
```

```cm:expect
java.class.repository-shape @ src/main/java/com/acme/billing/OrderStore.java:L5-L6
```
