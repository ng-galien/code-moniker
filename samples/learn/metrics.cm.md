---
name: metrics
title: Named local metrics
summary: Use named local metrics — wmc, rfc, lcom4, cbo, dit, noc, fan_in, fan_out — bound to self or each.
---

# Named Local Metrics

Named metrics are numeric expressions with an explicit binding. `self` is the
rule's owner def; `each` is the item currently visited by an aggregate. They
are computed from the **local file graph** only — direct child defs and the
refs extracted for the file under check. They do not use project-wide linkage.

| Metric | Local meaning |
| ------ | ------------- |
| `wmc(X)` | direct callable children, weight 1 each |
| `rfc(X)` | direct callables plus distinct targets they call |
| `lcom4(X)` | connected components among callables (calls or shared field use) |
| `cbo(X)` | distinct external type/namespace buckets coupled through refs |
| `dit(X)` | longest local inheritance chain |
| `noc(X)` | local children that inherit from `X` |
| `fan_in(X)` | refs whose target is `X` |
| `fan_out(X)` | refs whose source is `X` |

```toml cm:rules
default_rules = false

[[java.class.where]]
id        = "method-weight-budget"
rationale = "Weighted Methods per Class counts callable children. A small budget keeps the class focused."
expr      = "wmc(self) <= 2"

[[java.class.where]]
id        = "response-set-budget"
rationale = "Response For a Class is callables plus the distinct methods they call. A wide response set is hard to test."
expr      = "rfc(self) <= 3"

[[java.class.where]]
id        = "cohesion-budget"
rationale = "LCOM4 is the number of connected components among methods. More than one component suggests the class should be split."
expr      = "lcom4(self) <= 1"

[[java.class.where]]
id        = "coupling-budget"
rationale = "Coupling Between Objects counts the distinct external types a class touches. This teaching rule flags any outward coupling."
expr      = "cbo(self) <= 0"

[[java.class.where]]
id        = "no-local-subclasses"
rationale = "Number Of Children counts local subclasses. This teaching rule flags any base class that is extended in the same file."
expr      = "noc(self) <= 0"

[[java.class.where]]
id        = "no-inheritance-depth"
rationale = "Depth of Inheritance Tree measures the longest local inheritance chain. This teaching rule flags any class that extends another in the same file."
expr      = "dit(self) <= 0"

[[java.method.where]]
id        = "leaf-method-fan-out"
rationale = "fan_out(self) counts a method's outgoing refs. This teaching rule flags methods that call into other symbols."
expr      = "fan_out(self) <= 0"

[[java.method.where]]
id        = "private-method-fan-in"
rationale = "fan_in(self) counts incoming refs. This teaching rule flags methods that are called by others."
expr      = "fan_in(self) <= 0"

[[java.class.where]]
id        = "balanced-method-fan-out"
rationale = "Inside an aggregate, each binds to the visited method while self stays the class. cv is the coefficient of variation of per-method fan-out."
expr      = "count(method) >= 3 => cv(method, fan_out(each)) <= 0.1"
```

`Ledger` keeps two field clusters: deposit/withdraw/record share `balance` and
`audits`, while summary/label form a separate group — two components, so LCOM4
is above one. `AuditLedger` extends `Ledger` in the same file, which gives the
base class a local child and the subclass an inheritance depth.

```java cm:file=src/main/java/bank/Ledger.java
package bank;

import java.util.ArrayList;
import java.util.List;

public class Ledger {
	private int balance;
	private int audits;
	private List<String> entries = new ArrayList<>();

	public int deposit(int n) {
		balance = balance + n;
		entries.add("deposit");
		return record();
	}

	public int withdraw(int n) {
		balance = balance - n;
		return record();
	}

	private int record() {
		audits = audits + 1;
		return audits;
	}

	public int summary() {
		return label().length();
	}

	public String label() {
		return "ledger";
	}
}

class AuditLedger extends Ledger {
	public int extra() {
		return 0;
	}
}
```

```cm:expect
java.class.balanced-method-fan-out @ src/main/java/bank/Ledger.java:L6-L34
java.class.cohesion-budget @ src/main/java/bank/Ledger.java:L6-L34
java.class.coupling-budget @ src/main/java/bank/Ledger.java:L6-L34
java.class.method-weight-budget @ src/main/java/bank/Ledger.java:L6-L34
java.class.no-local-subclasses @ src/main/java/bank/Ledger.java:L6-L34
java.class.response-set-budget @ src/main/java/bank/Ledger.java:L6-L34
java.method.leaf-method-fan-out @ src/main/java/bank/Ledger.java:L11-L15
java.method.leaf-method-fan-out @ src/main/java/bank/Ledger.java:L17-L20
java.method.private-method-fan-in @ src/main/java/bank/Ledger.java:L22-L25
java.method.leaf-method-fan-out @ src/main/java/bank/Ledger.java:L27-L29
java.method.leaf-method-fan-out @ src/main/java/bank/Ledger.java:L31-L33
java.method.private-method-fan-in @ src/main/java/bank/Ledger.java:L31-L33
java.class.coupling-budget @ src/main/java/bank/Ledger.java:L36-L40
java.class.no-inheritance-depth @ src/main/java/bank/Ledger.java:L36-L40
```
