---
name: go
lang: go
blurb: Focused interfaces, exported naming, size budgets, and layering for Go
published: true
---

# Go check sample

A starter rule set for a Go module: interfaces stay focused, exported types
keep PascalCase names, exported functions and all methods stay short, and
domain packages never import infrastructure packages directly.

```toml cm:rules
default_rules = false

[aliases]
internal = "moniker ~ '**/package:internal/**'"
domain = "moniker ~ '**/package:domain/**'"
infra = "moniker ~ '**/package:/^(infra|infrastructure)$/**'"

src_domain = "source ~ '**/package:domain/**'"
tgt_infra = "target ~ '**/package:/^(infra|infrastructure)$/**'"

[[go.interface.where]]
id = "interface-small"
rationale = "Small Go interfaces are easier to satisfy and easier to mock. A wide interface often means one concept is doing too much."
expr = "count(method) <= 5"
message = "Interface `{name}` has too many methods."

[[go.struct.where]]
id = "exported-struct-pascalcase"
rationale = "In Go, PascalCase is how a type becomes exported. This rule keeps public types visibly public and idiomatic."
expr = "visibility = 'public' => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Exported struct `{name}` must use PascalCase."

[[go.func.where]]
id = "exported-func-small"
rationale = "Exported functions are entry points for other packages. Keeping them short helps readers understand the package surface quickly."
expr = "visibility = 'public' => lines <= 80"
message = "Exported function `{name}` is too long."

[[go.method.where]]
id = "method-small"
rationale = "Short methods keep receiver behavior local and make package code easier to review."
expr = "lines <= 80"
message = "Method `{name}` is too long."

[[go.refs.where]]
id = "domain-no-infra"
rationale = "Domain packages should not know storage or transport details. Keep infrastructure behind package boundaries."
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on infrastructure."
```

The module manifest anchors the import paths:

```text cm:file=go.mod
module example.com/app

go 1.22
```

The infrastructure package is a small adapter — nothing to flag here:

```go cm:file=infra/store.go
package infra

type Store struct{}

func (s Store) Fetch() int {
	return 42
}
```

The domain package concentrates the violations: a six-method interface, an
exported struct with an underscore in its name, an exported function and a
method both padded past the 80-line budget, and a direct import of the
`infra` package (which `domain-no-infra` would like to flag — see the note
below):

```go cm:file=domain/order.go
package domain

import "example.com/app/infra"

// OrderRepo is too wide: six methods on one interface.
type OrderRepo interface {
	Load(id int) int
	Save(id int) error
	Delete(id int) error
	List() []int
	Count() int
	Reset() error
}

type Order_record struct {
	Total int
}

func TotalOf(o Order_record) int {
	s := infra.Store{}
	return o.Total + s.Fetch()
}

func SettleEverything() int {
	total := 0
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	return total
}

func (o Order_record) Reconcile() int {
	total := o.Total
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	total += 1
	return total
}
```

Note on `interface-small`: the Go extractor records interface method
elements only as `uses_type` references — it never emits `method` defs under
an interface — so `count(method)` evaluates to 0 for every interface and the
rule can never fire. The six-method `OrderRepo` above documents the intent.

Note on `domain-no-infra`: extracted Go import targets are spelled with
`external_pkg:`/`path:` segments (`package:` segments only appear on the
source side, mirroring the importing file's directory), so the `tgt_infra`
alias — `target ~ '**/package:/^(infra|infrastructure)$/**'` — can never
match a Go ref target and the rule stays silent in any layout.

```cm:expect
! go.interface.interface-small the Go extractor emits no method defs under interfaces (method_elem only yields uses_type refs), so count(method) is always 0
! go.refs.domain-no-infra extracted Go ref targets use external_pkg:/path: segments, never package:, so the tgt_infra package pattern cannot match
go.struct.exported-struct-pascalcase @ domain/order.go:L15-L17
go.func.exported-func-small @ domain/order.go:L24-L107
go.method.method-small @ domain/order.go:L109-L192
```
