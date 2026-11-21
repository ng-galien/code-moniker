# Review

Use this posture for architecture audits, relationship questions, refactor
reasoning, and change-impact review. It gathers bounded facts and turns them
into an evidence-backed interpretation. It is read-only unless the user also
asks for implementation.

Run Onboard first when the project vocabulary and workspace identity are not
already established.

## Workflow

1. Scope the review from the user's subsystem, pain point, or change. Use a
   relevant project view before generic graph topology when declared project
   intent can change the interpretation.
2. Before interpreting rules, taxonomy, fragments, profiles, metrics, or other
   DSL constructs, run the corresponding command from the main skill's learn
   table.
3. Choose the MCP tool that answers the question.
   - use `code_moniker_diff` for symbol-level worktree changes;
   - use `code_moniker_graph` for one selected unit's neighborhood;
   - use `code_moniker_usages` when consumer or producer distribution matters;
   - use `code_moniker_context` for change impact, applicable rules, notes, and
     suggested checks;
   - use `code_moniker_rules` only for the relevant compiled testimony or
     indexed evaluation.

   With the CLI, start from the project root.

   ```sh
   code-moniker --version
   code-moniker rules show .
   ```

   When checking a boundary, run `code-moniker check . --report --format json`.
   Use the rule's counts and violations to see which references matched.
   For a disputed reference, run
   `code-moniker extract . --path '<file>' --shape ref --all --format tsv`
   with the project-relative file path. Compare the emitted reference with
   its source line. State any unrepresented form as a limit of that check.
   For indexed relationships, use `code-moniker query 'query.describe'` to
   discover a query for the specific relationship.

4. If competing mechanisms could change the conclusion, gather comparable
   bounded evidence for each one. Heavy edges, hubs, bidirectional relations,
   scattered consumers, and overlapping failures are investigation signals,
   not automatic refactor verdicts.
5. Present findings in this order: indexed facts, architectural
   interpretation, coverage and uncertainty, then recommendation. Preserve
   exact scope, filters, generation, completeness, and unresolved evidence.

## Boundaries

- Do not rename indexed facts to fit an architecture theory or present a
  heuristic as a Code Moniker result.
- Distinguish exact symbol usages from owner roll-ups that include descendants.
- Do not broaden the workspace, request another page, or read source merely to
  make the review appear exhaustive.
- Stop when the scoped question is answered with adequate coverage.
