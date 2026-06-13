import { Violation } from "./model";

export interface VisibleViolationGroup {
	violation: Violation;
	count: number;
	kinds: string[];
}

export function groupVisibleViolations(
	violations: readonly Violation[],
): VisibleViolationGroup[] {
	const groups = new Map<string, VisibleViolationGroup>();
	for (const violation of violations) {
		const key = visibleViolationKey(violation);
		const group = groups.get(key);
		if (group) {
			group.count += 1;
			if (!group.kinds.includes(violation.kind)) {
				group.kinds.push(violation.kind);
			}
			continue;
		}
		groups.set(key, {
			violation,
			count: 1,
			kinds: [violation.kind],
		});
	}
	return [...groups.values()];
}

export function visibleViolationDetail(group: VisibleViolationGroup): string {
	const kinds = group.kinds.join(", ");
	return group.count > 1 ? `${group.count} refs: ${kinds}` : kinds;
}

export function severityCounts(
	violations: readonly Violation[],
): { errors: number; warnings: number } {
	let errors = 0;
	let warnings = 0;
	for (const violation of violations) {
		if (violation.severity === "warn") {
			warnings += 1;
		} else {
			errors += 1;
		}
	}
	return { errors, warnings };
}

export function lineRangeLabel(lines: readonly [number, number]): string {
	const [start, end] = lines;
	return start === end ? `L${start}` : `L${start}-L${end}`;
}

function visibleViolationKey(violation: Violation): string {
	return [
		violation.rule_id,
		violation.severity,
		violation.lines[0],
		violation.lines[1],
		violation.explanation ?? violation.message,
	].join("\0");
}
