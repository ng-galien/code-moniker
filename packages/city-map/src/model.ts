export interface CityNode {
  id: string;
  label: string;
  uri?: string;
  file?: string;
  kind?: string;
}
export interface CityEdge {
  source: string;
  target: string;
  kind: string;
  weight: number;
}
export interface CityGraph {
  nodes: CityNode[];
  edges: CityEdge[];
}
export interface Community {
  id: string;
  members: string[];
  internalWeight: number;
  boundaryWeight: number;
}
export interface Partition {
  algorithm: string;
  communities: Community[];
  metrics: {
    modularity: number;
    internalWeightRatio: number | null;
    isolatedNodes: number;
    largestCommunityRatio: number;
  };
}
export interface Analysis {
  version: 1;
  graph: CityGraph;
  partitions: Partition[];
  stronglyConnectedComponents: string[][];
  parameters: { jaccardThreshold: number; resolution: number };
}
