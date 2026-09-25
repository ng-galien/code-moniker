import { prepare, summarize } from "./graph.js";
import {
  components,
  labelPropagation,
  modularityMoves,
  multilevelLouvain,
  similarityComponents,
  stronglyConnected,
} from "./algorithms.js";
import type { Analysis, CityGraph } from "./model.js";
export * from "./model.js";
export { jaccard } from "./algorithms.js";
export { layoutCity, type CityLayout } from "./layout.js";

export function analyze(
  graph: CityGraph,
  options: { jaccardThreshold?: number; resolution?: number } = {},
): Analysis {
  const jaccardThreshold = options.jaccardThreshold ?? 0.5,
    resolution = options.resolution ?? 1;
  if (
    !Number.isFinite(jaccardThreshold) ||
    jaccardThreshold <= 0 ||
    jaccardThreshold > 1
  )
    throw new Error("Jaccard threshold must be in (0,1]");
  if (!Number.isFinite(resolution) || resolution <= 0)
    throw new Error("Resolution must be positive");
  const g = prepare(graph);
  return {
    version: 1,
    graph: { nodes: g.nodes, edges: g.edges },
    parameters: { jaccardThreshold, resolution },
    partitions: [
      summarize(g, "connected-components", components(g.adjacency)),
      summarize(
        g,
        "jaccard-single-linkage",
        similarityComponents(g, jaccardThreshold),
      ),
      summarize(g, "weighted-label-propagation", labelPropagation(g)),
      summarize(g, "modularity-local-moves", modularityMoves(g, resolution)),
      summarize(g, "louvain", multilevelLouvain(g, resolution)),
    ],
    stronglyConnectedComponents: stronglyConnected(g),
  };
}
