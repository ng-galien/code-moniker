import { parseRuleFile, RuleEntry } from "../rules/parse";
import { parseScenarioMarkdown } from "../scenario/markdown";
import { langById, langByTomlSection, LANGS } from "../shared/languages";
import { CatalogDocument, CatalogEntry, CatalogRule } from "./model";
import { PackEntry, loadPackIndex } from "./packs";

const SCENARIO_EXTENSION = ".cm.md";

export class CatalogRepository {
	private packCache: PackEntry[] | undefined;

	async entries(): Promise<CatalogEntry[]> {
		return builtinEntries(await this.packs());
	}

	private async packs(): Promise<PackEntry[]> {
		if (!this.packCache) {
			const result = await loadPackIndex();
			this.packCache = result.ok ? result.packs : [];
		}
		return this.packCache;
	}

	async findEntry(id: string): Promise<CatalogEntry | undefined> {
		return (await this.entries()).find((entry) => entry.id === id);
	}

	async readDocument(entry: CatalogEntry): Promise<CatalogDocument> {
		return { entry, document: entry.document ?? "" };
	}

	async rulesFor(entry: CatalogEntry): Promise<CatalogRule[]> {
		const document = entry.document ?? "";
		const scenario = parseScenarioMarkdown(document);
		const fallbackLang = entry.langId ?? scenario.cells.find((cell) => cell.kind === "file")?.fence;
		const rules = scenario.cells.flatMap((cell) => {
			if (cell.kind !== "rules") {
				return [];
			}
			return parseRuleFile(cell.value).rules.map((rule) => fillRuleLanguage(rule, fallbackLang));
		});
		return rules.map((rule) => ({ entry, rule }));
	}

	refresh(): void {
		this.packCache = undefined;
	}
}

function builtinEntries(packs: PackEntry[]): CatalogEntry[] {
	return packs.map((entry): CatalogEntry => ({
		id: entry.category === "learn" ? `builtin:learn:${entry.name}` : `builtin:pack:${entry.name}`,
		source: "builtin",
		kind: "pack",
		category: entry.category,
		title: entry.title,
		fileName: `${entry.name}${SCENARIO_EXTENSION}`,
		blurb: entry.blurb,
		langId: entry.langId,
		level: entry.category === "learn" ? "Learn" : "Reference",
		tags: ["builtin", entry.category, "pack", "scenario", entry.name],
		document: entry.document,
	}));
}

function fillRuleLanguage(rule: RuleEntry, language: string | undefined): RuleEntry {
	if (rule.sampleLang) {
		return rule;
	}
	const langId = language ? langByTomlSection(language)?.id ?? language : undefined;
	return langId ? { ...rule, sampleLang: langId } : rule;
}

export function catalogLanguageLabel(langId: string | undefined): string {
	return langId ? langById(langId)?.label ?? langId : "multi";
}

export function catalogLanguageIds(entries: CatalogEntry[]): string[] {
	const supported = new Set(LANGS.map((lang) => lang.id));
	const seen = new Set<string>();
	for (const entry of entries) {
		if (entry.langId && supported.has(entry.langId)) {
			seen.add(entry.langId);
		}
	}
	return [...seen].sort((left, right) => catalogLanguageLabel(left).localeCompare(catalogLanguageLabel(right)));
}
