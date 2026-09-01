---
name: java-testing
title: Java and JUnit test policies
lang: java
blurb: Treat JUnit naming as an explicit team policy rather than a framework requirement
learn_kind: framework
learn_path: languages/java/testing
learn_order: 40
tags: java,junit,testing,naming
learn_aliases: junit
published: true
---

# Java and JUnit test policies

JUnit Jupiter supports package-visible test methods and does not require a
`test` prefix. Teams may prefer descriptive method names, display names, or a
local prefix. The following rule is intentionally an example team policy, not
a JUnit recommendation.

```toml cm:rules
default_rules = false

[[java.method.where]]
id = "package-tests-start-with-test"
expr = """
  srcset = 'test'
  AND visibility = 'package'
  AND any(out_refs, kind = 'annotates' AND target.name = 'Test')
  => name =~ ^test
"""
message = "This project expects package-visible JUnit method `{name}` to start with test."
```

```java cm:file=src/test/java/com/acme/OrderTest.java
package com.acme;

import org.junit.jupiter.api.Test;

public class OrderTest {
	@Test
	void rejectsNegativeTotal() {}
}
```

```cm:expect
java.method.package-tests-start-with-test @ src/test/java/com/acme/OrderTest.java:L6-L7
```
