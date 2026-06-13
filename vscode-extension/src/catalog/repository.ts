import * as path from "node:path";
import * as vscode from "vscode";

import { langById, langByTomlSection, LANGS } from "../shared/languages";
import { CmnbCell, CmnbDocument } from "../notebook/model";
import { lessonCells } from "../notebook/factory";
import { parseRuleFile, RuleEntry } from "../rules/parse";
import { firstLine, workspaceLabel } from "../shared/workspace";
import { CONCEPTS, LESSONS } from "./data";
import { loadLessonCells } from "./lessons";
import { CatalogDocument, CatalogEntry, CatalogRule } from "./model";
import { PackEntry, loadPackIndex } from "./packs";

const USER_CATALOG_FOLDER = ".code-moniker/catalog";

export class CatalogRepository {
	private readonly cellCache = new Map<string, CmnbCell[]>();
	private packCache: PackEntry[] | undefined;

	constructor(private readonly context: vscode.ExtensionContext) {}

	async entries(): Promise<CatalogEntry[]> {
		const builtin = builtinEntries(await this.packs());
		const user = await userEntries(await this.userCatalogFolder());
		return [...builtin, ...user];
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
		return { entry, cells: await this.readCells(entry) };
	}

	async readCells(entry: CatalogEntry): Promise<CmnbCell[]> {
		if (entry.source === "user") {
			return loadCells(this.context, entry);
		}
		const cached = this.cellCache.get(entry.id);
		if (cached) {
			return cached;
		}
		const loaded = await loadCells(this.context, entry);
		this.cellCache.set(entry.id, loaded);
		return loaded;
	}

	async rulesFor(entry: CatalogEntry): Promise<CatalogRule[]> {
		const cells = await this.readCells(entry);
		const rules = cells.flatMap((cell) => {
			if (cell.kind !== "rule") {
				return [];
			}
			return parseRuleFile(cell.value).rules.map((rule) => fillRuleLanguage(rule, cell.language));
		});
		return rules.map((rule) => ({ entry, rule }));
	}

	async userCatalogFolder(): Promise<vscode.Uri | undefined> {
		const folder = vscode.workspace.workspaceFolders?.[0];
		if (!folder) {
			return undefined;
		}
		const configured = vscode.workspace
			.getConfiguration("codeMoniker")
			.get<string>("catalog.userFolder", USER_CATALOG_FOLDER);
		const normalized = configured.replace(/^\/+/, "");
		return vscode.Uri.joinPath(folder.uri, ...normalized.split("/").filter(Boolean));
	}

	async copyToUserCatalog(entry: CatalogEntry): Promise<{ ok: true; uri: vscode.Uri } | { ok: false; error: string }> {
		const folder = await this.userCatalogFolder();
		if (!folder) {
			return {
				ok: false,
				error: "Open a workspace folder before copying a catalog entry.",
			};
		}
		await vscode.workspace.fs.createDirectory(folder);
		if (entry.document !== undefined) {
			const uri = await uniqueUri(folder, scenarioFileName(entry));
			await vscode.workspace.fs.writeFile(uri, new TextEncoder().encode(entry.document));
			return { ok: true, uri };
		}
		const cells = await this.readCells(entry);
		const uri = await uniqueUri(folder, entry.fileName);
		const doc: CmnbDocument = {
			version: 1,
			title: entry.title,
			catalog: { copiedFrom: entry.id },
			cells,
		};
		await vscode.workspace.fs.writeFile(uri, encodeNotebook(doc));
		this.cellCache.delete(`user:${uri.toString()}`);
		return { ok: true, uri };
	}

	refreshUserEntries(): void {
		this.packCache = undefined;
		for (const key of [...this.cellCache.keys()]) {
			if (key.startsWith("user:")) {
				this.cellCache.delete(key);
			}
		}
	}
}

async function userEntries(folder: vscode.Uri | undefined): Promise<CatalogEntry[]> {
	if (!folder) {
		return [];
	}
	const workspace = vscode.workspace.getWorkspaceFolder(folder);
	if (!workspace) {
		return [];
	}
	const root = relativeGlob(workspace.uri, folder);
	const pattern = new vscode.RelativePattern(workspace, `${root}/**/*.{cmnb,md}`);
	const uris = await vscode.workspace.findFiles(pattern);
	const entries: CatalogEntry[] = [];
	for (const uri of uris.sort((a, b) => a.fsPath.localeCompare(b.fsPath))) {
		const entry = uri.fsPath.endsWith(".md")
			? await userScenarioEntry(uri)
			: await userNotebookEntry(uri);
		if (entry) {
			entries.push(entry);
		}
	}
	return entries;
}

async function userNotebookEntry(uri: vscode.Uri): Promise<CatalogEntry | undefined> {
	const doc = await readNotebook(uri);
	if (!doc) {
		return undefined;
	}
	const title = doc.title ?? titleFromFile(uri);
	return {
		id: `user:${uri.toString()}`,
		source: "user",
		kind: "notebook",
		title,
		fileName: path.basename(uri.fsPath),
		blurb: blurbFromCells(doc.cells) ?? workspaceLabel(uri),
		langId: dominantLanguage(doc.cells),
		level: "Practice",
		tags: ["user", ...languagesIn(doc.cells)],
		uri,
	};
}

async function userScenarioEntry(uri: vscode.Uri): Promise<CatalogEntry | undefined> {
	const document = await readScenarioFile(uri);
	if (document === undefined) {
		return undefined;
	}
	const meta = scenarioFrontMatter(document);
	return {
		id: `user:${uri.toString()}`,
		source: "user",
		kind: "scenario",
		title: meta.name ?? titleFromFile(uri),
		fileName: path.basename(uri.fsPath),
		blurb: meta.blurb ?? workspaceLabel(uri),
		langId: meta.lang ? langByTomlSection(meta.lang)?.id : undefined,
		level: "Practice",
		tags: ["user", "scenario", ...(meta.lang ? [meta.lang] : [])],
		uri,
		document,
	};
}

async function loadCells(
	context: vscode.ExtensionContext,
	entry: CatalogEntry,
): Promise<CmnbCell[]> {
	if (entry.source === "user") {
		if (!entry.uri) {
			return [];
		}
		const doc = await readNotebook(entry.uri);
		return doc?.cells ?? [];
	}
	if (entry.kind === "concept") {
		const concept = CONCEPTS.find((candidate) => `builtin:concept:${candidate.id}` === entry.id);
		if (!concept) {
			return [];
		}
		return lessonCells(concept.title, concept.blurb, concept.langId, concept.sample, concept.ruleToml);
	}
	if (entry.kind === "lesson") {
		const lesson = LESSONS.find((candidate) => `builtin:lesson:${candidate.id}` === entry.id);
		if (!lesson) {
			return [];
		}
		const loaded = await loadLessonCells(context, lesson.id);
		if (!loaded.ok) {
			throw new Error(loaded.error);
		}
		return loaded.cells;
	}
	// Packs are multi-file scenarios, not .cmnb notebooks. They are opened via
	// the scenario notebook type (openScenarioDocument); reaching here means a
	// caller tried to render a pack as a single-file .cmnb — fail loudly rather
	// than silently flatten the layout.
	throw new Error(
		`Catalog entry "${entry.id}" is a scenario; open it with the scenario notebook, not as .cmnb.`,
	);
}

function builtinEntries(packs: PackEntry[]): CatalogEntry[] {
	return [
		...LESSONS.map((entry): CatalogEntry => ({
			id: `builtin:lesson:${entry.id}`,
			source: "builtin",
			kind: "lesson",
			title: entry.title,
			fileName: `${entry.title}.cmnb`,
			blurb: entry.blurb,
			langId: entry.langId,
			level: "Basics",
			tags: ["builtin", "lesson", ...entry.tags],
		})),
		...CONCEPTS.map((entry): CatalogEntry => ({
			id: `builtin:concept:${entry.id}`,
			source: "builtin",
			kind: "concept",
			title: entry.title,
			fileName: `${entry.title}.cmnb`,
			blurb: entry.blurb,
			langId: entry.langId,
			level: "Basics",
			tags: ["builtin", "concept", entry.id],
		})),
		...packs.map((entry): CatalogEntry => ({
			id: `builtin:pack:${entry.name}`,
			source: "builtin",
			kind: "pack",
			title: `${entry.name} scenario`,
			fileName: `${entry.name}.md`,
			blurb: entry.blurb,
			langId: entry.langId,
			level: "Reference",
			tags: ["builtin", "pack", "scenario", entry.name],
			document: entry.document,
		})),
	];
}

async function readNotebook(uri: vscode.Uri): Promise<CmnbDocument | undefined> {
	try {
		const bytes = await vscode.workspace.fs.readFile(uri);
		const doc = JSON.parse(new TextDecoder().decode(bytes)) as CmnbDocument;
		if (!Array.isArray(doc.cells)) {
			return undefined;
		}
		return doc;
	} catch {
		return undefined;
	}
}

function encodeNotebook(doc: CmnbDocument): Uint8Array {
	return new TextEncoder().encode(JSON.stringify(doc, null, "\t") + "\n");
}

function fillRuleLanguage(rule: RuleEntry, language: string): RuleEntry {
	if (rule.sampleLang) {
		return rule;
	}
	return { ...rule, sampleLang: language };
}

function languagesIn(cells: CmnbCell[]): string[] {
	const set = new Set<string>();
	for (const cell of cells) {
		if (cell.kind !== "markdown") {
			set.add(cell.language);
		}
	}
	return [...set];
}

function dominantLanguage(cells: CmnbCell[]): string | undefined {
	return languagesIn(cells)[0];
}

function blurbFromCells(cells: CmnbCell[]): string | undefined {
	const markdown = cells.find((cell) => cell.kind === "markdown")?.value;
	if (!markdown) {
		return undefined;
	}
	return firstLine(markdown).replace(/^#\s+/, "");
}

function titleFromFile(uri: vscode.Uri): string {
	return path.basename(uri.fsPath).replace(/\.(cmnb|md)$/, "");
}

function relativeGlob(root: vscode.Uri, folder: vscode.Uri): string {
	const relative = path.relative(root.fsPath, folder.fsPath).split(path.sep).join("/");
	return relative || ".";
}

async function uniqueUri(folder: vscode.Uri, fileName: string): Promise<vscode.Uri> {
	const parsed = path.parse(safeFileName(fileName));
	const ext = parsed.ext || ".cmnb";
	for (let index = 0; ; index++) {
		const suffix = index === 0 ? "" : ` ${index + 1}`;
		const candidate = vscode.Uri.joinPath(folder, `${parsed.name}${suffix}${ext}`);
		if (!(await exists(candidate))) {
			return candidate;
		}
	}
}

async function exists(uri: vscode.Uri): Promise<boolean> {
	try {
		await vscode.workspace.fs.stat(uri);
		return true;
	} catch {
		return false;
	}
}

function safeFileName(fileName: string): string {
	const trimmed = fileName.replace(/[/:\\?%*"<>|]/g, "-").trim();
	return trimmed || "code-moniker-sample.cmnb";
}

function scenarioFileName(entry: CatalogEntry): string {
	const base = entry.fileName.endsWith(".md") ? entry.fileName : `${entry.title}.md`;
	return safeFileName(base);
}

async function readScenarioFile(uri: vscode.Uri): Promise<string | undefined> {
	try {
		const bytes = await vscode.workspace.fs.readFile(uri);
		const text = new TextDecoder().decode(bytes);
		return text.includes("cm:rules") || text.includes("cm:file=") ? text : undefined;
	} catch {
		return undefined;
	}
}

function scenarioFrontMatter(document: string): { name?: string; lang?: string; blurb?: string } {
	const match = /^---\n([\s\S]*?)\n---/.exec(document);
	if (!match) {
		return {};
	}
	const meta: { name?: string; lang?: string; blurb?: string } = {};
	for (const line of match[1].split("\n")) {
		const pair = /^(name|lang|blurb):\s*(.*)$/.exec(line.trim());
		if (pair) {
			meta[pair[1] as "name" | "lang" | "blurb"] = pair[2].trim();
		}
	}
	return meta;
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
	return [...seen].sort();
}
