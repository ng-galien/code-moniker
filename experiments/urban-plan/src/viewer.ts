import * as THREE from "three";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";

import {
	buildingPosition,
	type SceneBuilding,
	type SceneSnapshot,
} from "./scene.ts";

const canvas = document.querySelector("canvas")!;
const meta = document.querySelector("#meta")!;
const pick = document.querySelector("#pick")!;

const renderer = new THREE.WebGLRenderer({ canvas, antialias: true });
renderer.setPixelRatio(Math.min(window.devicePixelRatio, 2));
renderer.setClearColor(0x0e141b, 1);

const scene = new THREE.Scene();
scene.fog = new THREE.Fog(0x0e141b, 28, 80);

const camera = new THREE.OrthographicCamera(-18, 18, 12, -12, 0.1, 200);
camera.position.set(16, 18, 16);
camera.lookAt(0, 0, 0);

const controls = new OrbitControls(camera, canvas);
controls.enableDamping = true;
controls.maxPolarAngle = Math.PI / 2.15;
controls.minPolarAngle = Math.PI / 6;
controls.target.set(4, 0, 4);

scene.add(new THREE.AmbientLight(0xb9c8d6, 0.7));
const sun = new THREE.DirectionalLight(0xfff1d6, 1.05);
sun.position.set(8, 18, 6);
scene.add(sun);

const ground = new THREE.Mesh(
	new THREE.PlaneGeometry(80, 80),
	new THREE.MeshStandardMaterial({ color: 0x1a2733, roughness: 1 }),
);
ground.rotation.x = -Math.PI / 2;
ground.position.y = -0.02;
scene.add(ground);

const KIND_COLOR: Record<string, number> = {
	dir: 0x6ea8d9,
	module: 0xc4a35a,
	crate: 0x6ea8d9,
};

const raycaster = new THREE.Raycaster();
const pointer = new THREE.Vector2();
const pickables: THREE.Object3D[] = [];
let snapshot: SceneSnapshot | null = null;

function colorFor(kind: string): number {
	return KIND_COLOR[kind] ?? 0x8aa4b8;
}

function rebuild(data: SceneSnapshot) {
	for (const object of pickables.splice(0)) {
		scene.remove(object);
	}
	snapshot = data;
	const group = new THREE.Group();
	const maxCount = Math.max(1, ...data.roads.map((road) => road.count));
	const byId = new Map(data.buildings.map((building) => [building.id, building]));

	for (const building of data.buildings) {
		const mesh = new THREE.Mesh(
			new THREE.BoxGeometry(building.width, building.height, building.depth),
			new THREE.MeshStandardMaterial({
				color: colorFor(building.kind),
				roughness: 0.45,
				metalness: 0.08,
			}),
		);
		const { x, z } = buildingPosition(building);
		mesh.position.set(x, building.height / 2, z);
		mesh.userData.building = building;
		group.add(mesh);
		pickables.push(mesh);
	}

	for (const road of data.roads) {
		const from = byId.get(road.from);
		const to = byId.get(road.to);
		if (!from || !to) {
			continue;
		}
		const a = buildingPosition(from);
		const b = buildingPosition(to);
		const start = new THREE.Vector3(a.x, 0.04, a.z);
		const end = new THREE.Vector3(b.x, 0.04, b.z);
		const geometry = new THREE.BufferGeometry().setFromPoints([start, end]);
		const line = new THREE.Line(
			geometry,
			new THREE.LineBasicMaterial({
				color: 0xd7c4a3,
				transparent: true,
				opacity: 0.35 + (road.count / maxCount) * 0.5,
			}),
		);
		line.userData.road = road;
		group.add(line);
	}

	const centroid = data.buildings.reduce(
		(acc, building) => {
			const pos = buildingPosition(building);
			acc.x += pos.x;
			acc.z += pos.z;
			return acc;
		},
		{ x: 0, z: 0 },
	);
	if (data.buildings.length > 0) {
		centroid.x /= data.buildings.length;
		centroid.z /= data.buildings.length;
		controls.target.set(centroid.x, 0.4, centroid.z);
		camera.position.set(centroid.x + 14, 16, centroid.z + 14);
	}
	scene.add(group);
	pickables.push(group);
	meta.textContent = [
		`prefix ${data.prefix}`,
		`generation ${data.generation ?? "fixture"}`,
		`${data.buildings.length} buildings`,
		`${data.roads.length} roads`,
		data.coverage.roadsOmitted ? `${data.coverage.roadsOmitted} roads omitted` : "",
	]
		.filter(Boolean)
		.join(" · ");
}

function describeBuilding(building: SceneBuilding): string {
	return `${building.label}  ${building.kind}  defs=${building.defs}  ${building.id}`;
}

canvas.addEventListener("pointermove", (event) => {
	const rect = canvas.getBoundingClientRect();
	pointer.x = ((event.clientX - rect.left) / rect.width) * 2 - 1;
	pointer.y = -((event.clientY - rect.top) / rect.height) * 2 + 1;
	raycaster.setFromCamera(pointer, camera);
	const hit = raycaster.intersectObjects(pickables, true).find((item) => {
		return Boolean(item.object.userData.building);
	});
	if (!hit) {
		pick.textContent = snapshot
			? "hover a building · drag to orbit · scroll to zoom"
			: "";
		return;
	}
	pick.textContent = describeBuilding(hit.object.userData.building);
});

function resize() {
	const width = canvas.clientWidth;
	const height = canvas.clientHeight;
	renderer.setSize(width, height, false);
	const aspect = width / Math.max(height, 1);
	const frustum = 14;
	camera.left = -frustum * aspect;
	camera.right = frustum * aspect;
	camera.top = frustum;
	camera.bottom = -frustum;
	camera.updateProjectionMatrix();
}

window.addEventListener("resize", resize);
resize();

function frame() {
	controls.update();
	renderer.render(scene, camera);
	requestAnimationFrame(frame);
}
frame();

const data = (await fetch("/snapshot.json").then((response) =>
	response.json(),
)) as SceneSnapshot;
rebuild(data);
