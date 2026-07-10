import type { HighlightedUsageDto } from "./panel";

// Pure grouping and labelling model for the usage navigator: rows bucket into
// production/tests/technical, then group by file, by acting scope, and by
// (action, target) occurrence. No DOM, no React — shared shape for the view.
export type UsageDirectionScope = "incoming" | "outgoing";
export type UsageBucketKind = "production" | "test" | "technical";
export type UsageSummaryKind = UsageBucketKind | "context" | "file";

export interface UsageBucket {
	kind: UsageBucketKind;
	label: string;
	rows: HighlightedUsageDto[];
}

export interface UsageFileGroup {
	bucket: UsageBucketKind;
	contexts: UsageContextGroup[];
	file: string;
	rows: HighlightedUsageDto[];
	scope: UsageDirectionScope;
}

export interface UsageContextGroup {
	label: string;
	occurrences: UsageOccurrence[];
	rows: HighlightedUsageDto[];
}

export interface UsageOccurrence {
	key: string;
	kind: string;
	label: string;
	rows: HighlightedUsageDto[];
	sample: HighlightedUsageDto;
}

export function usageBuckets(rows: HighlightedUsageDto[]): UsageBucket[] {
	const buckets: UsageBucket[] = [
		{ kind: "production", label: "Production", rows: [] },
		{ kind: "test", label: "Tests", rows: [] },
		{ kind: "technical", label: "Type-only and imports", rows: [] },
	];
	for (const usage of rows) {
		buckets[bucketIndex(usage)].rows.push(usage);
	}
	return buckets;
}

function bucketIndex(usage: HighlightedUsageDto): 0 | 1 | 2 {
	if (isTechnicalUsage(usage)) {
		return 2;
	}
	if (isTestFile(usage.file)) {
		return 1;
	}
	return 0;
}

export function isTechnicalUsage(usage: HighlightedUsageDto): boolean {
	const kind = usage.kind.toLowerCase();
	return (
		kind.startsWith("imports_") ||
		kind === "uses_type" ||
		kind === "returns_type" ||
		kind === "annotates"
	);
}

export function isTestFile(file: string): boolean {
	return (
		/(^|[/.])(__tests__|tests?|specs?)([/.]|$)/i.test(file) ||
		/\.(test|spec)\.[^.]+$/i.test(file)
	);
}

export function groupUsages(
	rows: HighlightedUsageDto[],
	bucket: UsageBucketKind,
	scope: UsageDirectionScope,
): UsageFileGroup[] {
	const files = new Map<string, HighlightedUsageDto[]>();
	for (const usage of rows) {
		const file = usage.file || "unknown";
		if (!files.has(file)) {
			files.set(file, []);
		}
		files.get(file)?.push(usage);
	}
	return Array.from(files.entries())
		.map(([file, fileRows]) => ({
			bucket,
			file,
			rows: sortUsages(fileRows),
			scope,
			contexts: groupUsageContexts(sortUsages(fileRows)),
		}))
		.sort((a, b) => b.rows.length - a.rows.length || a.file.localeCompare(b.file));
}

function groupUsageContexts(rows: HighlightedUsageDto[]): UsageContextGroup[] {
	const contexts = new Map<string, HighlightedUsageDto[]>();
	for (const usage of rows) {
		const label = usage.actor || usage.context || usage.endpoint || usage.prefix || "unknown";
		if (!contexts.has(label)) {
			contexts.set(label, []);
		}
		contexts.get(label)?.push(usage);
	}
	return Array.from(contexts.entries())
		.map(([label, contextRows]) => ({
			label,
			rows: sortUsages(contextRows),
			occurrences: groupOccurrences(sortUsages(contextRows)),
		}))
		.sort((a, b) => b.rows.length - a.rows.length || a.label.localeCompare(b.label));
}

function groupOccurrences(rows: HighlightedUsageDto[]): UsageOccurrence[] {
	const groups = new Map<string, HighlightedUsageDto[]>();
	for (const usage of rows) {
		const key = usage.kind.toLowerCase() + ":" + usageTarget(usage);
		if (!groups.has(key)) {
			groups.set(key, []);
		}
		groups.get(key)?.push(usage);
	}
	return Array.from(groups.entries())
		.flatMap(([key, groupRows]) => {
			const first = groupRows[0];
			if (!first) {
				return [];
			}
			return [
				{
					key: "usage:" + first.direction + ":" + key + ":" + first.file + ":" + first.location,
					kind: first.kind,
					label: usageTarget(first),
					rows: groupRows,
					sample: previewSample(groupRows),
				},
			];
		})
		.sort(
			(a, b) =>
				actionRank(a.kind) - actionRank(b.kind) ||
				b.rows.length - a.rows.length ||
				a.label.localeCompare(b.label),
		);
}

function previewSample(rows: HighlightedUsageDto[]): HighlightedUsageDto {
	return (
		rows.find((row) => row.line_range && !isTechnicalUsage(row)) ||
		rows.find((row) => row.line_range) ||
		rows[0]
	);
}

function sortUsages(rows: HighlightedUsageDto[]): HighlightedUsageDto[] {
	return [...rows].sort(
		(a, b) =>
			actionRank(a.kind) - actionRank(b.kind) ||
			usageTarget(a).localeCompare(usageTarget(b)) ||
			String(a.location || "").localeCompare(String(b.location || "")),
	);
}

function actionRank(kind: string): number {
	const normalized = kind.toLowerCase();
	const ranks: Record<string, number> = {
		calls: 0,
		method_call: 0,
		instantiates: 1,
		writes: 2,
		reads: 3,
		extends: 4,
		implements: 4,
		uses_type: 8,
		returns_type: 8,
		imports_symbol: 9,
		imports_module: 9,
	};
	return ranks[normalized] ?? 5;
}

export function usageTarget(usage: HighlightedUsageDto): string {
	return compactSymbol(usage.endpoint || usage.actor || usage.context || usage.prefix || "usage");
}

export function usageAction(kind: string): string {
	const normalized = kind.toLowerCase();
	const labels: Record<string, string> = {
		calls: "calls",
		method_call: "calls",
		reads: "reads",
		writes: "writes",
		instantiates: "creates",
		extends: "extends",
		implements: "implements",
		annotates: "annotates",
		returns_type: "returns type",
		uses_type: "uses type",
		imports_symbol: "imports",
		imports_module: "imports",
	};
	return labels[normalized] || normalized.replaceAll("_", " ");
}

export function kindLabel(kind: UsageSummaryKind): string {
	const labels: Record<UsageSummaryKind, string> = {
		production: "code",
		test: "tests",
		technical: "types",
		context: "scope",
		file: "file",
	};
	return labels[kind] || kind;
}

export function compactSymbol(value: string): string {
	return String(value || "unknown")
		.replace(/\s+/g, " ")
		.replace(/\(([^)]{56})[^)]*\)/, "($1...)");
}

export function splitFile(file: string): { dir: string; name: string } {
	const parts = String(file || "unknown").split("/");
	const name = parts.pop() || "unknown";
	const dir = parts.slice(-2).join("/");
	return { dir, name };
}

export function fileKind(file: string): string {
	if (isTestFile(file)) {
		return "test";
	}
	return file.split(".").pop()?.toLowerCase() || "file";
}

export function bucketMeta(rows: HighlightedUsageDto[]): string {
	const files = new Set(rows.map((row) => row.file || "unknown")).size;
	const contexts = new Set(
		rows.map((row) => row.actor || row.context || row.endpoint || row.prefix || "unknown"),
	).size;
	return `${files} file${files > 1 ? "s" : ""} · ${contexts} scope${contexts > 1 ? "s" : ""}`;
}

export function actionMeta(rows: HighlightedUsageDto[]): string {
	const counts = new Map<string, number>();
	for (const row of rows) {
		const action = usageAction(row.kind);
		counts.set(action, (counts.get(action) || 0) + 1);
	}
	return Array.from(counts.entries())
		.sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
		.slice(0, 2)
		.map(([action, count]) => (count > 1 ? `${count} ${action}` : action))
		.join(" · ");
}

export function occurrenceTooltip(occurrence: UsageOccurrence): string {
	const usage = occurrence.sample;
	return [
		usageAction(usage.kind),
		occurrence.label,
		usage.file,
		occurrence.rows.length > 1 ? `${occurrence.rows.length} references` : usage.location,
	]
		.filter(Boolean)
		.join(" · ");
}
