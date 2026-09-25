import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { createCityScene } from "../src/three.js";
import type { Analysis } from "../src/model.js";
import "./style.css";

const el = (id: string) => document.getElementById(id)!;
async function main() {
  const response = await fetch("/data.json");
  if (!response.ok) throw new Error((await response.json()).error);
  const data = await response.json(),
    analysis: Analysis = data.analysis;
  const container = el("viewport");
  const renderer = new THREE.WebGLRenderer({ antialias: true });
  renderer.setPixelRatio(Math.min(devicePixelRatio, 2));
  container.append(renderer.domElement);
  const tooltip = document.createElement("div");
  tooltip.className = "building-tooltip";
  tooltip.setAttribute("role", "tooltip");
  tooltip.hidden = true;
  const tooltipTitle = document.createElement("strong"),
    tooltipKind = document.createElement("span"),
    tooltipStats = document.createElement("p"),
    tooltipFile = document.createElement("small"),
    tooltipHint = document.createElement("small");
  tooltipHint.textContent = "Cliquer pour explorer les relations";
  tooltipHint.className = "tooltip-hint";
  tooltip.append(
    tooltipKind,
    tooltipTitle,
    tooltipStats,
    tooltipFile,
    tooltipHint,
  );
  container.append(tooltip);
  const nodeById = new Map(analysis.graph.nodes.map((n) => [n.id, n]));
  const neighborhood = new Map<string, Set<string>>(),
    incomingCounts = new Map<string, number>();
  for (const e of analysis.graph.edges) {
    if (e.source === e.target) continue;
    for (const [a, b] of [
      [e.source, e.target],
      [e.target, e.source],
    ]) {
      if (!neighborhood.has(a!)) neighborhood.set(a!, new Set());
      neighborhood.get(a!)!.add(b!);
    }
    incomingCounts.set(
      e.target,
      (incomingCounts.get(e.target) ?? 0) + e.weight,
    );
  }
  const scene = new THREE.Scene();
  scene.background = new THREE.Color("#141a1b");
  scene.add(new THREE.HemisphereLight(0xe1eeee, 0x344139, 2.5));
  const light = new THREE.DirectionalLight(0xfff2d8, 2);
  light.position.set(-20, 50, 20);
  scene.add(light);
  const camera = new THREE.OrthographicCamera(-60, 60, 60, -60, 0.1, 5000);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = !matchMedia("(prefers-reduced-motion: reduce)")
    .matches;
  controls.maxPolarAngle = Math.PI / 2.1;
  let city = createCityScene(analysis, "louvain", data.layouts?.louvain),
    links: THREE.LineSegments | undefined;
  scene.add(city.group);
  const matrix = new THREE.Matrix4(),
    positions = new Map<string, THREE.Vector3>();
  function fit() {
    city.mesh.computeBoundingBox();
    const bounds = new THREE.Box3().setFromObject(city.group);
    const center = bounds.getCenter(new THREE.Vector3()),
      size = bounds.getSize(new THREE.Vector3());
    const aspect = container.clientWidth / container.clientHeight,
      span = Math.max(size.x, size.z, 5);
    camera.position.copy(center).add(new THREE.Vector3(span, span * 1.5, span));
    camera.lookAt(center);
    camera.updateMatrixWorld();
    camera.zoom = 1;
    let halfHeight = 1;
    for (const x of [bounds.min.x, bounds.max.x])
      for (const y of [bounds.min.y, bounds.max.y])
        for (const z of [bounds.min.z, bounds.max.z]) {
          const corner = new THREE.Vector3(x, y, z).applyMatrix4(
            camera.matrixWorldInverse,
          );
          halfHeight = Math.max(
            halfHeight,
            Math.abs(corner.y),
            Math.abs(corner.x) / aspect,
          );
        }
    halfHeight *= 1.15;
    camera.left = -halfHeight * aspect;
    camera.right = halfHeight * aspect;
    camera.top = halfHeight;
    camera.bottom = -halfHeight;
    camera.updateProjectionMatrix();
    controls.target.copy(center);
    controls.update();
  }
  function clearLinks() {
    if (links) {
      scene.remove(links);
      links.geometry.dispose();
      (links.material as THREE.Material).dispose();
      links = undefined;
    }
  }
  function update() {
    tooltip.hidden = true;
    const algorithm = (el("algorithm") as HTMLSelectElement).value;
    clearLinks();
    scene.remove(city.group);
    city.dispose();
    city = createCityScene(analysis, algorithm, data.layouts?.[algorithm]);
    scene.add(city.group);
    positions.clear();
    city.nodeByInstance.forEach((id, i) => {
      city.mesh.getMatrixAt(i, matrix);
      positions.set(id, new THREE.Vector3().setFromMatrixPosition(matrix));
    });
    const p = analysis.partitions.find((p) => p.algorithm === algorithm)!;
    el("metrics").replaceChildren();
    for (const [label, value] of [
      ["Symboles", analysis.graph.nodes.length.toLocaleString("fr")],
      ["Groupes", p.communities.length.toLocaleString("fr")],
      ["Sans lien observé", p.metrics.isolatedNodes.toLocaleString("fr")],
      ["Modularité", p.metrics.modularity.toFixed(3)],
      [
        "Liens internes",
        p.metrics.internalWeightRatio === null
          ? "—"
          : `${Math.round(p.metrics.internalWeightRatio * 100)} %`,
      ],
    ]) {
      const row = document.createElement("div");
      row.className = "metric";
      const name = document.createElement("span"),
        number = document.createElement("strong");
      name.textContent = String(label);
      number.textContent = String(value);
      row.append(name, number);
      el("metrics").append(row);
    }
    el("selected").textContent = "Sélectionner un bâtiment";
    el("detail").textContent =
      "Chaque bâtiment représente un symbole de l’index.";
    el("relations").textContent = "";
    el("roads").textContent =
      `${city.roadsShown} / ${city.layout.roads.length} artères · ${city.layout.streets.filter((s) => s.points.length > 1).length} / ${city.layout.localStreetCandidates} voies locales · ${city.layout.unrouted} tracés non résolus. Les croisements ne créent pas de dépendance. Zone à part : ${city.layout.isolated.length} symboles sans lien observé.`;
    fit();
  }
  const ray = new THREE.Raycaster(),
    pointer = new THREE.Vector2();
  let downX = 0,
    downY = 0;
  let hoverFrame = 0;
  function hideTooltip() {
    if (hoverFrame) cancelAnimationFrame(hoverFrame);
    hoverFrame = 0;
    tooltip.hidden = true;
    renderer.domElement.style.cursor = "grab";
  }
  renderer.domElement.addEventListener("pointerleave", hideTooltip);
  controls.addEventListener("start", hideTooltip);
  window.addEventListener("keydown", (e) => {
    if (e.key === "Escape") hideTooltip();
  });
  function showHovered(e: PointerEvent) {
    if (hoverFrame) cancelAnimationFrame(hoverFrame);
    if (e.buttons) {
      hideTooltip();
      return;
    }
    const clientX = e.clientX,
      clientY = e.clientY;
    hoverFrame = requestAnimationFrame(() => {
      hoverFrame = 0;
      const rect = renderer.domElement.getBoundingClientRect();
      pointer.set(
        ((clientX - rect.left) / rect.width) * 2 - 1,
        (-(clientY - rect.top) / rect.height) * 2 + 1,
      );
      ray.setFromCamera(pointer, camera);
      const hit = ray.intersectObjects(city.pickable)[0];
      if (hit?.instanceId === undefined) {
        hideTooltip();
        return;
      }
      const id = city.nodeByInstance[hit.instanceId]!,
        node = nodeById.get(id)!;
      tooltipKind.textContent = node.kind ?? "Symbole";
      tooltipTitle.textContent = node.label;
      tooltipStats.textContent = `${neighborhood.get(id)?.size ?? 0} voisins · ${incomingCounts.get(id) ?? 0} références entrantes`;
      tooltipFile.textContent = node.file ?? "";
      tooltip.hidden = false;
      const width = tooltip.offsetWidth,
        height = tooltip.offsetHeight;
      tooltip.style.left = `${Math.max(8, Math.min(clientX - rect.left + 18, rect.width - width - 8))}px`;
      tooltip.style.top = `${Math.max(8, Math.min(clientY - rect.top + 18, rect.height - height - 8))}px`;
      renderer.domElement.style.cursor = "pointer";
    });
  }
  renderer.domElement.addEventListener("pointermove", showHovered);
  renderer.domElement.addEventListener("pointerup", showHovered);
  renderer.domElement.addEventListener("pointerdown", (e) => {
    downX = e.clientX;
    downY = e.clientY;
  });
  renderer.domElement.addEventListener("click", (e) => {
    if (Math.hypot(e.clientX - downX, e.clientY - downY) > 5) return;
    const rect = renderer.domElement.getBoundingClientRect();
    pointer.set(
      ((e.clientX - rect.left) / rect.width) * 2 - 1,
      (-(e.clientY - rect.top) / rect.height) * 2 + 1,
    );
    ray.setFromCamera(pointer, camera);
    const hit = ray.intersectObjects(city.pickable)[0];
    if (hit?.instanceId === undefined) return;
    const id = city.nodeByInstance[hit.instanceId]!,
      node = analysis.graph.nodes.find((n) => n.id === id)!;
    const isolated = new Set(city.layout.isolated);
    city.nodeByInstance.forEach((nodeId, i) =>
      city.mesh.setColorAt(
        i,
        new THREE.Color(
          nodeId === id
            ? "#d8b774"
            : isolated.has(nodeId)
              ? "#52625e"
              : "#a1b5b0",
        ),
      ),
    );
    city.mesh.instanceColor!.needsUpdate = true;
    const related = analysis.graph.edges.filter(
      (edge) => edge.source === id || edge.target === id,
    );
    el("selected").textContent = node.label;
    const building = city.layout.buildings.find((b) => b.id === id)!;
    el("detail").textContent =
      `${node.kind ?? "symbole"} · ${node.file ?? ""}${building.gateway ? " · Communication interquartiers" : ""}`;
    const aggregate = new Map<string, number>();
    for (const edge of related) {
      const other = edge.source === id ? edge.target : edge.source;
      aggregate.set(other, (aggregate.get(other) ?? 0) + edge.weight);
    }
    el("relations").replaceChildren();
    const summary = document.createElement("p");
    summary.textContent = `${aggregate.size} voisins · ${related.length} références · ${Math.min(aggregate.size, 200)} relations affichées`;
    el("relations").append(summary);
    clearLinks();
    const points: THREE.Vector3[] = [],
      neighbors = [...aggregate].sort((a, b) => b[1] - a[1]);
    for (const [other, weight] of neighbors.slice(0, 200)) {
      if (positions.has(other))
        points.push(positions.get(id)!, positions.get(other)!);
    }
    links = new THREE.LineSegments(
      new THREE.BufferGeometry().setFromPoints(points),
      new THREE.LineBasicMaterial({
        color: 0xc9ad74,
        transparent: true,
        opacity: 0.65,
      }),
    );
    scene.add(links);
    for (const [other, weight] of neighbors.slice(0, 12)) {
      const row = document.createElement("div");
      row.textContent = `${analysis.graph.nodes.find((n) => n.id === other)?.label ?? other} · ${weight}`;
      el("relations").append(row);
    }
  });
  el("scope").textContent =
    (data.provenance.path.length
      ? data.provenance.path.join(", ")
      : "Projet complet") +
    " · génération " +
    data.provenance.generation;
  el("coverage").textContent =
    `Relations résolues dans le périmètre capturé. ${data.provenance.outsideScope} références entrantes hors périmètre, ${data.provenance.unmappedSources} sources ambiguës. Les références externes et non résolues ne sont pas représentées. Un bâtiment isolé n’est pas nécessairement du code inutilisé.`;
  el("algorithm").addEventListener("change", update);
  el("reset").addEventListener("click", fit);
  new ResizeObserver(() => {
    renderer.setSize(container.clientWidth, container.clientHeight);
    fit();
  }).observe(container);
  update();
  renderer.setAnimationLoop(() => {
    controls.update();
    renderer.render(scene, camera);
  });
}
main().catch((error) => {
  el("error").hidden = false;
  el("error").textContent = error.message;
});
