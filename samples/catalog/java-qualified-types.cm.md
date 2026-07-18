---
name: java-qualified-types
lang: java
blurb: Java fully qualified type names are kept only when the simple name is ambiguous
published: true
---

# Java qualified type names

Fully qualified Java type names are useful when the simple name would be
ambiguous. When no competing type with the same simple name is visible, the
default rule prefers an import and the simple type name. Only package-qualified
names count: qualifying a nested type by its outer type (`Map.Entry`,
`Outer.Inner`) is idiomatic and stays clean.

```toml cm:rules
default_rules = false

[[java.refs.where]]
id       = "no-unnecessary-qualified-type-name"
severity = "warn"
expr     = "kind != 'uses_type' OR text !~ '^[A-Za-z_$][A-Za-z0-9_$]*\\.[a-z_$][A-Za-z0-9_$]*\\.' OR any(source.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target) OR any(source.ancestors.out_refs, kind = 'imports_symbol' AND target.name = current.target.name AND target != current.target)"
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

`ImportedGenerics` stays clean: a generic usage of an imported type is a simple
name, the type arguments do not qualify it:

```java cm:file=src/main/java/com/acme/time/ImportedGenerics.java
package com.acme.time;

import java.util.List;

public record ImportedGenerics(
		String label,
		List<String> entries,
		List<List<String>> batches
) {}
```

An unrelated import does not make a different fully qualified type ambiguous:

```java cm:file=src/main/java/com/acme/time/ClockWithUnrelatedImport.java
package com.acme.time;

import com.acme.other.Foo;

public class ClockWithUnrelatedImport {
	private java.time.LocalDate businessDate;
}
```

Qualifying a nested record, enum, or static class by its outer type is the
normal way to name it, including in static member expressions:

```java cm:file=src/main/java/com/acme/time/NestedOwner.java
package com.acme.time;

public class NestedOwner {
	public record Inner(String label) {}
	public enum Status { OPEN, CLOSED }
	public static class Helper {}
}
```

```java cm:file=src/main/java/com/acme/time/NestedUser.java
package com.acme.time;

public class NestedUser {
	private NestedOwner.Inner inner;
	private NestedOwner.Status status;
	private NestedOwner.Helper helper;
	private NestedOwner.Status fallback = NestedOwner.Status.OPEN;
}
```

`Map.Entry` is the idiomatic name of the JDK nested interface, importing `Map`
is enough. The same remains true when an outer type does not follow the usual
uppercase naming convention:

```java cm:file=src/main/java/com/acme/time/MapEntryUser.java
package com.acme.time;

import java.util.Map;

public class MapEntryUser {
	private Map.Entry<String, Integer> entry;
	private Map<String, Integer> plain;
}
```

```java cm:file=src/main/java/com/acme/time/lowerOwner.java
package com.acme.time;

public class lowerOwner {
	public static class Inner {}
}
```

```java cm:file=src/main/java/com/acme/time/LowercaseNestedUser.java
package com.acme.time;

public class LowercaseNestedUser {
	private lowerOwner.Inner nested;
}
```

A package-qualified generic base type still fires:

```java cm:file=src/main/java/com/acme/time/QualifiedGeneric.java
package com.acme.time;

public class QualifiedGeneric {
	private java.util.List<String> stillQualified;
}
```

So does a package-qualified type buried in a generic argument:

```java cm:file=src/main/java/com/acme/time/QualifiedArgument.java
package com.acme.time;

import java.util.List;

public class QualifiedArgument {
	private List<java.time.LocalDate> dates;
}
```

So does a package-qualified enum:

```java cm:file=src/main/java/com/acme/time/QualifiedEnum.java
package com.acme.time;

public class QualifiedEnum {
	private com.acme.other.Color banner;
}
```

Package qualification is syntax, not a casing convention. A legal uppercase
package segment must still fire:

```java cm:file=src/main/java/Com/vendor/UpperType.java
package Com.vendor;

public class UpperType {}
```

```java cm:file=src/main/java/com/acme/time/UpperPackageReference.java
package com.acme.time;

public class UpperPackageReference {
	private Com.vendor.UpperType value;
}
```

Wildcards and arrays follow the same contract: only the package-qualified
array element fires.

```java cm:file=src/main/java/com/acme/time/WildcardsAndArrays.java
package com.acme.time;

import java.util.List;
import java.util.Map;

public class WildcardsAndArrays {
	private List<? extends Number> numbers;
	private Map<String, ? super Integer> sinks;
	private List<String>[] buckets;
	private String[] names;
	private java.time.LocalDate[] dates;
}
```

Static members and generic heritage stay clean with plain imports:

```java cm:file=src/main/java/com/acme/time/StaticMembers.java
package com.acme.time;

import java.util.List;

public class StaticMembers {
	public static final List<String> DEFAULTS = List.of("a");

	public static List<String> defaults() {
		return DEFAULTS;
	}
}
```

```java cm:file=src/main/java/com/acme/time/GenericHeritage.java
package com.acme.time;

import java.util.ArrayList;

public class GenericHeritage extends ArrayList<String> implements Comparable<GenericHeritage> {
	public int compareTo(GenericHeritage other) {
		return 0;
	}
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

```java cm:file=src/main/java/com/acme/other/Color.java
package com.acme.other;

public enum Color {
	RED,
	GREEN
}
```

```cm:expect
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/ClockReader.java:L4
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/ClockWithUnrelatedImport.java:L6
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/QualifiedArgument.java:L6
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/QualifiedEnum.java:L4
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/QualifiedGeneric.java:L4
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/UpperPackageReference.java:L4
java.refs.no-unnecessary-qualified-type-name @ src/main/java/com/acme/time/WildcardsAndArrays.java:L11
```
