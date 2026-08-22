import { useMemo } from "react";
import { Canvas } from "@react-three/fiber";
import { Html, OrbitControls, OrthographicCamera } from "@react-three/drei";
import { Quaternion, Vector3 } from "three";

import {
	buildingPosition,
	type SceneBuilding,
	type SceneRoad,
	type SceneSnapshot,
} from "./scene.ts";

type CityProps = {
	snapshot: SceneSnapshot;
	picked: SceneBuilding | null;
	onPick: (building: SceneBuilding | null) => void;
};

const KIND_COLOR: Record<string, string> = {
	dir: "#6ea8d9",
	module: "#c4a35a",
};

export function City({ snapshot, picked, onPick }: CityProps) {
	const lookAt = useMemo(() => centroid(snapshot.buildings), [snapshot]);
	const maxCount = Math.max(1, ...snapshot.roads.map((road) => road.count));
	const byId = useMemo(
		() => new Map(snapshot.buildings.map((building) => [building.id, building])),
		[snapshot],
	);

	return (
		<Canvas
			gl={{ antialias: true }}
			onPointerMissed={() => onPick(null)}
			style={{ width: "100%", height: "100%" }}
		>
			<color attach="background" args={["#0e141b"]} />
			<fog attach="fog" args={["#0e141b", 28, 80]} />
			<OrthographicCamera
				makeDefault
				position={[lookAt[0] + 16, 18, lookAt[2] + 16]}
				zoom={22}
				near={0.1}
				far={200}
			/>
			<ambientLight intensity={0.7} color="#b9c8d6" />
			<directionalLight position={[8, 18, 6]} intensity={1.05} color="#fff1d6" />
			<mesh rotation={[-Math.PI / 2, 0, 0]} position={[0, -0.02, 0]}>
				<planeGeometry args={[80, 80]} />
				<meshStandardMaterial color="#1a2733" roughness={1} />
			</mesh>
			{snapshot.buildings.map((building) => (
				<BuildingMesh
					key={building.id}
					building={building}
					active={picked?.id === building.id}
					onPick={onPick}
				/>
			))}
			{snapshot.roads.map((road) => {
				const from = byId.get(road.from);
				const to = byId.get(road.to);
				if (!from || !to) {
					return null;
				}
				return (
					<RoadMesh
						key={`${road.from}->${road.to}`}
						from={from}
						to={to}
						road={road}
						maxCount={maxCount}
					/>
				);
			})}
			<OrbitControls
				makeDefault
				enableDamping
				target={lookAt}
				maxPolarAngle={Math.PI / 2.15}
				minPolarAngle={Math.PI / 6}
			/>
		</Canvas>
	);
}

function BuildingMesh({
	building,
	active,
	onPick,
}: {
	building: SceneBuilding;
	active: boolean;
	onPick: (building: SceneBuilding | null) => void;
}) {
	const { x, z } = buildingPosition(building);
	return (
		<group>
			<mesh
				position={[x, building.height / 2, z]}
				onPointerOver={(event) => {
					event.stopPropagation();
					onPick(building);
				}}
				onPointerOut={() => onPick(null)}
			>
				<boxGeometry args={[building.width, building.height, building.depth]} />
				<meshStandardMaterial
					color={active ? "#d7e8f6" : (KIND_COLOR[building.kind] ?? "#8aa4b8")}
					roughness={0.45}
					metalness={0.08}
				/>
			</mesh>
			<Html position={[x, building.height + 0.45, z]} center sprite>
				<div className="label">{building.label}</div>
			</Html>
		</group>
	);
}

function RoadMesh({
	from,
	to,
	road,
	maxCount,
}: {
	from: SceneBuilding;
	to: SceneBuilding;
	road: SceneRoad;
	maxCount: number;
}) {
	const layout = useMemo(() => {
		const a = buildingPosition(from);
		const b = buildingPosition(to);
		const start = new Vector3(a.x, 0.08, a.z);
		const end = new Vector3(b.x, 0.08, b.z);
		const direction = end.clone().sub(start);
		const length = direction.length();
		if (length < 0.01) {
			return null;
		}
		const quaternion = new Quaternion().setFromUnitVectors(
			new Vector3(0, 0, 1),
			direction.normalize(),
		);
		return {
			position: start.lerp(end, 0.5).toArray() as [number, number, number],
			quaternion: [quaternion.x, quaternion.y, quaternion.z, quaternion.w] as [
				number,
				number,
				number,
				number,
			],
			length,
		};
	}, [from, to]);
	if (!layout) {
		return null;
	}
	return (
		<mesh position={layout.position} quaternion={layout.quaternion}>
			<boxGeometry args={[0.1 + (road.count / maxCount) * 0.32, 0.07, layout.length]} />
			<meshStandardMaterial color="#e2c48a" emissive="#3a2e16" roughness={0.7} />
		</mesh>
	);
}

function centroid(buildings: SceneBuilding[]): [number, number, number] {
	if (buildings.length === 0) {
		return [4, 0.4, 4];
	}
	const sum = buildings.reduce(
		(acc, building) => {
			const pos = buildingPosition(building);
			acc.x += pos.x;
			acc.z += pos.z;
			return acc;
		},
		{ x: 0, z: 0 },
	);
	return [sum.x / buildings.length, 0.4, sum.z / buildings.length];
}
