import { useLayoutEffect, useMemo, useRef } from "react";
import { Canvas, useThree } from "@react-three/fiber";
import { Html, MapControls, OrthographicCamera } from "@react-three/drei";
import { Quaternion, Vector3, type OrthographicCamera as OrthoCam } from "three";
import type { MapControls as MapControlsImpl } from "three-stdlib";

import type {
	SceneBuilding,
	SceneDistrict,
	SceneRoad,
	SceneSnapshot,
	SceneStreet,
} from "./scene.ts";

export type CityPick =
	| { kind: "building"; building: SceneBuilding }
	| { kind: "district"; district: SceneDistrict }
	| { kind: "road"; road: SceneRoad }
	| null;

type CityProps = {
	snapshot: SceneSnapshot;
	picked: CityPick;
	pinned: CityPick;
	focus: SceneDistrict | null;
	onPick: (pick: CityPick) => void;
	onPin: (pick: CityPick) => void;
	onFocus: (district: SceneDistrict | null) => void;
};

const KIND_COLOR: Record<string, string> = {
	struct: "#e0c28a",
	enum: "#d4894a",
	trait: "#8eabce",
	type: "#c4b09a",
	union: "#c9a06a",
	impl: "#9aa7b5",
	module: "#d2bc7a",
	dir: "#7aa4c4",
	class: "#e0c28a",
	interface: "#8eabce",
};

const STREET_COLOR: Record<SceneStreet["clazz"], string> = {
	avenue: "#4a5564",
	street: "#414b59",
	lane: "#3b4450",
};

export function City({ snapshot, picked, pinned, focus, onPick, onPin, onFocus }: CityProps) {
	const look = useMemo(() => frameOf(snapshot, focus), [snapshot, focus]);
	const gesture = useRef({ x: 0, y: 0, dragging: false });

	return (
		<Canvas
			gl={{ antialias: true }}
			onPointerDown={(event) => {
				gesture.current = { x: event.clientX, y: event.clientY, dragging: false };
			}}
			onPointerMove={(event) => {
				if (Math.hypot(event.clientX - gesture.current.x, event.clientY - gesture.current.y) > 6) {
					gesture.current.dragging = true;
				}
			}}
			onPointerMissed={() => {
				if (!gesture.current.dragging) {
					onPick(null);
					onPin(null);
				}
			}}
			style={{ width: "100%", height: "100%" }}
			dpr={[1, 1.75]}
		>
			<color attach="background" args={["#243140"]} />
			<OrthographicCamera makeDefault near={0.1} far={8000} zoom={8} />
			<hemisphereLight args={["#d7e4ef", "#2a241c", 0.95]} />
			<directionalLight position={[40, 70, 28]} intensity={1.2} color="#fff6e4" />
			<Board snapshot={snapshot} look={look} />
			{snapshot.streets
				.filter((street) => street.clazz === "avenue")
				.map((street) => (
					<StreetMesh key={street.id} street={street} />
				))}
			{snapshot.districts.map((district) => (
				<DistrictMesh
					key={district.id}
					district={district}
					active={picked?.kind === "district" && picked.district.id === district.id}
					showLabel={
						(snapshot.config?.showLabels ?? true) &&
						pinned?.kind !== "road" &&
						shouldLabel(district, focus)
					}
					onPick={onPick}
					onFocus={onFocus}
					gesture={gesture}
				/>
			))}
			{snapshot.buildings.map((building) => (
				<BuildingMesh
					key={building.id}
					building={building}
					active={picked?.kind === "building" && picked.building.id === building.id}
					onPick={onPick}
				/>
			))}
			<Avenues snapshot={snapshot} picked={picked} pinned={pinned} onPick={onPick} onPin={onPin} />
			<CameraRig look={look} />
		</Canvas>
	);
}

type Look = {
	x: number;
	z: number;
	span: number;
	cityX: number;
	cityZ: number;
	width: number;
	depth: number;
};

function Board({ snapshot, look }: { snapshot: SceneSnapshot; look: Look }) {
	const w = snapshot.city.width;
	const d = snapshot.city.depth;
	return (
		<group>
			<mesh position={[look.cityX, -0.16, look.cityZ]}>
				<boxGeometry args={[w + 8, 0.2, d + 8]} />
				<meshStandardMaterial color="#3d4c5c" roughness={0.88} />
			</mesh>
			<mesh position={[look.cityX, -0.03, look.cityZ]}>
				<boxGeometry args={[w + 3.2, 0.1, d + 3.2]} />
				<meshStandardMaterial color="#4a5564" roughness={0.84} />
			</mesh>
		</group>
	);
}

function StreetMesh({ street }: { street: SceneStreet }) {
	return (
		<mesh position={[street.x, 0.02, street.z]} rotation={[-Math.PI / 2, 0, 0]}>
			<planeGeometry args={[street.width, street.depth]} />
			<meshStandardMaterial color={STREET_COLOR[street.clazz]} roughness={0.86} metalness={0.05} />
		</mesh>
	);
}

function DistrictMesh({
	district,
	active,
	showLabel,
	onPick,
	onFocus,
	gesture,
}: {
	district: SceneDistrict;
	active: boolean;
	showLabel: boolean;
	onPick: (pick: CityPick) => void;
	onFocus: (district: SceneDistrict) => void;
	gesture: { current: { dragging: boolean } };
}) {
	return (
		<group>
			<mesh position={[district.x, district.y - 0.02, district.z]}>
				<boxGeometry args={[district.width + 0.28, district.thickness, district.depth + 0.28]} />
				<meshStandardMaterial color="#24303c" roughness={0.88} />
			</mesh>
			<mesh
				position={[district.x, district.y, district.z]}
				onPointerOver={(event) => {
					event.stopPropagation();
					onPick({ kind: "district", district });
				}}
				onPointerOut={() => onPick(null)}
				onClick={(event) => {
					event.stopPropagation();
					if (!gesture.current.dragging) {
						onFocus(district);
					}
				}}
			>
				<boxGeometry args={[district.width, district.thickness, district.depth]} />
				<meshStandardMaterial
					color={active ? "#d5c4a8" : district.color}
					roughness={0.82}
					metalness={0.04}
				/>
			</mesh>
			{showLabel ? (
				<Html
					position={[district.x, district.y + district.thickness / 2 + 0.35, district.z]}
					center
					sprite
					pointerEvents="none"
					style={{ pointerEvents: "none" }}
				>
					<div className={district.level === 1 ? "label crate" : "label district"}>
						{district.label}
					</div>
				</Html>
			) : null}
		</group>
	);
}

function BuildingMesh({
	building,
	active,
	onPick,
}: {
	building: SceneBuilding;
	active: boolean;
	onPick: (pick: CityPick) => void;
}) {
	return (
		<mesh
			position={[building.x, building.y, building.z]}
			onPointerOver={(event) => {
				event.stopPropagation();
				onPick({ kind: "building", building });
			}}
			onPointerOut={() => onPick(null)}
		>
			<boxGeometry args={[building.width, building.height, building.depth]} />
			<meshStandardMaterial
				color={active ? "#f3efe6" : (KIND_COLOR[building.kind] ?? "#c4b49a")}
				roughness={0.42}
				metalness={0.08}
			/>
		</mesh>
	);
}

function Avenues({
	snapshot,
	picked,
	pinned,
	onPick,
	onPin,
}: {
	snapshot: SceneSnapshot;
	picked: CityPick;
	pinned: CityPick;
	onPick: (pick: CityPick) => void;
	onPin: (pick: CityPick) => void;
}) {
	const maxCount = Math.max(1, ...snapshot.roads.map((road) => road.count));
	const selected =
		pinned?.kind === "road" ? pinned.road : picked?.kind === "road" ? picked.road : null;
	const isolating = selected !== null;
	return (
		<>
			{snapshot.roads.map((road) => {
				const active = selected !== null && selected.from === road.from && selected.to === road.to;
				return (
					<OrthoRoad
						key={`${road.from}->${road.to}`}
						road={road}
						width={(0.48 + (road.count / maxCount) * 0.7) * (active ? 1.55 : 1)}
						y={active ? 0.42 : 0.16}
						active={active}
						dimmed={isolating && !active}
						onPick={onPick}
						onPin={onPin}
					/>
				);
			})}
		</>
	);
}

function OrthoRoad({
	road,
	width,
	y,
	active,
	dimmed,
	onPick,
	onPin,
}: {
	road: SceneRoad;
	width: number;
	y: number;
	active: boolean;
	dimmed: boolean;
	onPick: (pick: CityPick) => void;
	onPin: (pick: CityPick) => void;
}) {
	const points = road.points ?? [];
	if (points.length < 2) {
		return null;
	}
	const tint = roadTint(road.from, road.to, active);
	const start = points[0]!;
	const end = points[points.length - 1]!;
	return (
		<group
			onPointerOver={(event) => {
				event.stopPropagation();
				onPick({ kind: "road", road });
			}}
			onPointerOut={() => onPick(null)}
			onClick={(event) => {
				event.stopPropagation();
				onPin({ kind: "road", road });
			}}
		>
			{points.slice(0, -1).map((point, index) => (
				<RoadSlab
					key={`${road.from}-${index}`}
					a={point}
					b={points[index + 1]!}
					width={width}
					y={y}
					color={tint.color}
					emissive={tint.emissive}
					opacity={dimmed ? 0.12 : 1}
				/>
			))}
			{points.map((point, index) => (
				<mesh key={`${road.from}-joint-${index}`} position={[point.x, y, point.z]}>
					<boxGeometry args={[width, 0.18, width]} />
					<meshStandardMaterial
						color={tint.color}
						emissive={tint.emissive}
						roughness={0.38}
						metalness={0.1}
						transparent={dimmed}
						opacity={dimmed ? 0.12 : 1}
					/>
				</mesh>
			))}
			{active ? (
				<>
					<Html position={[start.x, y + 0.7, start.z]} center sprite pointerEvents="none">
						<div className="label port">de {road.fromLabel}</div>
					</Html>
					<Html position={[end.x, y + 0.7, end.z]} center sprite pointerEvents="none">
						<div className="label port">vers {road.toLabel}</div>
					</Html>
				</>
			) : null}
		</group>
	);
}

function roadTint(from: string, to: string, active: boolean): { color: string; emissive: string } {
	let h = 0;
	const key = `${from}->${to}`;
	for (let i = 0; i < key.length; i += 1) {
		h = (h * 33 + key.charCodeAt(i)) | 0;
	}
	const hue = Math.abs(h) % 360;
	if (active) {
		return { color: `hsl(${hue}, 85%, 72%)`, emissive: `hsl(${hue}, 90%, 28%)` };
	}
	return { color: `hsl(${hue}, 70%, 58%)`, emissive: `hsl(${hue}, 75%, 18%)` };
}

function RoadSlab({
	a,
	b,
	width,
	y,
	color,
	emissive,
	opacity,
}: {
	a: { x: number; z: number };
	b: { x: number; z: number };
	width: number;
	y: number;
	color: string;
	emissive: string;
	opacity: number;
}) {
	const layout = useMemo(() => {
		const start = new Vector3(a.x, y, a.z);
		const end = new Vector3(b.x, y, b.z);
		const direction = end.clone().sub(start);
		const length = direction.length();
		if (length < 0.05) {
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
	}, [a, b, y]);
	if (!layout) {
		return null;
	}
	return (
		<mesh position={layout.position} quaternion={layout.quaternion}>
			<boxGeometry args={[width, 0.18, layout.length]} />
			<meshStandardMaterial
				color={color}
				emissive={emissive}
				roughness={0.38}
				metalness={0.1}
				transparent={opacity < 1}
				opacity={opacity}
			/>
		</mesh>
	);
}



function CameraRig({ look }: { look: Look }) {
	const { size, camera } = useThree();
	const controls = useRef<MapControlsImpl>(null);
	const key = `${look.x.toFixed(2)}:${look.z.toFixed(2)}:${look.width.toFixed(2)}:${look.depth.toFixed(2)}`;
	useLayoutEffect(() => {
		const ortho = camera as OrthoCam;
		ortho.clearViewOffset();
		const iso = 1.85;
		const fitW = look.width + 12;
		const fitD = look.depth + 12;
		ortho.zoom = Math.max(2.2, Math.min(size.width / (fitW * iso), size.height / (fitD * iso)));
		ortho.near = 0.1;
		ortho.far = 8000;
		ortho.position.set(look.x + look.span * 0.55, look.span * 0.72, look.z + look.span * 0.55);
		ortho.updateProjectionMatrix();
		controls.current?.target.set(look.x, 0.4, look.z);
		controls.current?.update();
	}, [camera, key, look.depth, look.span, look.width, look.x, look.z, size.height, size.width]);
	return (
		<MapControls
			ref={controls}
			makeDefault
			enableDamping
			enablePan
			enableRotate
			enableZoom
			dampingFactor={0.14}
			screenSpacePanning
			minZoom={1.6}
			maxZoom={180}
			maxPolarAngle={Math.PI / 2.35}
			minPolarAngle={Math.PI / 10}
		/>
	);
}

function frameOf(snapshot: SceneSnapshot, focus: SceneDistrict | null): Look {
	const cityX = snapshot.city.width / 2;
	const cityZ = snapshot.city.depth / 2;
	if (!focus) {
		return {
			x: cityX,
			z: cityZ,
			span: snapshot.city.width,
			cityX,
			cityZ,
			width: snapshot.city.width,
			depth: snapshot.city.depth,
		};
	}
	return {
		x: focus.x,
		z: focus.z,
		span: Math.max(focus.width, focus.depth),
		cityX,
		cityZ,
		width: focus.width,
		depth: focus.depth,
	};
}

function shouldLabel(district: SceneDistrict, focus: SceneDistrict | null): boolean {
	if (district.level === 1 && !focus) {
		return true;
	}
	if (!focus) {
		return false;
	}
	if (district.id === focus.id) {
		return true;
	}
	if (!isInside(focus.id, district.id) && !isInside(district.id, focus.id)) {
		return false;
	}
	if (district.level > focus.level + 2) {
		return false;
	}
	return district.width > 6 || district.level <= focus.level + 1;
}

function isInside(parentId: string, childId: string): boolean {
	return childId === parentId || childId.startsWith(`${parentId}/`);
}
