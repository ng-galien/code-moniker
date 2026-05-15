# Extract Name Regex Filter

## Goal

Add a native `extract` filter for name-based exploration so users do not
have to pipe output through `grep`, especially when using `--format tree`.

## Delivered Behavior

- `code-moniker extract <PATH> --name <REGEX>` filters defs by the last
  moniker segment name.
- References are filtered by target name, matching the existing
  `--where` target semantics for refs.
- `--name` is repeatable; alternatives are ORed within the flag and
  ANDed with `--kind`, `--shape`, and `--where`.
- Callable signatures are matched on their bare name, so method/function
  filters do not need to include parameter slots.

## Example

```bash
code-moniker extract . --kind interface --name 'Resolver$' --format tree
```

## Validation

- TDD coverage in `crates/cli/tests/cli_e2e.rs` for Java interface
  filtering with tree output.
- Unit coverage in `crates/cli/src/predicate.rs` for the shared filter.
