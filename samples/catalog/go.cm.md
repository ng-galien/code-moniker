---
name: go
title: Go starter pack
lang: go
blurb: Focused interfaces, exported naming, and size budgets for Go
learn_kind: language
learn_path: languages/go
learn_order: 60
tags: go,naming,interfaces
published: true
---

# Go check sample

A starter rule set for a Go module: interfaces stay focused, exported types
keep PascalCase names, and exported functions and all methods stay short.

```toml cm:rules
default_rules = false

[aliases]
internal = "moniker ~ '**/package:internal/**'"

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

The domain package concentrates the demonstrated violations: a six-method
interface, an exported struct with an underscore in its name, and an exported
function and method both padded past the 80-line budget. Its import remains
fixture context, not a claimed dependency-boundary check.

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

```cm:expect
go.interface.interface-small @ domain/order.go:L6-L13
go.struct.exported-struct-pascalcase @ domain/order.go:L15-L17
go.func.exported-func-small @ domain/order.go:L24-L107
go.method.method-small @ domain/order.go:L109-L192
```
