# Guard

Use this posture after a validated change to preserve or deliberately evolve
the project's executable architectural memory. Guarding is not hook execution:
it is the agent's responsibility to decide whether the change keeps, revises,
adds, or removes architectural testimony.

## Workflow

1. Inspect the implemented code, tests, and actual diff. Identify the exact
   durable guarantee established, changed, or removed.
2. Re-read the affected corpus with a focused `rules show` call. Before judging
   or changing any construct, run its command from the main skill's learn
   table. Use `rules learn basics` for rule authoring, `rules learn taxonomy`
   for vocabulary or ids, and every other topic actually used by the rule.
3. Choose one outcome:
   - keep the testimony when the code still honors the same invariant;
   - revise it when the executable boundary, vocabulary, message, or rationale
     changed;
   - add it when the change established a durable, non-obvious invariant whose
     loss would recreate architectural drift;
   - remove it when the protected decision no longer exists.
4. Treat a taxonomy change as an explicit project-language decision. Do not add
   patterns, components, or aliases merely to silence diagnostics, and do not
   silently resolve a semantic choice that belongs to the user or project.
5. Keep the executable rule no broader than the guarantee named by its id. Use
   the message for the immediate failure and the rationale for the design
   decision and regression risk.
6. Run the narrow executable check whose scope can exercise the rule, then the
   configured broader gate when warranted. File-scoped checks do not prove
   workspace or linkage rules.

## Boundaries

- Do not add a rule merely because code changed, restate implementation, or
  duplicate behavior already protected more clearly by a focused test.
- Do not weaken, suppress, or bypass an existing rule to make a change pass
  unless the task authorizes that architectural decision.
- Use Git history only when the rationale is insufficient, stale, or conflicts
  with the current design. Commit co-occurrence is context, not causality.
- Keep static corpus diagnostics, indexed coverage, historical context, and
  architectural judgment separate.
