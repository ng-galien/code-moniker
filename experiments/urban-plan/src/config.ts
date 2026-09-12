export type HeightMode = "log" | "linear";

export type LayoutConfig = {
	citySize: number;
	maxDepth: number;
	minDefs: number;
	heightMode: HeightMode;
	heightScale: number;
	maxHeight: number;
	footprintScale: number;
	avenueGap: number;
	streetGap: number;
	laneGap: number;
	minCrateSide: number;
	minDistrictSide: number;
	includeSrc: boolean;
	includeTests: boolean;
	includeBenches: boolean;
	includeExamples: boolean;
	hiddenIds: string[];
	maxRoads: number;
	minRoadCount: number;
	showAvenues: boolean;
	showStreets: boolean;
	showDistricts: boolean;
	showLabels: boolean;
	buildingKinds: string[];
};

export const BUILDING_KIND_OPTIONS = [
	"struct",
	"enum",
	"trait",
	"type",
	"impl",
	"module",
	"dir",
] as const;

export const DEFAULT_LAYOUT: LayoutConfig = {
	citySize: 110,
	maxDepth: 4,
	minDefs: 1,
	heightMode: "log",
	heightScale: 0.5,
	maxHeight: 8,
	footprintScale: 1,
	avenueGap: 4,
	streetGap: 1.35,
	laneGap: 0.55,
	minCrateSide: 10,
	minDistrictSide: 1.5,
	includeSrc: true,
	includeTests: true,
	includeBenches: true,
	includeExamples: true,
	hiddenIds: [],
	maxRoads: 16,
	minRoadCount: 8,
	showAvenues: true,
	showStreets: true,
	showDistricts: true,
	showLabels: true,
	buildingKinds: [...BUILDING_KIND_OPTIONS],
};

const STORAGE_KEY = "urban-plan-layout";

export function loadLayoutConfig(): LayoutConfig {
	try {
		const raw = localStorage.getItem(STORAGE_KEY);
		if (!raw) {
			return DEFAULT_LAYOUT;
		}
		const parsed = JSON.parse(raw) as Partial<LayoutConfig>;
		return { ...DEFAULT_LAYOUT, ...parsed, hiddenIds: parsed.hiddenIds ?? [] };
	} catch {
		return DEFAULT_LAYOUT;
	}
}

export function saveLayoutConfig(config: LayoutConfig): void {
	localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
}
