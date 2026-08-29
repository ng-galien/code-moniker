# Develop

Use this posture to focus the project map, choose the existing owner that fits
the requested behavior, implement the change, and verify it. Run Onboard first
when the project context is not already established.

## Workflow

1. Narrow the rule corpus by the task's component, pattern, or exact rule, then
   request details only for that focus:

   ```sh
   code-moniker rules show . --component <name> --details
   code-moniker rules show . --pattern <name> --details
   code-moniker rules show . --rule <id>
   ```

2. Before interpreting a rule construct, run the corresponding command from
   the main skill's learn table. Rule authoring starts with
   `code-moniker rules learn basics` and adds only the topics used by the rule.
3. Select one symbolic surface. With MCP, use narrow symbol searches, then pass
   returned monikers to usages or graph tools. Use a project view only when it
   changes interpretation. In CLI-only mode, use bounded `stats`, `extract`,
   `diff`, or `check` commands instead of imitating an MCP sequence call by
   call.
4. When several mechanisms could own the behavior, keep a short candidate set
   and gather one bounded relationship result for every alternative that could
   change the decision. State why the selected owner fits and the others do
   not.
5. Before a structural edit, call `code_moniker_context` once when ownership,
   consumers, applicable rules, or change impact remain uncertain. Do not
   re-fetch sections it already supplied unless coverage is insufficient.
6. Inspect the actual source and current diff, implement through the existing
   owner and data flow, then run focused tests and the broader configured gate
   warranted by the change.
7. Continue with Guard after implementation and validation.

## Boundaries

- Keep `compact:true`, small budgets, and tight limits by default.
- Read source or AST only when indexed relationships cannot establish the
  required behavior.
- Treat missing, ambiguous, unresolved, and partial results as coverage facts,
  not as proof that a relationship is absent.
- Do not create a parallel mechanism merely because its local implementation
  appears easier than the existing project owner.
