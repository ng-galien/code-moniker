---
name: test-guardrails
lang: python
blurb: Production code stays isolated from test code and test names stay readable
published: true
---

# Test guardrails

Static guardrails around tests: production code must not depend on test code,
test doubles must not ship in production source sets, and test functions must
read like tests. The rules are language-agnostic moniker patterns; this
scenario demonstrates them with a small Python layout.

```toml cm:rules
# Test guardrail check sample.
#
# These rules enforce static guardrails around tests. They do not validate a
# temporal workflow; `code-moniker check` only analyzes the current graph.
#
# Use this sample to keep production code isolated from test code, keep test
# doubles out of production source sets, and keep test names easy to scan.

default_rules = false

[aliases]
# Def-scope alias. Use inside [[<lang>.<kind>.where]] rules.
test_src = "moniker ~ '**/*:/^tests?$/**'"

# Ref-scope aliases. Use inside [[refs.where]] rules.
src_test = "source ~ '**/*:/^tests?$/**'"
tgt_test = "target ~ '**/*:/^tests?$/**'"

# Production / test source separation ---------------------------------------

[[refs.where]]
id = "test-production-does-not-depend-on-tests"
# Production code must never import/call test code. This catches accidental
# coupling introduced while moving fast in an agent loop.
expr = "NOT $src_test => NOT $tgt_test"
message = "Production code must not depend on test code."

# Test doubles ---------------------------------------------------------------

[[default.class.where]]
id = "test-doubles-live-in-tests"
# Test doubles should not ship in production source sets. Tune the suffix list
# if your project has production types named *Mock or *Fixture.
expr = "name =~ (Stub|Fake|Mock|Spy|Dummy|TestDouble|Fixture|Mother)$ => $test_src"
message = "Test double `{name}` must live in test sources."

# Test naming ----------------------------------------------------------------

[[default.function.where]]
id = "test-functions-are-named-like-tests"
# For languages that expose test functions as `function`, keep test names
# readable. Rust has a dedicated `test` kind; see rust.toml for Rust-specific
# naming conventions.
expr = "$test_src => name =~ ^(test|should|it)(_|[A-Z0-9]).*"
message = "Test function `{name}` should read like a test."

[profiles.agent-test-guardrails]
# Use this profile from an agent harness. It keeps only fast, local rules that
# reduce common test hygiene regressions.
enable = [
  "^refs\\.test-production-does-not-depend-on-tests$",
  "^default\\.class\\.test-doubles-live-in-tests$",
  "^default\\.function\\.test-functions-are-named-like-tests$",
]
```

`PaymentStub` is a test double living in production sources, and it reaches
into the test tree for its gateway — both guardrails fire:

```python cm:file=src/payment.py
from tests.doubles import GatewayFake


class PaymentStub:
	def charge(self, amount):
		return GatewayFake().send(amount)
```

The test double itself is fine where it is — `GatewayFake` lives under
`tests/`, so the suffix rule stays quiet here:

```python cm:file=tests/doubles.py
class GatewayFake:
	def send(self, amount):
		return amount
```

One test function reads like a test, the other does not:

```python cm:file=tests/test_payment.py
def test_charge_succeeds():
	assert True


def charge_rejects_negative():
	assert True
```

```cm:expect
refs.test-production-does-not-depend-on-tests @ src/payment.py:L1
python.class.test-doubles-live-in-tests @ src/payment.py:L4-L6
refs.test-production-does-not-depend-on-tests @ src/payment.py:L6
python.function.test-functions-are-named-like-tests @ tests/test_payment.py:L5-L6
```
