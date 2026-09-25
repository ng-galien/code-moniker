import test from "node:test";
import assert from "node:assert/strict";
import { analyze, layoutCity } from "../dist/index.js";
import { streetRouter, placeBuildings } from "../dist/streets.js";

test("a hub is more central than its leaves, regardless of lexical position", () => {
  const ids = [...Array.from({ length: 16 }, (_, i) => `leaf${i}`), "zz-hub"];
  const edges = ids
    .slice(0, -1)
    .map((target) => ({ source: "zz-hub", target, weight: 1, kind: "calls" }));
  const neighbors = new Map(
    ids.map((id) => [
      id,
      new Set(id === "zz-hub" ? ids.slice(0, -1) : ["zz-hub"]),
    ]),
  );
  const positions = placeBuildings(ids, neighbors, edges, new Map());
  const hub = positions.find((b) => b.id === "zz-hub"),
    leaves = positions.filter((b) => b !== hub);
  const center = {
    x: leaves.reduce((sum, b) => sum + b.x, 0) / leaves.length,
    z: leaves.reduce((sum, b) => sum + b.z, 0) / leaves.length,
  };
  const distance = (b) => Math.hypot(b.x - center.x, b.z - center.z);
  assert.ok(
    distance(hub) <
      leaves.reduce((sum, b) => sum + distance(b), 0) / leaves.length,
  );
  assert.ok(hub.width > leaves[0].width);
  for (let i = 0; i < positions.length; i++)
    for (let j = i + 1; j < positions.length; j++) {
      const a = positions[i],
        b = positions[j];
      assert.ok(
        Math.abs(a.x - b.x) >= (a.width + b.width) / 2 ||
          Math.abs(a.z - b.z) >= (a.depth + b.depth) / 2,
      );
    }
});

test("routed streets avoid buildings and are reproducible", () => {
  const buildings = [
    { id: "a", x: 0, z: 0, width: 2, depth: 2 },
    { id: "b", x: 24, z: 0, width: 2, depth: 2 },
    { id: "obstacle", x: 12, z: 0, width: 6, depth: 10 },
  ];
  const route = streetRouter(buildings),
    points = route(buildings[0], buildings[1]);
  assert.ok(points.length >= 3);
  assert.deepEqual(points, route(buildings[0], buildings[1]));
  for (let i = 1; i < points.length; i++) {
    const a = points[i - 1],
      b = points[i];
    for (const box of buildings) {
      // Exact slab intersection with the building's rectangular footprint.
      let enter = 0,
        exit = 1;
      for (const [start, delta, min, max] of [
        [a.x, b.x - a.x, box.x - box.width / 2, box.x + box.width / 2],
        [a.z, b.z - a.z, box.z - box.depth / 2, box.z + box.depth / 2],
      ]) {
        if (!delta) {
          if (start < min || start > max) {
            enter = 1;
            exit = 0;
            break;
          }
        } else {
          const p = (min - start) / delta,
            q = (max - start) / delta;
          enter = Math.max(enter, Math.min(p, q));
          exit = Math.min(exit, Math.max(p, q));
        }
      }
      assert.ok(enter > exit);
    }
  }
});

test("every geographic connection has an indexed witness; isolates have no invented roads", () => {
  const graph = {
    nodes: ["a", "b", "c", "d", "isolate"].map((id) => ({ id, label: id })),
    edges: [
      { source: "a", target: "b", kind: "calls", weight: 20 },
      { source: "c", target: "d", kind: "calls", weight: 20 },
      { source: "b", target: "c", kind: "calls", weight: 1 },
    ],
  };
  const layout = layoutCity(analyze(graph));
  for (const row of [
    ...layout.streets,
    ...layout.roads.map((r) => ({
      source: r.gatewaySource,
      target: r.gatewayTarget,
    })),
  ]) {
    assert.ok(
      graph.edges.some(
        (e) =>
          (e.source === row.source && e.target === row.target) ||
          (e.source === row.target && e.target === row.source),
      ),
    );
    assert.notEqual(row.source, "isolate");
    assert.notEqual(row.target, "isolate");
  }
  assert.ok(layout.districts.every((d) => d.boundary.length >= 4));
});

test("empty and isolated inventories still produce complete finite geometry", () => {
  for (const nodes of [[], [{ id: "a", label: "a" }]]) {
    const layout = layoutCity(analyze({ nodes, edges: [] }));
    assert.equal(layout.buildings.length, nodes.length);
    assert.equal(layout.streets.length, 0);
    assert.equal(layout.roads.length, 0);
  }
});
