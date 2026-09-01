---
name: typescript-testing
title: TypeScript test conventions
lang: ts
blurb: Keep test-only naming policies explicit without pretending to understand Jest or Vitest semantics
learn_kind: framework
learn_path: languages/typescript/testing
learn_order: 30
tags: typescript,tests,jest,vitest,naming
learn_aliases: jest,vitest
published: true
---

# TypeScript test conventions

Code Moniker extracts declarations and references from Jest, Vitest, and other
TypeScript test files, but it does not execute the runner or infer a test from
framework semantics. This example is therefore an explicit project convention:
exported helpers below a `tests/` directory start with `create` or `build`.
The policy is declared for both `.ts` and `.tsx` helpers.

```toml cm:rules
default_rules = false

[aliases]
test_src = "moniker ~ '**/dir:/^(test|tests|__tests__)$/**'"

[[ts.function.where]]
id = "test-helper-prefix"
expr = "$test_src AND visibility = 'public' => name =~ ^(create|build)[A-Z]"
message = "Exported test helper `{name}` should start with create or build."

[[tsx.function.where]]
id = "test-helper-prefix"
expr = "$test_src AND visibility = 'public' => name =~ ^(create|build)[A-Z]"
message = "Exported test helper `{name}` should start with create or build."
```

```ts cm:file=tests/order_fixture.ts
export function orderFixture() {
	return { id: "order-1" };
}
```

```tsx cm:file=tests/order_view_fixture.test.tsx
export function orderViewFixture() {
	return <section />;
}
```

```cm:expect
ts.function.test-helper-prefix @ tests/order_fixture.ts:L1-L3
tsx.function.test-helper-prefix @ tests/order_view_fixture.test.tsx:L1-L3
```
