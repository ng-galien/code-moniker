import {
  BoxGeometry,
  Group,
  InstancedMesh,
  MeshStandardMaterial,
  MeshBasicMaterial,
  Matrix4,
  Color,
  Mesh,
  Vector2,
  Shape,
  ShapeGeometry,
  Float32BufferAttribute,
  BufferGeometry,
} from "three";
import type { Analysis } from "./model.js";
import { layoutCity, type CityLayout } from "./layout.js";

/** First deterministic scene scaffold; layout stability across revisions is not yet implemented. */
export function createCityScene(
  analysis: Analysis,
  algorithm = "louvain",
  suppliedLayout?: CityLayout,
) {
  const layout =
    suppliedLayout?.version === 2
      ? suppliedLayout
      : layoutCity(analysis, algorithm);
  const group = new Group();
  group.name = "Code Moniker City Map";
  const geometry = new BoxGeometry(1, 1, 1),
    material = new MeshStandardMaterial({ roughness: 0.85, metalness: 0 });
  const mesh = new InstancedMesh(
    geometry,
    material,
    analysis.graph.nodes.length,
  );
  const podium = new InstancedMesh(
    geometry,
    material,
    analysis.graph.nodes.length,
  );
  const roofGeometry = new BufferGeometry();
  // Shallow gable roof for low-rise buildings; all dimensions remain within their parcel.
  roofGeometry.setAttribute(
    "position",
    new Float32BufferAttribute(
      [
        -0.5, 0, -0.5, 0.5, 0, -0.5, 0, 1, -0.5, -0.5, 0, 0.5, 0, 1, 0.5, 0.5,
        0, 0.5, -0.5, 0, -0.5, 0, 1, -0.5, 0, 1, 0.5, -0.5, 0, -0.5, 0, 1, 0.5,
        -0.5, 0, 0.5, 0.5, 0, -0.5, 0.5, 0, 0.5, 0, 1, 0.5, 0.5, 0, -0.5, 0, 1,
        0.5, 0, 1, -0.5,
      ],
      3,
    ),
  );
  roofGeometry.computeVertexNormals();
  const roofMaterial = new MeshStandardMaterial({
    color: "#71827b",
    roughness: 1,
  });
  const roofs = new InstancedMesh(
    roofGeometry,
    roofMaterial,
    analysis.graph.nodes.length,
  );
  const caps = new InstancedMesh(
    geometry,
    roofMaterial,
    analysis.graph.nodes.length,
  );
  const nodeByInstance: string[] = [],
    transform = new Matrix4();
  const incoming = new Map<string, number>();
  for (const e of analysis.graph.edges)
    if (e.source !== e.target)
      incoming.set(e.target, (incoming.get(e.target) ?? 0) + e.weight);
  const isolated = new Set(layout.isolated);
  layout.buildings.forEach((b, i) => {
    const height = 1 + Math.log2(1 + (incoming.get(b.id) ?? 0));
    transform
      .makeScale(b.width * 0.82, height, b.depth * 0.82)
      .setPosition(b.x, height / 2, b.z);
    mesh.setMatrixAt(i, transform);
    mesh.setColorAt(i, new Color(isolated.has(b.id) ? "#52625e" : "#a1b5b0"));
    nodeByInstance.push(b.id);
    transform
      .makeScale(b.width, Math.min(0.65, height * 0.3), b.depth)
      .setPosition(b.x, Math.min(0.65, height * 0.3) / 2, b.z);
    podium.setMatrixAt(i, transform);
    podium.setColorAt(i, new Color(isolated.has(b.id) ? "#44534e" : "#839790"));
    const low = height < 3.5;
    transform
      .makeScale(
        low ? b.width * 0.92 : 0,
        low ? 0.45 : 0,
        low ? b.depth * 0.92 : 0,
      )
      .setPosition(b.x, height, b.z);
    roofs.setMatrixAt(i, transform);
    transform
      .makeScale(
        low ? 0 : b.width * 0.6,
        low ? 0 : 0.35,
        low ? 0 : b.depth * 0.6,
      )
      .setPosition(b.x, height + 0.175, b.z);
    caps.setMatrixAt(i, transform);
  });
  mesh.instanceMatrix.needsUpdate = true;
  if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
  group.add(mesh, podium, roofs, caps);
  const groundGeometries: BufferGeometry[] = [];
  const groundMaterial = new MeshBasicMaterial({ color: "#1a2421" });
  for (const d of layout.districts) {
    const shape = new Shape(d.boundary.map((p) => new Vector2(p.x, -p.z)));
    const groundGeometry = new ShapeGeometry(shape);
    groundGeometries.push(groundGeometry);
    const plate = new Mesh(groundGeometry, groundMaterial);
    plate.rotation.x = -Math.PI / 2;
    plate.position.y = -0.04;
    group.add(plate);
  }
  const roadMaterial = new MeshBasicMaterial({ color: "#4d5e53" });
  const streetMaterial = new MeshBasicMaterial({ color: "#384d40" });
  const roadGeometries: BufferGeometry[] = [];
  const roads = [...layout.roads]
    .sort((a, b) => b.weight - a.weight)
    .slice(0, 80)
    .filter((r) => r.points.length > 1);
  for (const [rows, material, width] of [
    [layout.streets, streetMaterial, 0.28],
    [roads, roadMaterial, 0.6],
  ] as const) {
    const vertices: number[] = [];
    for (const row of rows)
      for (let i = 1; i < row.points.length; i++) {
        const a = row.points[i - 1]!,
          b = row.points[i]!;
        const len = Math.hypot(b.x - a.x, b.z - a.z);
        if (!len) continue;
        const dx = ((-(b.z - a.z) / len) * width) / 2,
          dz = (((b.x - a.x) / len) * width) / 2;
        const y = width > 0.3 ? 0.06 : 0.04;
        vertices.push(
          a.x - dx,
          y,
          a.z - dz,
          a.x + dx,
          y,
          a.z + dz,
          b.x + dx,
          y,
          b.z + dz,
          a.x - dx,
          y,
          a.z - dz,
          b.x + dx,
          y,
          b.z + dz,
          b.x - dx,
          y,
          b.z - dz,
        );
      }
    const roadGeometry = new BufferGeometry();
    roadGeometry.setAttribute(
      "position",
      new Float32BufferAttribute(vertices, 3),
    );
    roadGeometries.push(roadGeometry);
    group.add(new Mesh(roadGeometry, material));
  }
  return {
    group,
    mesh,
    pickable: [mesh, podium, roofs, caps],
    nodeByInstance,
    layout,
    roadsShown: roads.length,
    dispose() {
      geometry.dispose();
      material.dispose();
      mesh.dispose();
      podium.dispose();
      roofs.dispose();
      caps.dispose();
      roofGeometry.dispose();
      roofMaterial.dispose();
      groundGeometries.forEach((g) => g.dispose());
      groundMaterial.dispose();
      roadMaterial.dispose();
      streetMaterial.dispose();
      roadGeometries.forEach((g) => g.dispose());
    },
  };
}
