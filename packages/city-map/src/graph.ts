import type { CityGraph, Partition } from "./model.js";

export function prepare(input: CityGraph) {
  const nodes = [...input.nodes].sort((a, b) =>
    a.id < b.id ? -1 : a.id > b.id ? 1 : 0,
  );
  const ordinals = new Map(nodes.map((n, i) => [n.id, i]));
  if (ordinals.size !== nodes.length) throw new Error("Duplicate node IDs");
  const adjacency = nodes.map(() => new Map<number, number>());
  const outgoing = nodes.map(() => new Set<number>());
  const incoming = nodes.map(() => new Set<number>());
  let totalWeight = 0;
  const edges = [...input.edges].sort((a, b) =>
    JSON.stringify(a).localeCompare(JSON.stringify(b)),
  );
  for (const e of edges) {
    const a = ordinals.get(e.source),
      b = ordinals.get(e.target);
    if (a === undefined || b === undefined)
      throw new Error("Edge endpoint missing from node inventory");
    if (!Number.isFinite(e.weight) || e.weight <= 0)
      throw new Error("Edge weight must be finite and positive");
    outgoing[a]!.add(b);
    incoming[b]!.add(a);
    // Self references remain in the evidence but do not influence grouping.
    if (a === b) continue;
    adjacency[a]!.set(b, (adjacency[a]!.get(b) ?? 0) + e.weight);
    adjacency[b]!.set(a, (adjacency[b]!.get(a) ?? 0) + e.weight);
    totalWeight += e.weight;
  }
  const degrees = adjacency.map((ns) =>
    [...ns.values()].reduce((a, b) => a + b, 0),
  );
  return { nodes, edges, adjacency, outgoing, incoming, degrees, totalWeight };
}
export type Prepared = ReturnType<typeof prepare>;

export function summarize(
  g: Prepared,
  algorithm: string,
  labels: number[],
): Partition {
  const groups = new Map<number, number[]>();
  labels.forEach((label, i) => {
    const group = groups.get(label) ?? [];
    group.push(i);
    groups.set(label, group);
  });
  let modularity = 0,
    internalTotal = 0;
  const communities = [...groups.values()].map((members) => {
    const set = new Set(members);
    let internalWeight = 0,
      boundaryWeight = 0,
      degree = 0;
    for (const a of members) {
      degree += g.degrees[a]!;
      for (const [b, w] of g.adjacency[a]!) {
        if (set.has(b)) internalWeight += w / 2;
        else boundaryWeight += w;
      }
    }
    internalTotal += internalWeight;
    if (g.totalWeight)
      modularity +=
        internalWeight / g.totalWeight - (degree / (2 * g.totalWeight)) ** 2;
    return {
      id: g.nodes[members[0]!]!.id,
      members: members.map((i) => g.nodes[i]!.id),
      internalWeight,
      boundaryWeight,
    };
  });
  return {
    algorithm,
    communities,
    metrics: {
      modularity,
      internalWeightRatio: g.totalWeight ? internalTotal / g.totalWeight : null,
      isolatedNodes: g.degrees.filter((d) => d === 0).length,
      largestCommunityRatio: g.nodes.length
        ? Math.max(0, ...communities.map((c) => c.members.length)) /
          g.nodes.length
        : 0,
    },
  };
}
