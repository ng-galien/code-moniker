---
name: java
lang: java
blurb: Java naming, size budgets, layering, and Spring conventions
published: true
---

# Java conventions

A Java-flavoured rule set: PascalCase classes, JUnit naming, class size
budgets, a domain/infrastructure boundary, and a battery of Spring
conventions (stereotype suffixes and packages, injection style, transaction
placement, proxy self-invocation, test slices). The layout below is a small
Maven-style project where each rule is broken exactly once.

```toml cm:rules
default_rules = false

[aliases]
java_main = "moniker ~ '**/srcset:main/**'"
java_test = "moniker ~ '**/srcset:test/**'"

api_pkg = "moniker ~ '**/package:api/**'"
domain_pkg = "moniker ~ '**/package:domain/**'"
infra_pkg = "moniker ~ '**/package:infrastructure/**'"
service_pkg = "moniker ~ '**/package:/^(service|application|domain)$/**'"
repository_pkg = "moniker ~ '**/package:/^(repository|persistence|infrastructure)$/**'"
config_pkg = "moniker ~ '**/package:/^(config|configuration)$/**'"

src_domain = "source ~ '**/package:domain/**'"
tgt_infra = "target ~ '**/package:infrastructure/**'"

[[java.class.where]]
id = "class-pascalcase"
rationale = "PascalCase makes Java classes read as named types and keeps them aligned with the language's everyday conventions."
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Class `{name}` must use PascalCase."

[[java.method.where]]
id = "package-junit-methods-start-with-test"
rationale = "A package-visible JUnit method is easiest to understand when its name clearly announces that it is a test."
expr = """
  $java_test
  AND visibility = 'package'
  AND any(out_refs, kind = 'annotates' AND target.name = 'Test')
  => name =~ ^test
"""
message = "Package-visible JUnit method `{name}` must start with test."

[[java.class.where]]
id = "main-classes-not-test-suffixed"
rationale = "Production classes and test classes should be easy to tell apart from their names alone."
expr = "$java_main => name !~ Test$"
message = "Production class `{name}` must not end with Test."

[[java.class.where]]
id = "class-budget"
rationale = "A class with too many methods is hard to learn and hard to change safely. Use this as an early signal to split responsibilities."
expr = "count(method) <= 25 AND all(method, lines <= 80)"
message = "Class `{name}` is too large."

[[java.refs.where]]
id = "domain-no-infra"
rationale = "Domain code should not know persistence or delivery details. Keep infrastructure behind service or application boundaries."
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."

[[java.refs.where]]
id = "no-unnecessary-qualified-type-name"
rationale = "A fully qualified Java type name is useful when the simple name is ambiguous. Otherwise, imports keep code shorter and easier to scan."
expr = "kind != 'uses_type' OR text = target.name OR any(source.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target) OR any(source.ancestors.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target)"
message = "Qualified Java type reference can use simple name `{target.name}` here."

[[java.class.where]]
id = "spring-controller-suffix"
rationale = "A controller suffix tells readers that the class handles web/API traffic."
expr = """
  any(out_refs,
    kind = 'annotates'
    AND target.name =~ ^(Controller|RestController)$
  )
  => name =~ Controller$
"""
message = "Spring controller `{name}` should end with Controller."

[[java.class.where]]
id = "spring-controller-package"
rationale = "Putting controllers in API or web packages keeps presentation code visible at the edge of the application."
expr = """
  any(out_refs,
    kind = 'annotates'
    AND target.name =~ ^(Controller|RestController)$
  )
  => moniker ~ '**/package:/^(api|web|controller)$/**'
"""
message = "Spring controller `{name}` should live in an API, web, or controller package."

[[java.class.where]]
id = "spring-service-suffix"
rationale = "A service suffix makes orchestration classes easy to find and separates them from entities or adapters."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Service')
  => name =~ Service$
"""
message = "Spring service `{name}` should end with Service."

[[java.class.where]]
id = "spring-service-package"
rationale = "Service classes should live near application behavior, not inside web or persistence packages."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Service')
  => $service_pkg
"""
message = "Spring service `{name}` should live in a service, application, or domain package."

[[java.class.where]]
id = "spring-repository-suffix"
rationale = "A repository suffix makes persistence-facing components obvious to readers."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Repository')
  => name =~ Repository$
"""
message = "Spring repository `{name}` should end with Repository."

[[java.class.where]]
id = "spring-repository-package"
rationale = "Repositories belong close to persistence concerns so storage details do not spread through feature packages."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Repository')
  => $repository_pkg
"""
message = "Spring repository `{name}` should live in a repository, persistence, or infrastructure package."

[[java.class.where]]
id = "spring-configuration-suffix"
rationale = "Configuration classes wire the application together. A clear suffix helps readers distinguish wiring from feature code."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Configuration')
  => name =~ (Config|Configuration)$
"""
message = "Spring configuration `{name}` should end with Config or Configuration."

[[java.class.where]]
id = "spring-configuration-package"
rationale = "Keeping configuration in a dedicated package makes startup wiring easier to find and review."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Configuration')
  => $config_pkg
"""
message = "Spring configuration `{name}` should live in a config or configuration package."

[[java.class.where]]
id = "spring-transactional-not-controller"
rationale = "Transactions are easier to reason about in service-layer code than in presentation handlers."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
  => NOT any(out_refs,
       kind = 'annotates'
       AND target.name =~ ^(Controller|RestController)$
     )
"""
message = "Spring controller `{name}` should not be annotated @Transactional."

[[java.class.where]]
id = "spring-controller-no-repository-direct"
rationale = "Controllers should delegate work to application services instead of reaching straight into persistence."
expr = """
  any(out_refs,
    kind = 'annotates'
    AND target.name =~ ^(Controller|RestController)$
  )
  => none(out_refs,
       target.name =~ Repository$
       OR target.name =~ ^(EntityManager|JdbcTemplate|DSLContext)$
     )
"""
message = "Spring controller `{name}` should not depend directly on repositories or persistence APIs."

[[java.class.where]]
id = "spring-service-no-web"
rationale = "Services should be callable from more than one adapter. Depending on controllers pulls them back toward the web layer."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Service')
  => none(out_refs,
       target.name =~ Controller$
       OR target ~ '**/package:/^(api|web|controller)$/**'
     )
"""
message = "Spring service `{name}` should not depend on controllers or web packages."

[[java.class.where]]
id = "spring-repository-no-web"
rationale = "Repositories are persistence components. They should not depend on controllers or web adapters."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Repository')
  => none(out_refs,
       target.name =~ Controller$
       OR target ~ '**/package:/^(api|web|controller)$/**'
     )
"""
message = "Spring repository `{name}` should not depend on controllers or web packages."

[[java.field.where]]
id = "spring-no-field-injection"
rationale = "Constructor injection makes dependencies explicit and keeps tests simpler."
expr = "none(out_refs, kind = 'annotates' AND target.name = 'Autowired')"
message = "Spring field `{name}` should not use @Autowired field injection; prefer constructor injection."

[[java.method.where]]
id = "spring-bean-methods-in-configuration"
rationale = "Bean factory methods are application wiring. Keeping them on configuration classes makes startup behavior easier to inspect."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Bean')
  => parent.name =~ (Config|Configuration)$
"""
message = "Spring @Bean method `{name}` should live on a configuration class."

[[java.method.where]]
id = "spring-transactional-methods-in-service"
rationale = "Method-level transactions should mark application work, not controller handling or low-level helpers."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'Transactional')
  => parent.name =~ Service$
     OR moniker ~ '**/package:/^(service|application)$/**'
"""
message = "Spring @Transactional method `{name}` should live in the service/application layer."

[[java.method.where]]
id = "spring-proxy-method-no-self-invocation"
rationale = "Spring proxy advice does not run on same-class calls. Move the advised call behind another bean or boundary."
expr = """
  any(out_refs,
    kind = 'annotates'
    AND target.name =~ ^(Transactional|Async|Cacheable|CachePut|CacheEvict|Caching|Retryable|ConcurrencyLimit|PreAuthorize|PostAuthorize|PreFilter|PostFilter|Secured|RolesAllowed)$
  )
  => none(in_refs,
       (kind = 'method_call' OR kind = 'calls')
       AND source.parent = target.parent
     )
"""
message = "Spring proxy-advised method `{name}` should not be called from the same class; the call bypasses proxy advice."

[[java.class.where]]
id = "spring-proxy-class-no-self-invocation"
rationale = "A class with advised methods should avoid calling those methods through `this`, because Spring proxy behavior can be skipped."
expr = """
  any(out_refs,
    kind = 'annotates'
    AND target.name =~ ^(Transactional|Async|Cacheable|CachePut|CacheEvict|Caching|Retryable|ConcurrencyLimit|PreAuthorize|PostAuthorize|PreFilter|PostFilter|Secured|RolesAllowed)$
  )
  => none(method,
       any(in_refs,
         (kind = 'method_call' OR kind = 'calls')
         AND source.parent = target.parent
       )
     )
"""
message = "Spring proxy-advised class `{name}` should not make same-class calls to advised methods; the call bypasses proxy advice."

[[java.class.where]]
id = "spring-webmvc-test-suffix"
rationale = "A Web MVC slice test should say which web slice it exercises before the reader opens the class."
expr = """
  $java_test
  AND any(out_refs, kind = 'annotates' AND target.name = 'WebMvcTest')
  => name =~ (ControllerTest|ControllerTests|WebMvcTest|WebMvcTests)$
"""
message = "Spring MVC slice test `{name}` should use an explicit controller/WebMvc test suffix."

[[java.class.where]]
id = "spring-datajpa-test-suffix"
rationale = "A JPA slice test should make its repository or persistence focus visible in the class name."
expr = """
  $java_test
  AND any(out_refs, kind = 'annotates' AND target.name = 'DataJpaTest')
  => name =~ (RepositoryTest|RepositoryTests|DataJpaTest|DataJpaTests)$
"""
message = "Spring Data JPA slice test `{name}` should use an explicit repository/DataJpa test suffix."

[[java.class.where]]
id = "spring-boot-test-suffix"
rationale = "A full application-context test is heavier than a unit or slice test, so its name should make that role clear."
expr = """
  $java_test
  AND any(out_refs, kind = 'annotates' AND target.name = 'SpringBootTest')
  => name =~ (Test|Tests|IT)$
"""
message = "Spring Boot test `{name}` should use a Test, Tests, or IT suffix."

[[java.class.where]]
id = "spring-boot-test-not-controller-slice"
rationale = "Use a broad Spring Boot test or a focused MVC slice test, not both on the same class."
expr = """
  any(out_refs, kind = 'annotates' AND target.name = 'SpringBootTest')
  => NOT any(out_refs, kind = 'annotates' AND target.name = 'WebMvcTest')
"""
message = "Use either @SpringBootTest or @WebMvcTest on `{name}`, not both."
```

## Naming and size budgets

A snake_case class, a production class that looks like a test, and a class
that blows the 25-method budget:

```java cm:file=src/main/java/com/acme/billing/legacy_gateway.java
package com.acme.billing;

public class legacy_gateway {
	public String send() {
		return "ok";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/InvoiceTest.java
package com.acme.billing;

public class InvoiceTest {
	public String number() {
		return "INV-1";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/BulkLoader.java
package com.acme.billing;

public class BulkLoader {
	void m01() {}
	void m02() {}
	void m03() {}
	void m04() {}
	void m05() {}
	void m06() {}
	void m07() {}
	void m08() {}
	void m09() {}
	void m10() {}
	void m11() {}
	void m12() {}
	void m13() {}
	void m14() {}
	void m15() {}
	void m16() {}
	void m17() {}
	void m18() {}
	void m19() {}
	void m20() {}
	void m21() {}
	void m22() {}
	void m23() {}
	void m24() {}
	void m25() {}
	void m26() {}
}
```

## Layering

Domain code reaches into infrastructure — a real cross-file ref:

```java cm:file=src/main/java/com/acme/domain/Order.java
package com.acme.domain;

import com.acme.infrastructure.OrderTable;

public class Order {
	public String id() {
		return new OrderTable().key();
	}
}
```

```java cm:file=src/main/java/com/acme/infrastructure/OrderTable.java
package com.acme.infrastructure;

public class OrderTable {
	public String key() {
		return "order";
	}
}
```

## Qualified type names

Fully qualified type names are reserved for real simple-name ambiguity:

```java cm:file=src/main/java/com/acme/billing/BillingClock.java
package com.acme.billing;

public class BillingClock {
	private java.time.LocalDate businessDate;
}
```

## Spring stereotypes: suffixes and packages

Each stereotype gets one class breaking the suffix and one breaking the
package. `OrderEndpoint` sits in the right package with the wrong name;
`PaymentController` has the right name in the wrong package — and so on for
services, repositories, and configuration:

```java cm:file=src/main/java/com/acme/web/OrderEndpoint.java
package com.acme.web;

import org.springframework.web.bind.annotation.RestController;

@RestController
public class OrderEndpoint {
	public String orders() {
		return "[]";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/PaymentController.java
package com.acme.billing;

import org.springframework.web.bind.annotation.RestController;

@RestController
public class PaymentController {
	public String pay() {
		return "paid";
	}
}
```

```java cm:file=src/main/java/com/acme/service/OrderManager.java
package com.acme.service;

import org.springframework.stereotype.Service;

@Service
public class OrderManager {
	public String place() {
		return "placed";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/BillingService.java
package com.acme.billing;

import org.springframework.stereotype.Service;

@Service
public class BillingService {
	public String bill() {
		return "billed";
	}
}
```

```java cm:file=src/main/java/com/acme/repository/OrderStore.java
package com.acme.repository;

import org.springframework.stereotype.Repository;

@Repository
public class OrderStore {
	public String load() {
		return "order";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/BillingRepository.java
package com.acme.billing;

import org.springframework.stereotype.Repository;

@Repository
public class BillingRepository {
	public String load() {
		return "bill";
	}
}
```

```java cm:file=src/main/java/com/acme/config/AppSetup.java
package com.acme.config;

import org.springframework.context.annotation.Configuration;

@Configuration
public class AppSetup {
}
```

```java cm:file=src/main/java/com/acme/billing/BillingConfig.java
package com.acme.billing;

import org.springframework.context.annotation.Configuration;

@Configuration
public class BillingConfig {
}
```

## Spring layering and injection style

A controller marked `@Transactional`, a controller talking to a repository
directly, a service and a repository depending back on the web layer, and an
`@Autowired` field:

```java cm:file=src/main/java/com/acme/web/CheckoutController.java
package com.acme.web;

import org.springframework.transaction.annotation.Transactional;
import org.springframework.web.bind.annotation.RestController;

@RestController
@Transactional
public class CheckoutController {
	public String checkout() {
		return "done";
	}
}
```

```java cm:file=src/main/java/com/acme/web/ReportController.java
package com.acme.web;

import com.acme.repository.OrderRepository;
import org.springframework.web.bind.annotation.RestController;

@RestController
public class ReportController {
	private final OrderRepository orders;

	public ReportController(OrderRepository orders) {
		this.orders = orders;
	}
}
```

```java cm:file=src/main/java/com/acme/repository/OrderRepository.java
package com.acme.repository;

public interface OrderRepository {
	String find(String id);
}
```

```java cm:file=src/main/java/com/acme/service/ReportService.java
package com.acme.service;

import com.acme.web.OrderEndpoint;
import org.springframework.stereotype.Service;

@Service
public class ReportService {
	private final OrderEndpoint endpoint = new OrderEndpoint();

	public String report() {
		return endpoint.orders();
	}
}
```

```java cm:file=src/main/java/com/acme/repository/AuditRepository.java
package com.acme.repository;

import com.acme.web.OrderEndpoint;
import org.springframework.stereotype.Repository;

@Repository
public class AuditRepository {
	private final OrderEndpoint endpoint = new OrderEndpoint();

	public String snapshot() {
		return endpoint.orders();
	}
}
```

```java cm:file=src/main/java/com/acme/billing/LegacyBean.java
package com.acme.billing;

import com.acme.repository.OrderStore;
import org.springframework.beans.factory.annotation.Autowired;

public class LegacyBean {
	@Autowired
	private OrderStore store;
}
```

## Transaction and proxy placement

A `@Bean` method outside a configuration class, a `@Transactional` method
outside the service layer, and the two proxy self-invocation traps:

```java cm:file=src/main/java/com/acme/billing/Beans.java
package com.acme.billing;

import org.springframework.context.annotation.Bean;

public class Beans {
	@Bean
	public String clock() {
		return "utc";
	}
}
```

```java cm:file=src/main/java/com/acme/billing/InvoiceWriter.java
package com.acme.billing;

import org.springframework.transaction.annotation.Transactional;

public class InvoiceWriter {
	@Transactional
	public void save() {
	}
}
```

`WalletService.transfer` calls its own advised `debit` method — the call
bypasses the Spring proxy:

```java cm:file=src/main/java/com/acme/service/WalletService.java
package com.acme.service;

import org.springframework.stereotype.Service;
import org.springframework.transaction.annotation.Transactional;

@Service
public class WalletService {
	@Transactional
	public void debit(int amount) {
	}

	public void transfer(int amount) {
		debit(amount);
	}
}
```

`LedgerKeeper` is advised at class level and still makes same-class calls:

```java cm:file=src/main/java/com/acme/billing/LedgerKeeper.java
package com.acme.billing;

import org.springframework.transaction.annotation.Transactional;

@Transactional
public class LedgerKeeper {
	public void post() {
		validate();
	}

	public void validate() {
	}
}
```

## Test sources

A package-visible JUnit method that does not start with `test`, three badly
suffixed slice tests, and a test mixing `@SpringBootTest` with `@WebMvcTest`:

```java cm:file=src/test/java/com/acme/billing/OrderTotalTest.java
package com.acme.billing;

import org.junit.jupiter.api.Test;

public class OrderTotalTest {
	@Test
	void checksTotal() {
	}

	@Test
	void testRejectsNegative() {
	}
}
```

```java cm:file=src/test/java/com/acme/web/OrderEndpointCheck.java
package com.acme.web;

import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;

@WebMvcTest
public class OrderEndpointCheck {
	public void rendersOrders() {
	}
}
```

```java cm:file=src/test/java/com/acme/repository/OrderStoreCheck.java
package com.acme.repository;

import org.springframework.boot.test.autoconfigure.orm.jpa.DataJpaTest;

@DataJpaTest
public class OrderStoreCheck {
	public void loadsOrders() {
	}
}
```

```java cm:file=src/test/java/com/acme/BootSmoke.java
package com.acme;

import org.springframework.boot.test.context.SpringBootTest;

@SpringBootTest
public class BootSmoke {
	public void contextLoads() {
	}
}
```

```java cm:file=src/test/java/com/acme/web/MixedWebMvcTest.java
package com.acme.web;

import org.springframework.boot.test.autoconfigure.web.servlet.WebMvcTest;
import org.springframework.boot.test.context.SpringBootTest;

@SpringBootTest
@WebMvcTest
public class MixedWebMvcTest {
	public void rendersOrders() {
	}
}
```

```cm:expect
java.method.spring-bean-methods-in-configuration @ src/main/java/com/acme/billing/Beans.java:L6-L9
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/billing/BillingClock.java:L4
java.class.spring-configuration-package @ src/main/java/com/acme/billing/BillingConfig.java:L5-L7
java.class.spring-repository-package @ src/main/java/com/acme/billing/BillingRepository.java:L5-L10
java.class.spring-service-package @ src/main/java/com/acme/billing/BillingService.java:L5-L10
java.class.class-budget @ src/main/java/com/acme/billing/BulkLoader.java:L3-L30
java.class.main-classes-not-test-suffixed @ src/main/java/com/acme/billing/InvoiceTest.java:L3-L7
java.method.spring-transactional-methods-in-service @ src/main/java/com/acme/billing/InvoiceWriter.java:L6-L8
java.class.spring-proxy-class-no-self-invocation @ src/main/java/com/acme/billing/LedgerKeeper.java:L5-L13
java.field.spring-no-field-injection @ src/main/java/com/acme/billing/LegacyBean.java:L8
java.class.spring-controller-package @ src/main/java/com/acme/billing/PaymentController.java:L5-L10
java.class.class-pascalcase @ src/main/java/com/acme/billing/legacy_gateway.java:L3-L7
java.class.spring-configuration-suffix @ src/main/java/com/acme/config/AppSetup.java:L5-L7
java.refs.domain-no-infra @ src/main/java/com/acme/domain/Order.java:L3
java.refs.domain-no-infra @ src/main/java/com/acme/domain/Order.java:L7
java.refs.domain-no-infra @ src/main/java/com/acme/domain/Order.java:L7
java.refs.domain-no-infra @ src/main/java/com/acme/domain/Order.java:L7
java.class.spring-repository-no-web @ src/main/java/com/acme/repository/AuditRepository.java:L6-L13
java.class.spring-repository-suffix @ src/main/java/com/acme/repository/OrderStore.java:L5-L10
java.class.spring-service-suffix @ src/main/java/com/acme/service/OrderManager.java:L5-L10
java.class.spring-service-no-web @ src/main/java/com/acme/service/ReportService.java:L6-L13
java.method.spring-proxy-method-no-self-invocation @ src/main/java/com/acme/service/WalletService.java:L8-L10
java.class.spring-transactional-not-controller @ src/main/java/com/acme/web/CheckoutController.java:L6-L12
java.class.spring-controller-suffix @ src/main/java/com/acme/web/OrderEndpoint.java:L5-L10
java.class.spring-controller-no-repository-direct @ src/main/java/com/acme/web/ReportController.java:L6-L13
java.class.spring-boot-test-suffix @ src/test/java/com/acme/BootSmoke.java:L5-L9
java.method.package-junit-methods-start-with-test @ src/test/java/com/acme/billing/OrderTotalTest.java:L6-L8
java.class.spring-datajpa-test-suffix @ src/test/java/com/acme/repository/OrderStoreCheck.java:L5-L9
java.class.spring-boot-test-not-controller-slice @ src/test/java/com/acme/web/MixedWebMvcTest.java:L6-L11
java.class.spring-webmvc-test-suffix @ src/test/java/com/acme/web/OrderEndpointCheck.java:L5-L9
```
