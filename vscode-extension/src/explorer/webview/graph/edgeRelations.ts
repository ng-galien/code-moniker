import type { CockpitFilters, CockpitRelation } from "../../protocol";

export interface ActiveEdgeRelations {
	relation: CockpitRelation;
	relations: CockpitRelation[];
	label: string;
}

export function selectActiveEdgeRelations(
	relations: CockpitRelation[],
	labels: Partial<Record<CockpitRelation, string>>,
	filters: CockpitFilters,
): ActiveEdgeRelations | undefined {
	const active = relations.filter((relation) => filters[relation]);
	if (active.length === 0) return undefined;
	return {
		relation: active[0],
		relations: active,
		label: active.map((relation) => labels[relation] ?? relation).join(" · "),
	};
}
