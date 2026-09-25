import test from "node:test";
import assert from "node:assert/strict";
import { captureIndex } from "../dist/node.js";

const symbols = ["a", "b", "lonely"].map((id, i) => ({
  id: `symbol:0:${i}`,
  uri: id,
  root: "/project",
  file: "module.ts",
  name: id,
  kind: "function",
}));
const status = { phase: "ready", generation: 1, root: "/project" };
function client(changeGeneration = false) {
  return {
    workspace: { status: async () => status },
    symbols: {
      search: async (_, opts) => ({
        generation: 1,
        nextCursor: opts.cursor ? null : { offset: 2 },
        data: {
          total: 3,
          rows: opts.cursor ? symbols.slice(2) : symbols.slice(0, 2),
        },
      }),
      usages: async (id, _, opts) => ({
        generation: changeGeneration ? 2 : 1,
        nextCursor: null,
        data: {
          rows:
            id === "symbol:0:1"
              ? [
                  {
                    reference: "ref:1",
                    context: "a",
                    root: "/project",
                    file: "module.ts",
                    kind: "calls",
                  },
                  {
                    reference: "ref:2",
                    context: "outside",
                    root: "/project",
                    file: "elsewhere.ts",
                    kind: "calls",
                  },
                  {
                    reference: "ref:3",
                    context: "a",
                    root: "/project",
                    file: "module.ts",
                    kind: "uses_type",
                    via: "alias",
                  },
                ]
              : [],
        },
      }),
    },
  };
}

test("capture pages inventory, retains isolated nodes and excludes indirect aliases", async () => {
  const result = await captureIndex(client());
  assert.equal(result.graph.nodes.length, 3);
  assert.equal(result.graph.edges.length, 1);
  assert.equal(result.provenance.outsideScope, 1);
  assert.equal(result.provenance.indirectReferencesExcluded, 1);
});
test("mixed generations and oversized scopes fail rather than emitting partial graphs", async () => {
  await assert.rejects(captureIndex(client(true)), /Index changed/);
  await assert.rejects(
    captureIndex(client(), { maxSymbols: 2 }),
    /budget exceeded/,
  );
});
