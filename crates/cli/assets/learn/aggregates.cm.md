---
name: aggregates
title: Numeric aggregates, entropy, and mode
summary: Aggregate a numeric expression over a domain with sum, max, min, avg, median, percentile, stddev, var, cv, gini — plus entropy and mode over any value.
---

# Numeric Aggregates, Entropy, And Mode

Aggregators evaluate a numeric expression once per item of a domain and reduce
the values to a single number. Here every rule aggregates `lines` over the
`method` domain, so each rule speaks about the *distribution* of method sizes
in a class rather than any single method.

| Function | Reduces to |
| -------- | ---------- |
| `sum(D, E)` | total |
| `max(D, E)` / `min(D, E)` | extreme value |
| `avg(D, E)` | arithmetic mean |
| `median(D, E)` | 50th percentile |
| `percentile(D, E, P)` | percentile `P` (0–100) |
| `stddev(D, E)` / `var(D, E)` | spread around the mean |
| `cv(D, E)` | coefficient of variation, `stddev / abs(mean)` |
| `gini(D, E)` | inequality of the distribution |

`entropy(D, E)` and `mode(D, E)` work over **any** value, not only numbers:
entropy is the normalized diversity of the values, and mode is the most
frequent one.

```toml cm:rules
default_rules = false

[[java.class.where]]
id        = "method-lines-sum"
rationale = "sum totals a numeric expression across the domain."
expr      = "sum(method, lines) <= 1"

[[java.class.where]]
id        = "method-lines-max"
rationale = "max returns the largest value; here, the longest method body."
expr      = "max(method, lines) <= 1"

[[java.class.where]]
id        = "method-lines-min"
rationale = "min returns the smallest value; this rule expects every method to span at least four lines."
expr      = "min(method, lines) >= 4"

[[java.class.where]]
id        = "method-lines-avg"
rationale = "avg is the arithmetic mean of the values."
expr      = "avg(method, lines) <= 1"

[[java.class.where]]
id        = "method-lines-median"
rationale = "median is the 50th percentile, robust to a single outlier."
expr      = "median(method, lines) <= 1"

[[java.class.where]]
id        = "method-lines-p90"
rationale = "percentile takes the cut point as a third argument from 0 to 100."
expr      = "percentile(method, lines, 90) <= 1"

[[java.class.where]]
id        = "method-lines-stddev"
rationale = "stddev measures spread around the mean; a flat distribution stays near zero."
expr      = "stddev(method, lines) <= 0.1"

[[java.class.where]]
id        = "method-lines-var"
rationale = "var is the population variance, the square of stddev."
expr      = "var(method, lines) <= 0.1"

[[java.class.where]]
id        = "method-lines-cv"
rationale = "cv normalizes spread by the mean, so it compares classes of different sizes."
expr      = "cv(method, lines) <= 0.1"

[[java.class.where]]
id        = "method-lines-gini"
rationale = "gini is 0 when every method is the same size and approaches 1 as a few methods dominate."
expr      = "gini(method, lines) <= 0.1"

[[java.class.where]]
id        = "method-visibility-entropy"
rationale = "entropy works on non-numeric values: a class with mixed visibilities has positive diversity."
expr      = "entropy(method, visibility) <= 0.1"

[[java.class.where]]
id        = "methods-mostly-public"
rationale = "mode returns the most frequent value and can be compared with = or !=."
expr      = "mode(method, visibility) = 'public'"
```

`Report` mixes one short public method with three longer private ones, so the
size distribution is uneven (nonzero stddev, var, cv, gini) and the dominant
visibility is private.

```java cm:file=src/main/java/stats/Report.java
package stats;

public class Report {
	public int a() {
		return 1;
	}

	private int b() {
		int x = 1;
		int y = 2;
		return x + y;
	}

	private int c() {
		int s = 0;
		for (int i = 0; i < 10; i++) {
			s = s + i;
		}
		return s;
	}

	private int d() {
		return 0;
	}
}
```

```cm:expect
java.class.method-lines-avg @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-cv @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-gini @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-max @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-median @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-min @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-p90 @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-stddev @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-sum @ src/main/java/stats/Report.java:L3-L25
java.class.method-lines-var @ src/main/java/stats/Report.java:L3-L25
java.class.method-visibility-entropy @ src/main/java/stats/Report.java:L3-L25
java.class.methods-mostly-public @ src/main/java/stats/Report.java:L3-L25
```
