---
name: spring-transactions-proxies
title: Spring transactions and proxy calls
lang: java
blurb: Detect direct same-class calls to advised methods and explain proxy-mode limits
learn_kind: framework
learn_path: languages/java/spring/transactions-and-proxies
learn_order: 30
tags: java,spring,transactions,aop,proxy,self-invocation
published: true
---

# Spring transactions and proxy calls

In Spring proxy mode, an internal call to an advised method does not enter
through the proxy. The rule below is method-level: it flags an advised method
with an incoming call from the same class. It does not apply to AspectJ weaving
and cannot decide whether the caller actually expects a second interception.

```toml cm:rules
default_rules = false

[[java.method.where]]
id = "transactional-method-in-service"
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
  => parent.name =~ Service$
     OR moniker ~ '**/package:/^(service|application)$/**'
"""
message = "Spring @Transactional method `{name}` should live in the service/application layer."

[[java.method.where]]
id = "advised-method-no-same-class-call"
expr = """
  any(out_refs, kind = 'annotates' AND target.name =~ ^(Transactional|Async|Cacheable)$)
  => none(in_refs,
       (kind = 'method_call' OR kind = 'calls')
       AND source.parent = target.parent
     )
"""
message = "Audit same-class calls to advised method `{name}` in Spring proxy mode."
```

```java cm:file=src/main/java/com/acme/service/WalletService.java
package com.acme.service;

import org.springframework.transaction.annotation.Transactional;

public class WalletService {
	@Transactional
	public void debit() {}

	public void transfer() {
		debit();
	}
}
```

```java cm:file=src/main/java/com/acme/billing/InvoiceWriter.java
package com.acme.billing;

import org.springframework.transaction.annotation.Transactional;

public class InvoiceWriter {
	@Transactional
	public void save() {}
}
```

```cm:expect
java.method.advised-method-no-same-class-call @ src/main/java/com/acme/service/WalletService.java:L6-L7
java.method.transactional-method-in-service @ src/main/java/com/acme/billing/InvoiceWriter.java:L6-L7
```
