import type { SimulationNodeDatum } from "d3-force";
import {
  forceSimulation,
  forceLink,
  forceCollide,
  forceX,
  forceY,
  forceRadial,
} from "d3-force";

export interface Building extends SimulationNodeDatum {
  id: string;
  x: number;
  z: number;
  width: number;
  depth: number;
  gateway: boolean;
}
export interface Point {
  x: number;
  z: number;
}
export interface Street {
  source: string;
  target: string;
  weight: number;
  points: Point[];
}
export interface Connection {
  source: string;
  target: string;
  weight: number;
}

/** Local relationships position buildings; external neighbors attract gateways. */
export function placeBuildings(
  members: string[],
  neighbors: Map<string, Set<string>>,
  edges: Connection[],
  gateways: Map<string, Point>,
): Building[] {
  const internalDegree = new Map<string, Set<string>>();
  for (const edge of edges) {
    if (!internalDegree.has(edge.source))
      internalDegree.set(edge.source, new Set());
    if (!internalDegree.has(edge.target))
      internalDegree.set(edge.target, new Set());
    internalDegree.get(edge.source)!.add(edge.target);
    internalDegree.get(edge.target)!.add(edge.source);
  }
  const maxDegree = Math.max(
    1,
    ...[...internalDegree.values()].map((s) => s.size),
  );
  const centrality = (id: string) =>
    (internalDegree.get(id)?.size ?? 0) / maxDegree;
  const nodes: Building[] = [...members]
    .sort((a, b) => centrality(b) - centrality(a) || a.localeCompare(b))
    .map((id) => {
      const degree = neighbors.get(id)?.size ?? 0;
      const width = 1.2 + Math.min(3, Math.log2(1 + degree) * 0.5);
      return {
        id,
        x: NaN,
        z: 0,
        width,
        depth: width * 0.8,
        gateway: gateways.has(id),
      };
    });
  const radius = Math.max(4, Math.sqrt(nodes.length) * 3);
  const sim = forceSimulation(nodes)
    .stop()
    .force(
      "links",
      forceLink<Building, Connection>(edges.map((e) => ({ ...e })))
        .id((n) => n.id)
        .distance(6)
        .strength(0.12),
    )
    .force(
      "collision",
      forceCollide<Building>(
        (n) => Math.hypot(n.width, n.depth) / 2 + 1.2,
      ).iterations(4),
    )
    .force(
      "x",
      forceX<Building>((n) => (gateways.get(n.id)?.x ?? 0) * radius).strength(
        (n) => (n.gateway ? 0.08 : 0.03),
      ),
    )
    .force(
      "y",
      forceY<Building>((n) => (gateways.get(n.id)?.z ?? 0) * radius).strength(
        (n) => (n.gateway ? 0.08 : 0.03),
      ),
    )
    .force(
      "center",
      forceRadial<Building>(0).strength((n) => 0.02 + 0.25 * centrality(n.id)),
    );
  sim.tick(140);
  sim.stop();
  // Resolve residual collisions deterministically and reserve space around each parcel.
  const placed: Building[] = [];
  for (const node of nodes) {
    const origin = { x: node.x, z: node.y ?? 0 };
    let attempt = 0;
    node.z = origin.z;
    while (
      placed.some(
        (b) =>
          Math.abs(b.x - node.x) < (b.width + node.width) / 2 + 2 &&
          Math.abs(b.z - node.z) < (b.depth + node.depth) / 2 + 2,
      )
    ) {
      attempt++;
      const angle = attempt * 2.399963229728653;
      node.x = origin.x + Math.sqrt(attempt) * Math.cos(angle) * 1.5;
      node.z = origin.z + Math.sqrt(attempt) * Math.sin(angle) * 1.5;
    }
    placed.push(node);
  }
  return nodes.map(({ id, x, z, width, depth, gateway }) => ({
    id,
    x,
    z,
    width,
    depth,
    gateway,
  }));
}

/** Monotone hull around parcels, not a circular community plate. */
export function parcelBoundary(buildings: Building[]): Point[] {
  const points = buildings
    .flatMap((b) =>
      [-1, 1].flatMap((sx) =>
        [-1, 1].map((sz) => ({
          x: b.x + sx * (b.width / 2 + 1),
          z: b.z + sz * (b.depth / 2 + 1),
        })),
      ),
    )
    .sort((a, b) => a.x - b.x || a.z - b.z);
  const cross = (a: Point, b: Point, c: Point) =>
    (b.x - a.x) * (c.z - a.z) - (b.z - a.z) * (c.x - a.x);
  const half = (rows: Point[]) => {
    const hull: Point[] = [];
    for (const p of rows) {
      while (hull.length > 1 && cross(hull.at(-2)!, hull.at(-1)!, p) <= 0)
        hull.pop();
      hull.push(p);
    }
    hull.pop();
    return hull;
  };
  return [...half(points), ...half([...points].reverse())];
}

/** Maximum-weight spanning forest: every street is backed by a real relation. */
export function streetBackbone(edges: Connection[]): Connection[] {
  const parents = new Map<string, string>();
  const root = (id: string): string => {
    let current = id;
    while (parents.has(current)) current = parents.get(current)!;
    return current;
  };
  return [...edges]
    .sort(
      (a, b) =>
        b.weight - a.weight ||
        a.source.localeCompare(b.source) ||
        a.target.localeCompare(b.target),
    )
    .filter((e) => {
      const a = root(e.source),
        b = root(e.target);
      if (a === b) return false;
      parents.set(a, b);
      return true;
    });
}

/** Orthogonal A* routing on a clearance grid. Failure is explicit, never a line through a building. */
export function streetRouter(buildings: Building[]) {
  const step = 0.75,
    clearance = 0.35;
  const blocked = new Set<string>();
  const key = (x: number, z: number) => `${x},${z}`;
  for (const b of buildings)
    for (
      let x = Math.floor((b.x - b.width / 2 - clearance) / step);
      x <= Math.ceil((b.x + b.width / 2 + clearance) / step);
      x++
    )
      for (
        let z = Math.floor((b.z - b.depth / 2 - clearance) / step);
        z <= Math.ceil((b.z + b.depth / 2 + clearance) / step);
        z++
      )
        blocked.add(key(x, z));
  const trapped = new Set<string>(),
    open = new Set<string>();
  const hasRoom = (x: number, z: number) => {
    const first = key(x, z);
    if (trapped.has(first)) return false;
    if (open.has(first)) return true;
    const seen = new Set([first]),
      queue: [[number, number]] | [number, number][] = [[x, z]];
    for (let i = 0; i < queue.length && seen.size < 128; i++) {
      const [px, pz] = queue[i]!;
      for (const [dx, dz] of [
        [1, 0],
        [-1, 0],
        [0, 1],
        [0, -1],
      ]) {
        const nx = px + dx!,
          nz = pz + dz!,
          k = key(nx, nz);
        if (!blocked.has(k) && !seen.has(k)) {
          seen.add(k);
          queue.push([nx, nz]);
        }
      }
    }
    const destination = seen.size >= 128 ? open : trapped;
    for (const k of seen) destination.add(k);
    return seen.size >= 128;
  };
  const access = (
    b: Building,
    toward: Building,
    exclude?: string,
  ): [number, number] => {
    const cx = Math.round(b.x / step),
      cz = Math.round(b.z / step);
    for (let r = 1; ; r++) {
      const candidates: [number, number][] = [];
      for (let dx = -r; dx <= r; dx++)
        for (const dz of [-r, r]) candidates.push([cx + dx, cz + dz]);
      for (let dz = -r + 1; dz < r; dz++)
        for (const dx of [-r, r]) candidates.push([cx + dx, cz + dz]);
      const free = candidates.filter(
        ([x, z]) =>
          key(x, z) !== exclude && !blocked.has(key(x, z)) && hasRoom(x, z),
      );
      if (free.length)
        return free.sort(
          (a, c) =>
            Math.hypot(a[0] * step - toward.x, a[1] * step - toward.z) -
            Math.hypot(c[0] * step - toward.x, c[1] * step - toward.z),
        )[0]!;
    }
  };
  // Supercover grid traversal: a shortcut may not pass through even the corner
  // of a blocked cell. This removes staircases without cutting through parcels.
  const visible = (a: Point, b: Point): boolean => {
    let x = Math.round(a.x / step),
      z = Math.round(a.z / step);
    const ex = Math.round(b.x / step),
      ez = Math.round(b.z / step);
    const dx = ex - x,
      dz = ez - z,
      sx = Math.sign(dx),
      sz = Math.sign(dz);
    const tx = dx ? 1 / Math.abs(dx) : Infinity,
      tz = dz ? 1 / Math.abs(dz) : Infinity;
    let nextX = tx / 2,
      nextZ = tz / 2;
    while (x !== ex || z !== ez) {
      if (nextX === nextZ) {
        if (blocked.has(key(x + sx, z)) || blocked.has(key(x, z + sz)))
          return false;
        x += sx;
        z += sz;
        nextX += tx;
        nextZ += tz;
      } else if (nextX < nextZ) {
        x += sx;
        nextX += tx;
      } else {
        z += sz;
        nextZ += tz;
      }
      if (blocked.has(key(x, z))) return false;
    }
    return true;
  };
  return (a: Building, b: Building): Point[] => {
    const start = access(a, b),
      end = access(b, a, key(...start));
    const endKey = key(...end);
    type Entry = { x: number; z: number; cost: number; score: number };
    const heap: Entry[] = [];
    const push = (e: Entry) => {
      heap.push(e);
      let i = heap.length - 1;
      while (i > 0) {
        const p = (i - 1) >> 1;
        if (heap[p]!.score <= e.score) break;
        heap[i] = heap[p]!;
        i = p;
      }
      heap[i] = e;
    };
    const pop = () => {
      const first = heap[0]!,
        last = heap.pop()!;
      if (heap.length) {
        let i = 0;
        while (i * 2 + 1 < heap.length) {
          let j = i * 2 + 1;
          if (j + 1 < heap.length && heap[j + 1]!.score < heap[j]!.score) j++;
          if (last.score <= heap[j]!.score) break;
          heap[i] = heap[j]!;
          i = j;
        }
        heap[i] = last;
      }
      return first;
    };
    const distance = (x: number, z: number) =>
      Math.abs(end[0] - x) + Math.abs(end[1] - z);
    const costs = new Map([[key(...start), 0]]),
      previous = new Map<string, string>();
    push({ x: start[0], z: start[1], cost: 0, score: distance(...start) });
    let visited = 0;
    while (heap.length && visited++ < 24000) {
      const row = pop(),
        current = key(row.x, row.z);
      if (row.cost !== costs.get(current)) continue;
      if (current === endKey) {
        const path: Point[] = [];
        let cursor: string | undefined = current;
        while (cursor) {
          const [x, z] = cursor.split(",").map(Number);
          path.push({ x: x! * step, z: z! * step });
          cursor = previous.get(cursor);
        }
        path.reverse();
        const simplified: Point[] = [path[0]!];
        let anchor = 0;
        while (anchor < path.length - 1) {
          let next = anchor + 1;
          while (
            next + 1 < path.length &&
            visible(path[anchor]!, path[next + 1]!)
          )
            next++;
          simplified.push(path[next]!);
          anchor = next;
        }
        return simplified;
      }
      for (const [dx, dz] of [
        [1, 0],
        [0, 1],
        [-1, 0],
        [0, -1],
      ]) {
        const x = row.x + dx!,
          z = row.z + dz!,
          next = key(x, z),
          cost = row.cost + 1;
        if (blocked.has(next) || cost >= (costs.get(next) ?? Infinity))
          continue;
        costs.set(next, cost);
        previous.set(next, current);
        push({ x, z, cost, score: cost + distance(x, z) });
      }
    }
    return [];
  };
}
