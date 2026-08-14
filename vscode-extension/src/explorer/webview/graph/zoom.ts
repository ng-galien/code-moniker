export type CockpitZoomLevel = "detail" | "summary" | "glyph";

export function cockpitZoomLevel(zoom: number): CockpitZoomLevel {
	if (zoom < 0.32) return "glyph";
	if (zoom < 0.7) return "summary";
	return "detail";
}

export function showCockpitEdgeLabel(zoom: number): boolean {
	return zoom >= 0.5;
}
