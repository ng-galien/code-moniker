---
name: test-guardrails
lang: python
blurb: Production code stays separate from test helpers, and tests read clearly
published: true
---

# Test guardrails

Static guardrails around tests: production code must not depend on test code,
test doubles must not ship in production sources, and test functions should
read like behavior claims. The scenario demonstrates those habits with a small
Python layout.

```toml cm:rules
default_rules = false

[aliases]
test_src = "moniker ~ '**/*:/^tests?$/**'"

src_test = "source ~ '**/*:/^tests?$/**'"
tgt_test = "target ~ '**/*:/^tests?$/**'"

[[refs.where]]
id = "test-production-does-not-depend-on-tests"
rationale = "Production code should not rely on helpers that only exist for tests. This keeps shipped code independent from the test tree."
expr = "NOT $src_test => NOT $tgt_test"
message = "Production code must not depend on test code."

[[default.class.where]]
id = "test-doubles-live-in-tests"
rationale = "Names like Fake, Mock, and Fixture usually describe test doubles. Keeping them under tests prevents accidental production use."
expr = "name =~ (Stub|Fake|Mock|Spy|Dummy|TestDouble|Fixture|Mother)$ => $test_src"
message = "Test double `{name}` must live in test sources."

[[default.function.where]]
id = "test-functions-are-named-like-tests"
rationale = "A test function should read like a behavior claim, so failures are understandable in a report."
expr = "$test_src => name =~ ^(test|should|it)(_|[A-Z0-9]).*"
message = "Test function `{name}` should read like a test."

[profiles.agent-test-guardrails]
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
