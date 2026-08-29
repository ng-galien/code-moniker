# Onboard

Use this posture to enter an unfamiliar project and establish its vocabulary
and structural map before choosing an implementation target. Complete it once
per project context; repeat it only when the rule corpus, roots, or connection
changed materially.

## Workflow

1. Start with the project-authored corpus:

   ```sh
   code-moniker rules show .
   ```

2. If the output contains rules, run `code-moniker rules learn basics` before
   interpreting their structure. If it declares a taxonomy, also run
   `code-moniker rules learn taxonomy` before interpreting its patterns,
   components, rule ids, aliases, or diagnostics. The initial `rules show`
   call detects the project vocabulary; it does not replace the required learn
   topic.
3. Read the unfiltered corpus as the project's declared map. If no taxonomy or
   rules exist, record that absence and continue without inventing vocabulary.
4. With MCP, verify the absolute `expected_roots` and read `workspace` with a
   small budget. In CLI-only mode, use `code-moniker stats .`. Relate language,
   scale, and concentration facts to the project vocabulary without assuming a
   component is a directory or language module.
5. Read `workspace/views` only when project intent or a contextual architecture
   view matters. Before interpreting or using fragments and view identifiers,
   run `code-moniker rules learn fragments`; follow only a view URI returned by
   the listing.
6. Hand the resulting vocabulary, relevant views, coverage limits, and likely
   areas of interest to the Develop or Review posture.

## Boundaries

- Do not select the first plausible symbol before establishing the project map.
- Do not treat taxonomy classification as proof that the declared architecture
  is correct or fully covered by the current index.
- If MCP reports `workspace_mismatch`, stop and fix the project binding instead
  of querying or refreshing the wrong workspace.
- Stop once the map is sufficient to focus the actual task.
