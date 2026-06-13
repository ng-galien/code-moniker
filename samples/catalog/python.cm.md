---
name: python
lang: python
blurb: Naming, private-name hygiene, class budgets, and layering for Python
published: true
---

# Python check sample

A starter rule set for a Python package: classes stay PascalCase, module-level
functions stay snake_case, classes stay within a method budget, and domain
packages never import infrastructure packages directly.

```toml cm:rules
default_rules = false

[aliases]
src = "moniker ~ '**/*:src/**'"
tests = "moniker ~ '**/dir:/^tests?$/**'"
domain = "moniker ~ '**/package:domain/**'"
infra = "moniker ~ '**/package:infrastructure/**'"

src_domain = "source ~ '**/package:domain/**'"
tgt_infra = "target ~ '**/package:infrastructure/**'"

[[python.class.where]]
id = "class-pascalcase"
rationale = "PascalCase helps classes stand out from functions and modules, which makes Python code easier to scan."
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Class `{name}` must use PascalCase."

[[python.function.where]]
id = "function-snakecase"
rationale = "Snake case is the usual shape for Python functions. A consistent shape lets readers recognize callables quickly."
expr = "name =~ ^[a-z_][a-z0-9_]*$"
message = "Function `{name}` must use snake_case."

[[python.method.where]]
id = "private-methods-underscore"
rationale = "A leading underscore tells readers that a method is an implementation detail, not part of the class contract."
expr = "visibility = 'private' => name =~ ^_"
message = "Private method `{name}` must start with underscore."

[[python.class.where]]
id = "class-budget"
rationale = "A class with too many methods is hard to learn in one pass. Use this budget as a prompt to split responsibilities."
expr = "count(method) <= 20 AND all(method, lines <= 60)"
message = "Class `{name}` is too large."

[[python.refs.where]]
id = "domain-no-infra"
rationale = "Domain code should describe the business problem. Database and framework details belong in outer packages."
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."
```

The infrastructure package is a small adapter — nothing to flag here:

```python cm:file=infrastructure/db.py
class Database:
    def fetch(self):
        return 42
```

The domain package concentrates the violations: a lowercase class, a
PascalCase module-level function, a class with 21 methods (one past the
budget), and direct imports and calls into the `infrastructure` package:

```python cm:file=domain/order.py
from infrastructure.db import Database


class order_record:
    total = 0


class Order:
    def __init__(self):
        self.db = Database()

    def total(self):
        return self.db.fetch()


def LoadOrder():
    return Database().fetch()


class Ledger:
    def entry_01(self):
        return 1

    def entry_02(self):
        return 2

    def entry_03(self):
        return 3

    def entry_04(self):
        return 4

    def entry_05(self):
        return 5

    def entry_06(self):
        return 6

    def entry_07(self):
        return 7

    def entry_08(self):
        return 8

    def entry_09(self):
        return 9

    def entry_10(self):
        return 10

    def entry_11(self):
        return 11

    def entry_12(self):
        return 12

    def entry_13(self):
        return 13

    def entry_14(self):
        return 14

    def entry_15(self):
        return 15

    def entry_16(self):
        return 16

    def entry_17(self):
        return 17

    def entry_18(self):
        return 18

    def entry_19(self):
        return 19

    def entry_20(self):
        return 20

    def entry_21(self):
        return 21
```

Note on `private-methods-underscore`: the Python extractor derives a method's
visibility from its name — `private` exactly when the name starts with a
non-dunder `__` — so every private method necessarily matches `^_` and the
rule can never fire.

```cm:expect
! python.method.private-methods-underscore Python visibility is derived from the leading-underscore name itself, so a private method always starts with _ and the implication is a tautology
python.refs.domain-no-infra @ domain/order.py:L1
python.class.class-pascalcase @ domain/order.py:L4-L5
python.refs.domain-no-infra @ domain/order.py:L10
python.function.function-snakecase @ domain/order.py:L16-L17
python.refs.domain-no-infra @ domain/order.py:L17
python.class.class-budget @ domain/order.py:L20-L82
```
