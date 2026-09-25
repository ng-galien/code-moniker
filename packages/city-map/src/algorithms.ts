import type { Prepared } from "./graph.js";
import { UndirectedGraph } from "graphology";
import louvain from "graphology-communities-louvain";

export function multilevelLouvain(g: Prepared, resolution: number): number[] {
  if (!g.totalWeight) return g.nodes.map((_, i) => i);
  const graph = new UndirectedGraph();
  g.nodes.forEach((_, i) => graph.addNode(String(i)));
  g.adjacency.forEach((ns, a) => {
    for (const [b, weight] of ns)
      if (a < b) graph.addEdge(String(a), String(b), { weight });
  });
  const result = louvain(graph, { resolution, randomWalk: false });
  // Louvain can leave disconnected communities. Split those before presenting districts.
  return components(
    g.adjacency.map(
      (ns, a) => new Map([...ns].filter(([b]) => result[a] === result[b])),
    ),
  );
}

export function components(
  adjacency: ReadonlyArray<ReadonlyMap<number, number>>,
): number[] {
  const labels = adjacency.map(() => -1);
  for (let i = 0; i < labels.length; i++) {
    if (labels[i] !== -1) continue;
    const stack = [i];
    labels[i] = i;
    while (stack.length) {
      const a = stack.pop()!;
      for (const b of adjacency[a]!.keys())
        if (labels[b] === -1) {
          labels[b] = i;
          stack.push(b);
        }
    }
  }
  return labels;
}

// Reference set-algebra implementation. It does not claim Roaring performance.
export function jaccard(
  a: ReadonlySet<number>,
  b: ReadonlySet<number>,
): number {
  let common = 0;
  const small = a.size < b.size ? a : b,
    large = small === a ? b : a;
  for (const item of small) if (large.has(item)) common++;
  const union = a.size + b.size - common;
  return union ? common / union : 0;
}

export function similarityComponents(g: Prepared, threshold: number): number[] {
  // Only existing undirected edges are candidates: bounded by E, not all N² pairs.
  // Closed neighborhoods let strongly connected cliques match. This is single linkage.
  const sets = g.adjacency.map((ns, i) => new Set([i, ...ns.keys()]));
  const similar = g.nodes.map(() => new Map<number, number>());
  for (let a = 0; a < sets.length; a++)
    for (const b of g.adjacency[a]!.keys()) {
      if (b <= a) continue;
      const score = jaccard(sets[a]!, sets[b]!);
      if (score >= threshold) {
        similar[a]!.set(b, score);
        similar[b]!.set(a, score);
      }
    }
  return components(similar);
}

export function labelPropagation(g: Prepared): number[] {
  const labels = g.nodes.map((_, i) => i);
  // Asynchronous updates; keep the existing label on a tie to avoid two-node oscillation.
  for (let pass = 0; pass < 100; pass++) {
    let moved = false;
    for (let a = 0; a < labels.length; a++) {
      const scores = new Map<number, number>();
      for (const [b, w] of g.adjacency[a]!)
        scores.set(labels[b]!, (scores.get(labels[b]!) ?? 0) + w);
      let best = labels[a]!,
        score = scores.get(best) ?? 0;
      for (const [label, candidate] of [...scores].sort(
        (a, b) => a[0] - b[0],
      )) {
        if (candidate > score + 1e-12) {
          best = label;
          score = candidate;
        }
      }
      moved ||= labels[a] !== best;
      labels[a] = best;
    }
    if (!moved) break;
  }
  return labels;
}

export function modularityMoves(g: Prepared, resolution: number): number[] {
  const labels = g.nodes.map((_, i) => i),
    totals = [...g.degrees];
  if (!g.totalWeight) return labels;
  // Single-level modularity optimization, not full multilevel Louvain or Leiden.
  for (let pass = 0; pass < 100; pass++) {
    let moved = false;
    for (let a = 0; a < labels.length; a++) {
      const degree = g.degrees[a]!;
      if (!degree) continue;
      const old = labels[a]!;
      totals[old] = totals[old]! - degree;
      const weights = new Map<number, number>();
      for (const [b, w] of g.adjacency[a]!)
        weights.set(labels[b]!, (weights.get(labels[b]!) ?? 0) + w);
      const score = (label: number) =>
        (weights.get(label) ?? 0) -
        (resolution * degree * totals[label]!) / (2 * g.totalWeight);
      let best = old,
        bestScore = score(old);
      for (const label of [...weights.keys()].sort((a, b) => a - b)) {
        const candidate = score(label);
        if (candidate > bestScore + 1e-12) {
          best = label;
          bestScore = candidate;
        }
      }
      totals[best] = totals[best]! + degree;
      labels[a] = best;
      moved ||= best !== old;
    }
    if (!moved) break;
  }
  // Moving articulation vertices can disconnect groups; expose separate connected parts.
  const within = g.adjacency.map(
    (ns, a) => new Map([...ns].filter(([b]) => labels[a] === labels[b])),
  );
  return components(within);
}

export function stronglyConnected(g: Prepared): string[][] {
  const seen = new Set<number>(),
    order: number[] = [];
  for (let start = 0; start < g.nodes.length; start++) {
    if (seen.has(start)) continue;
    seen.add(start);
    const stack: Array<[number, Iterator<number>]> = [
      [start, g.outgoing[start]!.values()],
    ];
    while (stack.length) {
      const [a, it] = stack[stack.length - 1]!;
      const next = it.next();
      if (next.done) {
        order.push(a);
        stack.pop();
      } else if (!seen.has(next.value)) {
        seen.add(next.value);
        stack.push([next.value, g.outgoing[next.value]!.values()]);
      }
    }
  }
  seen.clear();
  const result: string[][] = [];
  for (const start of order.reverse()) {
    if (seen.has(start)) continue;
    const stack = [start],
      group: string[] = [];
    seen.add(start);
    while (stack.length) {
      const a = stack.pop()!;
      group.push(g.nodes[a]!.id);
      for (const b of g.incoming[a]!)
        if (!seen.has(b)) {
          seen.add(b);
          stack.push(b);
        }
    }
    result.push(group.sort());
  }
  return result.sort((a, b) => a[0]!.localeCompare(b[0]!));
}
