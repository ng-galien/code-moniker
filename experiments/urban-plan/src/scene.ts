/** Renderer-free urban-plan IR. Three.js must not live here. */

import type { LayoutConfig } from "./config.ts";

export type MetricBinding = "defs" | "none";

export type RoadClass = "avenue" | "street" | "lane";

export type SceneCity = {
	width: number;
	depth: number;
};

export type SceneDistrict = {
	id: string;
	label: string;
	kind: string;
	level: number;
	parentId: string | null;
	crateId: string;
	defs: number;
	x: number;
	y: number;
	z: number;
	width: number;
	depth: number;
	thickness: number;
	color: string;
};

export type SceneBuilding = {
	id: string;
	label: string;
	kind: string;
	districtId: string;
	crateId: string;
	defs: number;
	x: number;
	y: number;
	z: number;
	width: number;
	depth: number;
	height: number;
};

export type SceneStreet = {
	id: string;
	clazz: RoadClass;
	x: number;
	y: number;
	z: number;
	width: number;
	depth: number;
};

export type ScenePoint = {
	x: number;
	z: number;
};

export type SceneRoad = {
	from: string;
	to: string;
	fromLabel: string;
	toLabel: string;
	count: number;
	kinds: string[];
	clazz: RoadClass;
	points: ScenePoint[];
};

export type SceneCoverage = {
	nodes: number;
	districts: number;
	edgesTotal: number;
	edgesEmitted: number;
	roadsOmitted: number;
};

export type IdentityEdge = {
	source: string;
	target: string;
	kinds: string[];
	count: number;
};

export type IdentityTree = {
	id: string;
	name: string;
	kind: string;
	defs: number;
	children: IdentityTree[];
};

export type SceneSnapshot = {
	kind: "urban-plan";
	version: 3;
	generation: number | null;
	prefix: string;
	capturedAt: string;
	heightMetric: MetricBinding;
	footprintMetric: MetricBinding;
	city: SceneCity;
	config: LayoutConfig;
	tree: IdentityTree;
	edges: IdentityEdge[];
	districts: SceneDistrict[];
	buildings: SceneBuilding[];
	streets: SceneStreet[];
	roads: SceneRoad[];
	coverage: SceneCoverage;
};
