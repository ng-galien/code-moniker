---
name: fowler-refactoring
lang: java
blurb: Fowler's code smells explained with small Java examples
published: true
---

# Fowler — Refactoring (code smells)

Ruleset inspired by Martin Fowler, *Refactoring*, chapter 3 "Bad Smells in
Code". Each rule introduces one smell that can be spotted from structure:
long methods, large classes, long parameter lists, data classes, lazy classes,
and feature envy.

```toml cm:rules
default_rules = false

[[java.method.where]]
id   = "fowler-long-method"
rationale = "A long method asks the reader to hold too much context at once. Extract smaller steps with names that explain the work."
expr = "lines <= 30"
message = "Long Method: `{name}` is too long; consider Extract Method."

[[java.class.where]]
id   = "fowler-large-class"
rationale = "A large class often means several responsibilities live together. Splitting it can make changes safer."
expr = "count(method) <= 20"
message = "Large Class: `{name}` has too many methods; consider Extract Class."

[[java.method.where]]
id   = "fowler-long-parameter-list-method"
rationale = "A long parameter list is hard to call correctly and hard to read. Group related values behind a clearer object."
expr = "count(param) <= 5"
message = "Long Parameter List: method `{name}` takes too many parameters; consider Introduce Parameter Object."

[[java.class.where]]
id   = "fowler-data-class"
rationale = "A data class with no behavior often means other objects are making its decisions for it."
expr = "count(field) >= 3 => count(method, name !~ ^(get|set|is)[A-Z_].* AND name !~ ^(equals|hashCode|toString)$) >= 1"
message = "Data Class: `{name}` has fields but no behaviour beyond accessors; consider Move Method."

[[java.class.where]]
id   = "fowler-lazy-class"
rationale = "A class with almost no state or behavior may add indirection without helping the reader."
expr = "count(method) >= 2 OR count(field) >= 2"
message = "Lazy Class: `{name}` is too small; consider Inline Class."

[[java.method.where]]
id   = "fowler-feature-envy"
rationale = "A method that mostly talks to another object may belong closer to that object."
expr = """
  count(out_refs, kind = 'calls' OR kind = 'method_call') < 3
  OR count(out_refs, (kind = 'calls' OR kind = 'method_call') AND source.parent != target.parent)
     <= count(out_refs, (kind = 'calls' OR kind = 'method_call') AND source.parent = target.parent)
"""
message = "Feature Envy: `{name}` calls other classes more than its own; consider Move Method."

[profiles.fowler-refactoring]
enable = [
  "^java\\.method\\.fowler-",
  "^java\\.class\\.fowler-",
]
```

## Long Method

`monthlySummary` runs well past the thirty-line threshold — straight-line
arithmetic that begs for Extract Method:

```java cm:file=src/main/java/com/shop/report/ReportBuilder.java
package com.shop.report;

public class ReportBuilder {
	public String monthlySummary() {
		int week1 = 120;
		int week2 = 95;
		int week3 = 143;
		int week4 = 88;
		int gross = week1 + week2 + week3 + week4;
		int returns1 = 4;
		int returns2 = 7;
		int returns3 = 2;
		int returns4 = 9;
		int returns = returns1 + returns2 + returns3 + returns4;
		int net = gross - returns;
		int taxBase = net * 100;
		int taxRate = 21;
		int tax = taxBase * taxRate / 100;
		int shippingFlat = 350;
		int shippingPerUnit = 12;
		int shipping = shippingFlat + shippingPerUnit * net;
		int discountThreshold = 400;
		int discount = net > discountThreshold ? net * 5 / 100 : 0;
		int total = taxBase + tax + shipping - discount;
		String header = "monthly summary";
		String lineGross = "gross: " + gross;
		String lineReturns = "returns: " + returns;
		String lineNet = "net: " + net;
		String lineTax = "tax: " + tax;
		String lineShipping = "shipping: " + shipping;
		String lineDiscount = "discount: " + discount;
		String lineTotal = "total: " + total;
		return header + lineGross + lineReturns + lineNet
			+ lineTax + lineShipping + lineDiscount + lineTotal;
	}

	public String title() {
		return "report";
	}
}
```

## Large Class

`ReservationDesk` hoards twenty-one tiny methods:

```java cm:file=src/main/java/com/shop/booking/ReservationDesk.java
package com.shop.booking;

public class ReservationDesk {
	public int hold() { return 1; }
	public int release() { return 2; }
	public int confirm() { return 3; }
	public int cancel() { return 4; }
	public int upgrade() { return 5; }
	public int downgrade() { return 6; }
	public int checkIn() { return 7; }
	public int checkOut() { return 8; }
	public int extend() { return 9; }
	public int shorten() { return 10; }
	public int transfer() { return 11; }
	public int split() { return 12; }
	public int merge() { return 13; }
	public int quote() { return 14; }
	public int invoice() { return 15; }
	public int refund() { return 16; }
	public int remind() { return 17; }
	public int archive() { return 18; }
	public int restore() { return 19; }
	public int audit() { return 20; }
	public int report() { return 21; }
}
```

## Long Parameter List

Six parameters on `planTrip` — an Introduce Parameter Object candidate:

```java cm:file=src/main/java/com/shop/travel/Scheduler.java
package com.shop.travel;

public class Scheduler {
	public String planTrip(String origin, String destination, int year,
			int month, int day, boolean flexible) {
		return origin + destination + year + month + day + flexible;
	}

	public String idle() {
		return "idle";
	}
}
```

## Data Class and Feature Envy

`Coordinates` carries three fields and nothing but getters; `TripSummary`
is the class that actually computes with them — its `describe` method makes
three calls to `Coordinates` and none to its own class:

```java cm:file=src/main/java/com/shop/travel/Coordinates.java
package com.shop.travel;

public class Coordinates {
	private double latitude;
	private double longitude;
	private double altitude;

	public double getLatitude() {
		return latitude;
	}

	public double getLongitude() {
		return longitude;
	}

	public double getAltitude() {
		return altitude;
	}
}
```

```java cm:file=src/main/java/com/shop/travel/TripSummary.java
package com.shop.travel;

public class TripSummary {
	public String describe(Coordinates point) {
		return "lat " + point.getLatitude()
			+ " lon " + point.getLongitude()
			+ " alt " + point.getAltitude();
	}

	public String label() {
		return "trip";
	}
}
```

## Lazy Class

`Stamp` no longer earns its keep — one field, one trivial method:

```java cm:file=src/main/java/com/shop/mail/Stamp.java
package com.shop.mail;

public class Stamp {
	private String code;

	public String code() {
		return code;
	}
}
```

```cm:expect
java.class.fowler-large-class @ src/main/java/com/shop/booking/ReservationDesk.java:L3-L25
java.class.fowler-lazy-class @ src/main/java/com/shop/mail/Stamp.java:L3-L9
java.method.fowler-long-method @ src/main/java/com/shop/report/ReportBuilder.java:L4-L35
java.class.fowler-data-class @ src/main/java/com/shop/travel/Coordinates.java:L3-L19
java.method.fowler-long-parameter-list-method @ src/main/java/com/shop/travel/Scheduler.java:L4-L7
java.method.fowler-feature-envy @ src/main/java/com/shop/travel/TripSummary.java:L4-L8
```
