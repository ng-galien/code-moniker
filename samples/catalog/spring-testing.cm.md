---
name: spring-testing
title: Spring test slices and context tests
lang: java
blurb: Keep Spring slice and full-context test policies separate from general JUnit naming
learn_kind: framework
learn_path: languages/java/spring/testing
learn_order: 40
tags: java,spring,testing,webmvctest,datajpatest,springboottest
published: true
---

# Spring test slices and context tests

`@WebMvcTest` and `@DataJpaTest` are focused slices; `@SpringBootTest` loads a
broader application context. Code Moniker matches the direct annotation name
and applies a project naming policy. It does not run the context or infer
composed test annotations.

```toml cm:rules
default_rules = false

[[java.class.where]]
id = "webmvc-test-suffix"
expr = "any(out_refs, kind = 'annotates' AND target.name = 'WebMvcTest') => name =~ (ControllerTest|WebMvcTest)$"
message = "Spring MVC slice test `{name}` should identify the tested slice."

[[java.class.where]]
id = "datajpa-test-suffix"
expr = "any(out_refs, kind = 'annotates' AND target.name = 'DataJpaTest') => name =~ (RepositoryTest|DataJpaTest)$"
message = "Spring Data JPA slice test `{name}` should identify the persistence slice."

[[java.class.where]]
id = "springboot-test-suffix"
expr = "any(out_refs, kind = 'annotates' AND target.name = 'SpringBootTest') => name =~ (IntegrationTest|SpringBootTest)$"
message = "Full-context Spring test `{name}` should identify its integration scope."
```

```java cm:file=src/test/java/com/acme/web/OrdersCheck.java
package com.acme.web;

import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;

@WebMvcTest
public class OrdersCheck {}
```

```java cm:file=src/test/java/com/acme/repository/OrdersRepositoryCheck.java
package com.acme.repository;

import org.springframework.boot.test.autoconfigure.orm.jpa.DataJpaTest;

@DataJpaTest
public class OrdersRepositoryCheck {}
```

```java cm:file=src/test/java/com/acme/ApplicationSmoke.java
package com.acme;

import org.springframework.boot.test.context.SpringBootTest;

@SpringBootTest
public class ApplicationSmoke {}
```

```cm:expect
java.class.webmvc-test-suffix @ src/test/java/com/acme/web/OrdersCheck.java:L5-L6
java.class.datajpa-test-suffix @ src/test/java/com/acme/repository/OrdersRepositoryCheck.java:L5-L6
java.class.springboot-test-suffix @ src/test/java/com/acme/ApplicationSmoke.java:L5-L6
```
