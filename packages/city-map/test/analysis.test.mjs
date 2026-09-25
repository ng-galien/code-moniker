import test from "node:test";
import assert from "node:assert/strict";
import { analyze, jaccard, layoutCity } from "../dist/index.js";
import { createCityScene } from "../dist/three.js";

function bridgeGraph() {
  const nodes = ["a", "b", "c", "d", "e", "f", "isolated"].map((id) => ({
    id,
    label: id,
  }));
  const edges = [
    ["a", "b", 10],
    ["b", "c", 10],
    ["c", "a", 10],
    ["d", "e", 10],
    ["e", "f", 10],
    ["f", "d", 10],
    ["c", "d", 1],
  ].map(([source, target, weight]) => ({
    source,
    target,
    weight,
    kind: "calls",
  }));
  return { nodes, edges };
}

test("dense groups separated by a weak bridge; isolated inventory is preserved", () => {
  const result = analyze(bridgeGraph());
  for (const p of result.partitions) {
    assert.deepEqual(p.communities.flatMap((c) => c.members).sort(), [
      "a",
      "b",
      "c",
      "d",
      "e",
      "f",
      "isolated",
    ]);
    assert.equal(p.metrics.isolatedNodes, 1);
    if (p.algorithm === "connected-components") {
      assert.equal(p.communities.length, 2);
      continue;
    }
    assert.deepEqual(
      p.communities.map((c) => c.members),
      [["a", "b", "c"], ["d", "e", "f"], ["isolated"]],
    );
    assert.ok(p.metrics.modularity > 0.48);
  }
  assert.deepEqual(result.stronglyConnectedComponents, [
    ["a", "b", "c"],
    ["d", "e", "f"],
    ["isolated"],
  ]);
});

test("two-node label propagation converges to one group", () => {
  const result = analyze({
    nodes: [
      { id: "a", label: "a" },
      { id: "b", label: "b" },
    ],
    edges: [{ source: "a", target: "b", kind: "calls", weight: 1 }],
  });
  assert.equal(
    result.partitions.find((p) => p.algorithm === "weighted-label-propagation")
      .communities.length,
    1,
  );
});

test("partition is independent of input order and display names/files", () => {
  const graph = bridgeGraph(),
    first = analyze(graph);
  const reordered = {
    nodes: [...graph.nodes]
      .reverse()
      .map((n) => ({ ...n, label: "renamed", file: "same/arbitrary/folder" })),
    edges: [...graph.edges].reverse(),
  };
  assert.deepEqual(analyze(reordered).partitions, first.partitions);
});

test("set similarity has reference values", () => {
  assert.equal(jaccard(new Set([1, 2]), new Set([2, 3])), 1 / 3);
  assert.equal(jaccard(new Set(), new Set()), 0);
});

test("empty graph, singleton and self-loops do not manufacture connectivity", () => {
  assert.equal(
    analyze({ nodes: [], edges: [] }).partitions[0].metrics.internalWeightRatio,
    null,
  );
  const result = analyze({
    nodes: [{ id: "x", label: "x" }],
    edges: [{ source: "x", target: "x", kind: "calls", weight: 1 }],
  });
  assert.equal(result.partitions[0].metrics.isolatedNodes, 1);
  assert.equal(result.graph.edges.length, 1);
});

test("invalid inputs fail explicitly", () => {
  assert.throws(
    () => analyze({ nodes: [{ id: "a" }, { id: "a" }], edges: [] }),
    /Duplicate/,
  );
  assert.throws(
    () =>
      analyze({
        nodes: [],
        edges: [
          { source: "missing", target: "other", kind: "calls", weight: 1 },
        ],
      }),
    /endpoint/,
  );
  assert.throws(
    () => analyze(bridgeGraph(), { jaccardThreshold: NaN }),
    /threshold/,
  );
});

test("Three.js adapter preserves every symbol and releases scene resources", () => {
  const scene = createCityScene(analyze(bridgeGraph()));
  assert.equal(scene.mesh.count, 7);
  assert.equal(new Set(scene.nodeByInstance).size, 7);
  assert.ok(scene.group.children.length >= 1);
  scene.dispose();
});

test("force layout is deterministic, complete, finite and keeps isolates explicit", () => {
  const result = analyze(bridgeGraph()),
    first = layoutCity(result);
  assert.deepEqual(layoutCity(result), first);
  assert.equal(first.buildings.length, result.graph.nodes.length);
  assert.deepEqual(first.isolated, ["isolated"]);
  assert.equal(first.roads.length, 1);
  assert.ok(
    first.buildings.every((b) => Number.isFinite(b.x) && Number.isFinite(b.z)),
  );
});
