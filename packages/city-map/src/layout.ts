import {
  forceSimulation,
  forceLink,
  forceManyBody,
  forceCollide,
  forceX,
  forceY,
  type SimulationNodeDatum,
  type SimulationLinkDatum,
} from "d3-force";
import type { Analysis } from "./model.js";
import {
  placeBuildings,
  parcelBoundary,
  streetBackbone,
  streetRouter,
  type Building,
  type Street,
  type Point,
  type Connection,
} from "./streets.js";
interface District extends SimulationNodeDatum {
  id: string;
  radius: number;
  members: string[];
}
interface Road extends SimulationLinkDatum<District> {
  weight: number;
}
export interface CityLayout {
  version: 2;
  districts: Array<{
    id: string;
    x: number;
    z: number;
    radius: number;
    members: string[];
    boundary: Point[];
  }>;
  buildings: Building[];
  roads: Array<{
    source: string;
    target: string;
    weight: number;
    gatewaySource: string;
    gatewayTarget: string;
    points: Point[];
  }>;
  streets: Street[];
  unrouted: number;
  localStreetCandidates: number;
  isolated: string[];
}
/** Reproducible community-force layout; coordinates are not calibrated distances. */
export function layoutCity(
  analysis: Analysis,
  algorithm = "louvain",
): CityLayout {
  const partition = analysis.partitions.find((p) => p.algorithm === algorithm);
  if (!partition) throw new Error(`Unknown partition: ${algorithm}`);
  const linked = new Set<string>();
  const neighbors = new Map<string, Set<string>>();
  for (const e of analysis.graph.edges)
    if (e.source !== e.target) {
      linked.add(e.source);
      linked.add(e.target);
      if (!neighbors.has(e.source)) neighbors.set(e.source, new Set());
      if (!neighbors.has(e.target)) neighbors.set(e.target, new Set());
      neighbors.get(e.source)!.add(e.target);
      neighbors.get(e.target)!.add(e.source);
    }
  const isolated = analysis.graph.nodes
    .filter((n) => !linked.has(n.id))
    .map((n) => n.id);
  const districts: District[] = partition.communities
    .filter((c) => c.members.some((id) => linked.has(id)))
    .map((c) => ({
      id: c.id,
      radius: 2.3 * Math.sqrt(c.members.length) + 3,
      members: c.members,
    }));
  const byNode = new Map(
    districts.flatMap((d) => d.members.map((id) => [id, d.id] as const)),
  );
  const aggregated = new Map<
    string,
    { source: string; target: string; weight: number }
  >();
  for (const e of analysis.graph.edges) {
    const a = byNode.get(e.source),
      b = byNode.get(e.target);
    if (!a || !b || a === b) continue;
    const [source, target] = [a, b].sort();
    const key = JSON.stringify([source, target]);
    const row = aggregated.get(key) ?? {
      source: source!,
      target: target!,
      weight: 0,
    };
    row.weight += e.weight;
    aggregated.set(key, row);
  }
  const roads = [...aggregated.values()].sort(
    (a, b) =>
      a.source.localeCompare(b.source) || a.target.localeCompare(b.target),
  );
  const radius = new Map(districts.map((d) => [d.id, d.radius]));
  const edges: Road[] = roads.map((r) => ({ ...r }));
  const simulation = forceSimulation(districts)
    .stop()
    .force(
      "charge",
      forceManyBody<District>().strength((d) => -d.radius),
    )
    .force(
      "collision",
      forceCollide<District>((d) => d.radius + 2).iterations(4),
    )
    .force(
      "links",
      forceLink<District, Road>(edges)
        .id((d) => d.id)
        .distance(
          (l) =>
            radius.get((l.source as District).id)! +
            radius.get((l.target as District).id)! +
            10 +
            25 / (1 + Math.log1p(l.weight)),
        )
        .strength((l) => Math.min(0.5, 0.04 * Math.log1p(l.weight))),
    )
    .force("x", forceX(0).strength(0.12))
    .force("y", forceY(0).strength(0.12));
  simulation.tick(240);
  simulation.stop();
  const districtById = new Map(districts.map((d) => [d.id, d]));
  const internals = new Map<string, Connection[]>();
  const gatewayDirections = new Map<string, Point>();
  const gatewayWeights = new Map<string, number>();
  const witnesses = new Map<string, Connection>();
  for (const e of analysis.graph.edges) {
    if (e.source === e.target) continue;
    const a = byNode.get(e.source),
      b = byNode.get(e.target);
    if (!a || !b) continue;
    if (a === b) {
      const rows = internals.get(a) ?? [];
      rows.push(e);
      internals.set(a, rows);
    } else {
      const ids = [a, b].sort(),
        key = JSON.stringify(ids);
      const oriented =
        a === ids[0] ? e : { ...e, source: e.target, target: e.source };
      if (!witnesses.has(key) || witnesses.get(key)!.weight < e.weight)
        witnesses.set(key, oriented);
      for (const [node, from, to] of [
        [e.source, a, b],
        [e.target, b, a],
      ] as const) {
        const p = districtById.get(from)!,
          q = districtById.get(to)!;
        const dx = q.x! - p.x!,
          dz = q.y! - p.y!,
          len = Math.hypot(dx, dz) || 1;
        const v = gatewayDirections.get(node) ?? { x: 0, z: 0 };
        v.x += (dx / len) * e.weight;
        v.z += (dz / len) * e.weight;
        gatewayDirections.set(node, v);
        gatewayWeights.set(node, (gatewayWeights.get(node) ?? 0) + e.weight);
      }
    }
  }
  for (const [id, p] of gatewayDirections) {
    p.x /= gatewayWeights.get(id)!;
    p.z /= gatewayWeights.get(id)!;
  }
  const locals = new Map<string, Building[]>();
  for (const d of districts) {
    const members = placeBuildings(
      d.members,
      neighbors,
      internals.get(d.id) ?? [],
      gatewayDirections,
    );
    locals.set(d.id, members);
    d.radius = Math.max(
      4,
      ...members.map(
        (b) => Math.hypot(b.x, b.z) + Math.hypot(b.width, b.depth) / 2 + 2,
      ),
    );
    radius.set(d.id, d.radius);
  }
  // Recompute collision radii after the internal geography has reserved its actual area.
  simulation.force(
    "collision",
    forceCollide<District>((d) => d.radius + 3).iterations(5),
  );
  simulation.force(
    "links",
    forceLink<District, Road>(edges)
      .id((d) => d.id)
      .distance(
        (l) =>
          radius.get((l.source as District).id)! +
          radius.get((l.target as District).id)! +
          12,
      )
      .strength(0.05),
  );
  simulation.alpha(1).tick(200);
  simulation.stop();
  // A final non-overlap pass is deterministic, with no reliance on simulation convergence.
  const settled: District[] = [];
  for (const d of [...districts].sort(
    (a, b) => b.radius - a.radius || a.id.localeCompare(b.id),
  )) {
    const x = d.x!,
      y = d.y!;
    let attempt = 0;
    while (
      settled.some(
        (other) =>
          Math.hypot(d.x! - other.x!, d.y! - other.y!) <
          d.radius + other.radius + 2,
      )
    ) {
      attempt++;
      d.x = x + 2 * Math.sqrt(attempt) * Math.cos(attempt * 2.399963229728653);
      d.y = y + 2 * Math.sqrt(attempt) * Math.sin(attempt * 2.399963229728653);
    }
    settled.push(d);
  }
  const buildings: Building[] = districts.flatMap((d) =>
    locals.get(d.id)!.map((b) => ({ ...b, x: b.x + d.x!, z: b.z + d.y! })),
  );
  const right = Math.max(0, ...districts.map((d) => d.x! + d.radius)),
    columns = Math.max(1, Math.ceil(Math.sqrt(isolated.length) * 0.6));
  isolated.forEach((id, i) => {
    buildings.push({
      id,
      x: right + 16 + (i % columns) * 2,
      z:
        (Math.floor(i / columns) - Math.ceil(isolated.length / columns) / 2) *
        2,
      width: 1.2,
      depth: 0.96,
      gateway: false,
    });
  });
  const buildingsById = new Map(buildings.map((b) => [b.id, b]));
  const route = streetRouter(buildings);
  let unrouted = 0;
  const routedRoads = roads.map((r) => {
    const witness = witnesses.get(JSON.stringify([r.source, r.target]))!;
    return {
      ...r,
      gatewaySource: witness.source,
      gatewayTarget: witness.target,
      points: [] as Point[],
    };
  });
  for (const r of [...routedRoads]
    .sort((a, b) => b.weight - a.weight)
    .slice(0, 80)) {
    r.points = route(
      buildingsById.get(r.gatewaySource)!,
      buildingsById.get(r.gatewayTarget)!,
    );
    if (!r.points.length) unrouted++;
  }
  const backbone = [...internals.values()]
    .flatMap(streetBackbone)
    .sort((a, b) => b.weight - a.weight || a.source.localeCompare(b.source));
  const streets = backbone
    .slice(0, 1200)
    .map((e) => ({
      ...e,
      points: route(buildingsById.get(e.source)!, buildingsById.get(e.target)!),
    }));
  unrouted += streets.filter((s) => !s.points.length).length;
  return {
    version: 2,
    districts: districts.map((d) => ({
      id: d.id,
      x: d.x!,
      z: d.y!,
      radius: d.radius,
      members: d.members,
      boundary: parcelBoundary(locals.get(d.id)!).map((p) => ({
        x: p.x + d.x!,
        z: p.z + d.y!,
      })),
    })),
    buildings,
    roads: routedRoads,
    streets,
    unrouted,
    localStreetCandidates: backbone.length,
    isolated,
  };
}
