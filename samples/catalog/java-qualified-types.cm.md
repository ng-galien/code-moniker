---
name: java-qualified-types
lang: java
blurb: Java fully qualified type names are kept only when the simple name is ambiguous
published: true
---

# Java qualified type names

Fully qualified Java type names are useful when the simple name would be
ambiguous. When no competing type with the same simple name is visible, the
default rule prefers an import and the simple type name.

```toml cm:rules
default_rules = false

[[java.refs.where]]
id       = "no-unnecessary-qualified-type-name"
severity = "warn"
expr     = "kind != 'uses_type' OR text = target.name OR any(source.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target) OR any(source.ancestors.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target)"
message  = "Qualified Java type reference can use simple name `{target.name}` here; keep fully qualified names for real ambiguity."
```

`ClockReader` uses a fully qualified `LocalDate` even though `LocalDate` is not
ambiguous:

```java cm:file=src/main/java/com/acme/time/ClockReader.java
package com.acme.time;

public class ClockReader {
	private java.time.LocalDate businessDate;
}
```

`AuditClock` stays clean because `Instant` would otherwise name
`com.acme.other.Instant`:

```java cm:file=src/main/java/com/acme/time/AuditClock.java
package com.acme.time;

import com.acme.other.Instant;

public class AuditClock {
	private java.time.Instant capturedAt;
	private Instant localInstant;
}
```

An unrelated import does not make a different fully qualified type ambiguous:

```java cm:file=src/main/java/com/acme/time/ClockWithUnrelatedImport.java
package com.acme.time;

import com.acme.other.Foo;

public class ClockWithUnrelatedImport {
	private java.time.LocalDate businessDate;
}
```

```java cm:file=src/main/java/com/acme/other/Instant.java
package com.acme.other;

public class Instant {
}
```

```java cm:file=src/main/java/com/acme/other/Foo.java
package com.acme.other;

public class Foo {
}
```

```cm:expect
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/ClockReader.java:L4
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/ClockWithUnrelatedImport.java:L6
```
