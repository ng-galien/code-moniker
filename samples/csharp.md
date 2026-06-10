---
name: csharp
lang: cs
blurb: C# naming conventions, class budgets, and Domain/Infrastructure layering
published: true
---

# C# starter pack

The C# sample enforces the usual .NET conventions — PascalCase classes and
public methods, `I`-prefixed interfaces — plus a size budget on classes and a
namespace-level layering rule: `Domain` code must never reference
`Infrastructure` code directly.

```toml cm:rules
# C# check sample.
# Copy to `.code-moniker.toml` and adapt namespace names.

default_rules = false

[aliases]
domain = "moniker ~ '**/package:Domain/**'"
infra = "moniker ~ '**/package:Infrastructure/**'"
tests = "moniker ~ '**/dir:/^[Tt]ests$/**'"

src_domain = "source ~ '**/package:Domain/**'"
tgt_infra = "target ~ '**/package:Infrastructure/**'"

[[cs.class.where]]
id = "class-pascalcase"
# C# classes should use PascalCase.
expr = "name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Class `{name}` must use PascalCase."

[[cs.interface.where]]
id = "interface-starts-with-i"
# C# interfaces often use the IName convention.
expr = "name =~ ^I[A-Z][A-Za-z0-9]*$"
message = "Interface `{name}` must start with I."

[[cs.method.where]]
id = "public-method-pascalcase"
# Public methods should use PascalCase.
expr = "visibility = 'public' => name =~ ^[A-Z][A-Za-z0-9]*$"
message = "Public method `{name}` must use PascalCase."

[[cs.class.where]]
id = "class-budget"
# Limit class size through direct child methods and properties.
expr = "count(method) <= 25 AND count(property) <= 20"
message = "Class `{name}` is too large."

[[cs.refs.where]]
id = "domain-no-infra"
# Direct refs from Domain namespaces to Infrastructure namespaces are forbidden.
expr = "$src_domain => NOT $tgt_infra"
message = "Domain code must not depend directly on Infrastructure."
```

The domain file collects the naming offenders: an interface without the `I`
prefix, a snake_case class, and a lowercase public method. It also
instantiates a persistence table straight from the `Infrastructure`
namespace — see the note on `domain-no-infra` below:

```cs cm:file=src/Domain/Order.cs
using Acme.Infrastructure;

namespace Acme.Domain
{
	public interface OrderRepository
	{
		Order Find(string id);
	}

	public class Order
	{
		public string Id { get; set; }
	}

	public class order_service
	{
		public string run()
		{
			var table = new OrderTable();
			return table.Key();
		}
	}
}
```

The infrastructure adapter itself is clean:

```cs cm:file=src/Infrastructure/OrderTable.cs
namespace Acme.Infrastructure
{
	public class OrderTable
	{
		public string Key()
		{
			return "order";
		}
	}
}
```

`CustomerProfile` is a data bag that grew past the 20-property budget:

```cs cm:file=src/Domain/CustomerProfile.cs
namespace Acme.Domain
{
	public class CustomerProfile
	{
		public string FirstName { get; set; }
		public string LastName { get; set; }
		public string Email { get; set; }
		public string Phone { get; set; }
		public string Street { get; set; }
		public string City { get; set; }
		public string ZipCode { get; set; }
		public string Country { get; set; }
		public string Company { get; set; }
		public string VatNumber { get; set; }
		public string Language { get; set; }
		public string Currency { get; set; }
		public string TimeZone { get; set; }
		public string Segment { get; set; }
		public string Channel { get; set; }
		public string Referrer { get; set; }
		public string LoyaltyTier { get; set; }
		public string Newsletter { get; set; }
		public string SupportPlan { get; set; }
		public string AccountManager { get; set; }
		public string Notes { get; set; }
	}
}
```

A note on `domain-no-infra`: the C# extractor records cross-namespace
references either as `external_pkg` targets (for `using` directives) or as
name-match guesses inside the source's own namespace, so no reference ever
carries a `package:Infrastructure` target and the rule cannot fire today. It
is kept in the sample as the intended shape of the layering contract.

```cm:expect
! cs.refs.domain-no-infra C# cross-namespace refs resolve to external_pkg or in-module name-match targets, never package:Infrastructure
cs.class.class-budget @ src/Domain/CustomerProfile.cs:L3-L26
cs.interface.interface-starts-with-i @ src/Domain/Order.cs:L5-L8
cs.class.class-pascalcase @ src/Domain/Order.cs:L15-L22
cs.method.public-method-pascalcase @ src/Domain/Order.cs:L17-L21
```
