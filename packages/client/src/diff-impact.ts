import { execFile } from "node:child_process";
import { mkdir, mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, extname, join, resolve } from "node:path";

import type {
	DiffImpactCompareFile,
	DiffImpactRef,
	DiffImpactResult,
	DiffImpactSide,
	DiffImpactSymbol,
	WorkspaceSourceDocumentDto,
} from "./generated.js";
import type { NodeDaemonRuntime, OwnedDaemon } from "./node.js";
import type { CodeMonikerClient } from "./client.js";

const GIT_OUTPUT_LIMIT = 32 * 1024 * 1024;

export interface GitDiffImpactOptions {
	repository: string;
	base: string;
	head: string;
	project?: string;
	ticket?: string;
	gitBinary?: string;
	environment?: Record<string, string | undefined>;
	binaryCandidates?: readonly [string, ...string[]];
}

export interface DiffImpactFileInventory {
	status: "added" | "modified" | "deleted" | "renamed";
	oldPath: string | null;
	newPath: string | null;
	renameScore: number | null;
	language: string | null;
	category: "source" | "binary" | "documentation" | "schema" | "manifest" | "lockfile" | "configuration" | null;
	analyzed: boolean;
	omission: string | null;
}

export interface DiffImpactArtifact {
	schemaVersion: 1;
	kind: "code-moniker.diff-impact";
	repository: string;
	project: string;
	ticket: string | null;
	revisions: {
		base: { requested: string; resolved: string };
		head: { requested: string; resolved: string };
	};
	scope: string;
	inventory: {
		files: DiffImpactFileInventory[];
		totals: Record<DiffImpactFileInventory["status"], number>;
	};
	semantic: DiffImpactResult;
	tests: {
		basis: "analyzed-path-and-extractor-kind";
		files: string[];
		symbolChanges: number;
	};
	coverage: {
		corpus: "changed-files";
		changedFiles: number;
		analyzedFiles: number;
		skippedFiles: number;
		relations: "changed-file-extraction";
	};
	limitations: string[];
}

export interface DiffImpactOutput {
	artifact: DiffImpactArtifact;
	json: string;
	text: string;
}

export interface GitChange {
	status: DiffImpactFileInventory["status"];
	oldPath: string | null;
	newPath: string | null;
	renameScore: number | null;
	oldHunks: Array<{ start: number; end: number }>;
	newHunks: Array<{ start: number; end: number }>;
}

interface PreparedDiffImpact {
	base: string;
	head: string;
	files: DiffImpactCompareFile[];
	baseDocuments: WorkspaceSourceDocumentDto[];
	headDocuments: WorkspaceSourceDocumentDto[];
	inventory: DiffImpactFileInventory[];
}

export async function runGitDiffImpact(
	options: GitDiffImpactOptions,
	createRuntime: (registryDirectory: string) => NodeDaemonRuntime,
): Promise<DiffImpactOutput> {
	const sessionDirectory = await mkdtemp(join(tmpdir(), "code-moniker-diff-impact-"));
	const workspace = join(sessionDirectory, "workspace");
	const registry = join(sessionDirectory, "registry");
	await mkdir(workspace);
	await mkdir(registry);
	const runtime = createRuntime(registry);
	let source: GitRevisionSource | undefined;
	let owned: OwnedDaemon | undefined;
	let client: CodeMonikerClient | undefined;
	try {
		source = await GitRevisionSource.open(options);
		const prepared = await source.prepare();
		owned = await runtime.launch({
			workspaceRoots: [workspace],
			binaryCandidates: options.binaryCandidates,
		});
		client = await runtime.connect(owned.entry, {
			clientName: "@code-moniker/client/diff-impact",
		});
		if (!client.supportsQuery("diff-impact.compare")) {
			throw new Error("the owned daemon does not support diff-impact.compare");
		}
		const project = options.project ?? inferProjectName(options.repository);
		const scope = `${prepared.base}..${prepared.head}`;
		const semantic = await client.diffImpact.compare({
			scope,
			project,
			base: {
				srcset: "diff-impact",
				revision: prepared.base,
				documents: prepared.baseDocuments,
			},
			head: {
				srcset: "diff-impact",
				revision: prepared.head,
				documents: prepared.headDocuments,
			},
			files: prepared.files,
		});
		const artifact = buildArtifact(options, project, prepared, semantic);
		return {
			artifact,
			json: canonicalJson(artifact),
			text: renderDiffImpact(artifact),
		};
	} finally {
		client?.close();
		try {
			if (owned !== undefined) {
				try {
					await runtime.stopOwned(owned);
				} catch {
					owned.process.terminate();
					await runtime.waitForExit(owned.entry);
				}
			}
		} finally {
			try {
				await source?.dispose();
			} finally {
				await rm(sessionDirectory, { recursive: true, force: true });
			}
		}
	}
}

export function renderDiffImpact(artifact: DiffImpactArtifact): string {
	const summary = artifact.semantic.summary;
	const lines = [
		`Diff impact ${shortRevision(artifact.revisions.base.resolved)}..${shortRevision(artifact.revisions.head.resolved)}`,
		`${artifact.coverage.changedFiles} changed files; ${artifact.coverage.analyzedFiles} analyzed; ${summary.symbol_changes} symbol changes; ${summary.ref_changes} relation changes.`,
	];
	const statuses = artifact.inventory.totals;
	lines.push(
		`Files: +${statuses.added} ~${statuses.modified} -${statuses.deleted} →${statuses.renamed}.`,
	);
	const symbolKinds = countBy(artifact.semantic.symbol_changes, (change) => change.kind);
	if (symbolKinds.size > 0) {
		lines.push(`Symbols: ${renderCounts(symbolKinds)}.`);
		const publicChanges = artifact.semantic.symbol_changes.filter(isPublicSymbolChange).length;
		lines.push(`Public surface: ${publicChanges} changed public symbols.`);
		lines.push("Representative symbols:");
		for (const [kind, changes] of groupSymbolChanges(artifact.semantic.symbol_changes)) {
			const shown = representativeSymbolChanges(changes, 6).map(renderSymbolChange);
			if (shown.length === 0) continue;
			const remainder = changes.length - shown.length;
			lines.push(`- ${kind} (${changes.length}): ${shown.join("; ")}${remainder > 0 ? `; +${remainder} more` : ""}`);
		}
	}
	const byZone = groupInventoryByZone(artifact);
	if (byZone.size > 0) {
		lines.push("Zones:");
		for (const [zone, files] of byZone) {
			lines.push(`- ${zone}`);
			for (const file of files) {
				lines.push(`  - ${renderFileImpact(artifact, file)}`);
				if (!file.analyzed) continue;
				for (const change of fileSymbolChanges(artifact, inventoryPath(file))) {
					lines.push(`    - [${change.kind}] ${renderFileSymbolChange(change)}`);
				}
			}
		}
	}
	const pureMoves = artifact.semantic.files.filter((file) => file.disposition === "moved").length;
	if (pureMoves > 0) {
		lines.push(`${pureMoves} files are established as pure symbolic moves.`);
	}
	if (summary.ref_changes > 0) {
		const relationKinds = countBy(artifact.semantic.ref_changes, (change) => change.kind);
		lines.push(
			`Relations: ${summary.ref_changes} extracted changes from changed files (${renderCounts(relationKinds)}); ${summary.retargeted_refs} retargeted.`,
		);
		const retargeted = representativeRetargets(artifact.semantic.ref_changes, 5);
		if (retargeted.length > 0) {
			lines.push("Representative retargets:");
			for (const change of retargeted) lines.push(`- ${renderRetarget(change)}`);
		}
	}
	lines.push(
		artifact.tests.files.length > 0
			? `Tests: ${artifact.tests.files.length} changed analyzed test files (${artifact.tests.files.join(", ")}); ${artifact.tests.symbolChanges} test-symbol changes.`
			: "Tests: no changed analyzed test file or extracted test symbol was established in the bounded corpus.",
	);
	if (artifact.limitations.length > 0) {
		lines.push("Limits:");
		for (const limitation of artifact.limitations) {
			lines.push(`- ${limitation}`);
		}
	}
	return `${lines.join("\n")}\n`;
}

function groupInventoryByZone(
	artifact: DiffImpactArtifact,
): Map<string, DiffImpactFileInventory[]> {
	const groups = new Map<string, DiffImpactFileInventory[]>();
	for (const file of artifact.inventory.files) {
		const path = inventoryPath(file);
		const zone = zoneForPath(path);
		const files = groups.get(zone) ?? [];
		files.push(file);
		groups.set(zone, files);
	}
	return new Map(
		[...groups]
			.sort(([left], [right]) => ordinalCompare(left, right))
			.map(([zone, files]) => [
				zone,
				files.sort((left, right) => ordinalCompare(inventoryPath(left), inventoryPath(right))),
			]),
	);
}

function renderFileImpact(
	artifact: DiffImpactArtifact,
	file: DiffImpactFileInventory,
): string {
	const path = inventoryPath(file);
	if (!file.analyzed) {
		const category = file.category === null ? "" : `; category=${file.category}`;
		return `${path} — status=${file.status}; outside-index${category}; reason=${file.omission ?? "not analyzable"}`;
	}
	const symbols = fileSymbolChanges(artifact, path);
	const publicSymbols = symbols.filter(isPublicSymbolChange).length;
	const relations = artifact.semantic.ref_changes.filter((change) => change.file === path).length;
	const tests = symbols.filter(
		(change) => change.new?.test_artifact === true || change.old?.test_artifact === true,
	).length;
	return `${path} — status=${file.status}; analyzed; symbols=${symbols.length}; public=${publicSymbols}; relations=${relations}; tests=${tests}`;
}

function fileSymbolChanges(
	artifact: DiffImpactArtifact,
	path: string,
): DiffImpactSymbol[] {
	return artifact.semantic.symbol_changes.filter(
		(change) => symbolChangePath(change) === path,
	);
}

function inventoryPath(file: DiffImpactFileInventory): string {
	return file.newPath ?? file.oldPath ?? "<unknown>";
}

function symbolChangePath(change: DiffImpactSymbol): string {
	return change.new?.file ?? change.old?.file ?? "<unknown>";
}

function zoneForPath(path: string): string {
	const parts = path.split("/").filter(Boolean);
	if (parts.length <= 1) return "<root>";
	return parts.slice(0, Math.min(2, parts.length - 1)).join("/");
}

function countBy<T>(values: readonly T[], keyOf: (value: T) => string): Map<string, number> {
	const counts = new Map<string, number>();
	for (const value of values) {
		const key = keyOf(value);
		counts.set(key, (counts.get(key) ?? 0) + 1);
	}
	return new Map([...counts].sort(([left], [right]) => ordinalCompare(left, right)));
}

function renderCounts(counts: ReadonlyMap<string, number>): string {
	return [...counts].map(([kind, count]) => `${kind}=${count}`).join(", ");
}

function groupSymbolChanges(changes: readonly DiffImpactSymbol[]): Array<[string, DiffImpactSymbol[]]> {
	const groups = new Map<string, DiffImpactSymbol[]>();
	for (const change of [...changes].sort(compareSymbolChanges)) {
		const group = groups.get(change.kind) ?? [];
		group.push(change);
		groups.set(change.kind, group);
	}
	return [...groups].sort(([left], [right]) => ordinalCompare(left, right));
}

function compareSymbolChanges(left: DiffImpactSymbol, right: DiffImpactSymbol): number {
	const structuralOrder = symbolChangeScore(right) - symbolChangeScore(left);
	if (structuralOrder !== 0) return structuralOrder;
	const publicOrder = Number(isPublicSymbolChange(right)) - Number(isPublicSymbolChange(left));
	if (publicOrder !== 0) return publicOrder;
	return ordinalCompare(symbolIdentity(left.new ?? left.old), symbolIdentity(right.new ?? right.old));
}

function representativeSymbolChanges(changes: readonly DiffImpactSymbol[], limit: number): DiffImpactSymbol[] {
	const candidates = [...changes]
		.filter((change) => !isSyntheticOrLeaf(change))
		.sort(compareSymbolChanges);
	const selected: DiffImpactSymbol[] = [];
	const files = new Set<string>();
	for (const change of candidates) {
		const side = change.new ?? change.old;
		const file = side?.file ?? "<unknown>";
		if (files.has(file)) continue;
		selected.push(change);
		files.add(file);
		if (selected.length === limit) return selected;
	}
	for (const change of candidates) {
		if (selected.includes(change)) continue;
		selected.push(change);
		if (selected.length === limit) break;
	}
	return selected;
}

function symbolChangeScore(change: DiffImpactSymbol): number {
	const side = change.new ?? change.old;
	const kindScore = ({
		class: 90, interface: 90, trait: 90, enum: 85, struct: 85,
		fn: 80, function: 80, method: 70, const: 60, module: 50,
	} as Record<string, number>)[side?.kind ?? ""] ?? 0;
	return kindScore + (isPublicSymbolChange(change) ? 100 : 0);
}

function isSyntheticOrLeaf(change: DiffImpactSymbol): boolean {
	const side = change.new ?? change.old;
	if (side?.test_artifact === true) return true;
	if (side === null || side === undefined) return true;
	if (["field", "parameter", "param", "variable"].includes(side.kind)) return true;
	return symbolIdentity(side).includes("function:__cb_");
}

function isPublicSymbolChange(change: DiffImpactSymbol): boolean {
	return change.old?.visibility === "public" || change.new?.visibility === "public";
}

function renderSymbolChange(change: DiffImpactSymbol): string {
	const oldIdentity = symbolIdentity(change.old);
	const newIdentity = symbolIdentity(change.new);
	return oldIdentity !== "<unknown>" && newIdentity !== "<unknown>" && oldIdentity !== newIdentity
		? `${oldIdentity} → ${newIdentity}`
		: newIdentity !== "<unknown>" ? newIdentity : oldIdentity;
}

function renderFileSymbolChange(change: DiffImpactSymbol): string {
	const oldSymbol = fileSymbolDescriptor(change.old);
	const newSymbol = fileSymbolDescriptor(change.new);
	return oldSymbol !== "<unknown>" && newSymbol !== "<unknown>" && oldSymbol !== newSymbol
		? `${oldSymbol} → ${newSymbol}`
		: newSymbol !== "<unknown>" ? newSymbol : oldSymbol;
}

function fileSymbolDescriptor(side: DiffImpactSide | null | undefined): string {
	return side === null || side === undefined ? "<unknown>" : `${side.kind}:${side.name}`;
}

function symbolIdentity(side: DiffImpactSide | null | undefined): string {
	return side?.compact_identity ?? side?.identity ?? "<unknown>";
}

function renderRetarget(change: DiffImpactRef): string {
	return `${change.file}: ${change.old_target_compact ?? change.old_target ?? "<unknown>"} → ${change.new_target_compact ?? change.new_target ?? "<unknown>"}`;
}

function representativeRetargets(changes: readonly DiffImpactRef[], limit: number): DiffImpactRef[] {
	const selected: DiffImpactRef[] = [];
	const seen = new Set<string>();
	for (const change of changes) {
		const oldTarget = change.old_target_compact ?? change.old_target;
		const newTarget = change.new_target_compact ?? change.new_target;
		if (oldTarget === null || oldTarget === undefined || newTarget === null || newTarget === undefined) continue;
		if (oldTarget === newTarget || oldTarget.includes("function:__cb_") || newTarget.includes("function:__cb_")) continue;
		const key = `${change.file}\0${oldTarget}\0${newTarget}`;
		if (seen.has(key)) continue;
		seen.add(key);
		selected.push(change);
		if (selected.length === limit) break;
	}
	return selected;
}

export function canonicalJson(value: unknown): string {
	return `${JSON.stringify(sortJson(value), null, 2)}\n`;
}

class GitRevisionSource {
	private constructor(
		private readonly options: GitDiffImpactOptions,
		private readonly repositoryArguments: string[],
		private readonly temporaryDirectory?: string,
	) {}

	static async open(options: GitDiffImpactOptions): Promise<GitRevisionSource> {
		const local = await isDirectory(options.repository);
		if (local) {
			return new GitRevisionSource(options, ["-C", resolve(options.repository)]);
		}
		const temporaryDirectory = await mkdtemp(join(tmpdir(), "code-moniker-git-"));
		const gitDirectory = join(temporaryDirectory, "repository.git");
		const source = new GitRevisionSource(options, ["--git-dir", gitDirectory], temporaryDirectory);
		try {
			await source.git(["init", "--bare", gitDirectory], false);
			await source.git(["--git-dir", gitDirectory, "remote", "add", "origin", options.repository], false);
			await source.git(["--git-dir", gitDirectory, "config", "remote.origin.promisor", "true"], false);
			await source.git(["--git-dir", gitDirectory, "config", "remote.origin.partialclonefilter", "blob:none"], false);
			await source.fetchRevision(options.base, "base");
			await source.fetchRevision(options.head, "head");
			return source;
		} catch (error) {
			await source.dispose();
			throw error;
		}
	}

	async prepare(): Promise<PreparedDiffImpact> {
		const base = await this.resolveRevision(this.options.base, "base");
		const head = await this.resolveRevision(this.options.head, "head");
		const changes = await this.changes(base, head);
		const baseDocuments: WorkspaceSourceDocumentDto[] = [];
		const headDocuments: WorkspaceSourceDocumentDto[] = [];
		const files: DiffImpactCompareFile[] = [];
		const inventory: DiffImpactFileInventory[] = [];
		for (const change of changes) {
			const path = change.newPath ?? change.oldPath ?? "";
			const oldLanguage = change.oldPath === null ? null : sourceLanguageForPath(change.oldPath);
			const newLanguage = change.newPath === null ? null : sourceLanguageForPath(change.newPath);
			const language = newLanguage ?? oldLanguage;
			let omission: string | null = null;
			let oldContent: string | undefined;
			let newContent: string | undefined;
			if (change.status === "renamed" && oldLanguage !== newLanguage) {
				omission = `language changed across rename (${oldLanguage ?? "unsupported"} -> ${newLanguage ?? "unsupported"})`;
			} else if (language === null) {
				omission = "unsupported language";
			} else {
				try {
					if (change.oldPath !== null) oldContent = await this.blob(base, change.oldPath);
					if (change.newPath !== null) newContent = await this.blob(head, change.newPath);
					if (oldContent?.includes("\0") || newContent?.includes("\0")) omission = "binary content";
				} catch (error) {
					omission = `content unavailable: ${messageOf(error)}`;
				}
			}
			const analyzed = language !== null && omission === null;
			inventory.push({
				status: change.status,
				oldPath: change.oldPath,
				newPath: change.newPath,
				renameScore: change.renameScore,
				language,
				category: fileCategory(path, language, omission),
				analyzed,
				omission,
			});
			if (!analyzed || language === null) continue;
			if (oldContent !== undefined && change.oldPath !== null) {
				baseDocuments.push({ uri: change.oldPath, language, content: oldContent });
			}
			if (newContent !== undefined && change.newPath !== null) {
				headDocuments.push({ uri: change.newPath, language, content: newContent });
			}
			files.push({
				status: change.status,
				old_uri: change.oldPath,
				new_uri: change.newPath,
				old_hunks: change.oldHunks,
				new_hunks: change.newHunks,
				rename_score: change.renameScore,
			});
		}
		return {
			base,
			head,
			files,
			baseDocuments: baseDocuments.sort(byUri),
			headDocuments: headDocuments.sort(byUri),
			inventory: inventory.sort(compareInventory),
		};
	}

	async dispose(): Promise<void> {
		if (this.temporaryDirectory !== undefined) {
			await rm(this.temporaryDirectory, { recursive: true, force: true });
		}
	}

	private async fetchRevision(revision: string, label: "base" | "head"): Promise<void> {
		await this.git([
			...this.repositoryArguments,
			"fetch",
			"--no-tags",
			"--depth=1",
			"--filter=blob:none",
			"origin",
			`+${revision}:refs/code-moniker/${label}`,
		], false);
	}

	private async resolveRevision(requested: string, remoteLabel: "base" | "head"): Promise<string> {
		const revision = this.temporaryDirectory === undefined
			? requested
			: `refs/code-moniker/${remoteLabel}`;
		return (await this.git([...this.repositoryArguments, "rev-parse", "--verify", `${revision}^{commit}`], false)).trim();
	}

	private async changes(base: string, head: string): Promise<GitChange[]> {
		const raw = await this.git([
			...this.repositoryArguments,
			"diff",
			"--name-status",
			"-z",
			"--find-renames",
			base,
			head,
		], false);
		const changes = parseGitNameStatus(raw);
		for (const change of changes) {
			const path = change.newPath ?? change.oldPath;
			if (path === null) continue;
			const patch = await this.git([
				...this.repositoryArguments,
				"diff",
				"--unified=0",
				"--no-color",
				base,
				head,
				"--",
				change.oldPath ?? path,
				...(change.newPath !== null && change.newPath !== change.oldPath ? [change.newPath] : []),
			], false);
			const hunks = parseHunks(patch);
			change.oldHunks = hunks.old;
			change.newHunks = hunks.new;
		}
		return changes.sort(compareChanges);
	}

	private blob(revision: string, path: string): Promise<string> {
		return this.git([...this.repositoryArguments, "show", `${revision}:${path}`], false);
	}

	private git(args: string[], includeRepository = true): Promise<string> {
		return runProcess(this.options.gitBinary ?? "git", includeRepository ? [...this.repositoryArguments, ...args] : args, this.options.environment);
	}
}

function buildArtifact(
	options: GitDiffImpactOptions,
	project: string,
	prepared: PreparedDiffImpact,
	semantic: DiffImpactResult,
): DiffImpactArtifact {
	const totals = { added: 0, modified: 0, deleted: 0, renamed: 0 };
	for (const file of prepared.inventory) totals[file.status] += 1;
	const testFiles = semantic.files
		.filter((file) => file.test_artifact)
		.map((file) => file.new_path ?? file.old_path ?? "")
		.sort(ordinalCompare);
	const testSymbols = semantic.symbol_changes.filter((change) =>
		change.new?.test_artifact === true || change.old?.test_artifact === true
	).length;
	const skipped = prepared.inventory.filter((file) => !file.analyzed);
	const limitations = [
		"Only complete files changed between the two revisions are indexed; unchanged project context is outside this report.",
		"Relation facts are extracted from changed files only; workspace linkage and unchanged target files are not loaded for this bounded comparison.",
		"Test association is factual and limited to analyzed changed test paths or symbols classified as tests by the extractor.",
	];
	if (skipped.length > 0) {
		limitations.push(`${skipped.length} changed files were omitted because their language or content was not analyzable.`);
	}
	return {
		schemaVersion: 1,
		kind: "code-moniker.diff-impact",
		repository: redactCredentials(options.repository),
		project,
		ticket: options.ticket ?? null,
		revisions: {
			base: { requested: options.base, resolved: prepared.base },
			head: { requested: options.head, resolved: prepared.head },
		},
		scope: `${prepared.base}..${prepared.head}`,
		inventory: { files: prepared.inventory, totals },
		semantic,
		tests: { basis: "analyzed-path-and-extractor-kind", files: testFiles, symbolChanges: testSymbols },
		coverage: {
			corpus: "changed-files",
			changedFiles: prepared.inventory.length,
			analyzedFiles: prepared.inventory.length - skipped.length,
			skippedFiles: skipped.length,
			relations: "changed-file-extraction",
		},
		limitations,
	};
}

export function parseGitNameStatus(raw: string): GitChange[] {
	const fields = raw.split("\0");
	if (fields.at(-1) === "") fields.pop();
	const changes: GitChange[] = [];
	for (let index = 0; index < fields.length;) {
		const code = fields[index++];
		if (code === undefined) break;
		if (code.startsWith("R")) {
			const oldPath = fields[index++] ?? null;
			const newPath = fields[index++] ?? null;
			changes.push({ status: "renamed", oldPath, newPath, renameScore: Number(code.slice(1)), oldHunks: [], newHunks: [] });
			continue;
		}
		if (!matchesStatus(code, "A", "D", "M")) {
			throw new Error(`unsupported git diff status ${JSON.stringify(code)}`);
		}
		const path = fields[index++] ?? null;
		const status = code === "A" ? "added" : code === "D" ? "deleted" : "modified";
		changes.push({
			status,
			oldPath: status === "added" ? null : path,
			newPath: status === "deleted" ? null : path,
			renameScore: null,
			oldHunks: [],
			newHunks: [],
		});
	}
	return changes;
}

function parseHunks(patch: string): { old: Array<{ start: number; end: number }>; new: Array<{ start: number; end: number }> } {
	const old: Array<{ start: number; end: number }> = [];
	const next: Array<{ start: number; end: number }> = [];
	for (const line of patch.split("\n")) {
		const match = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@/.exec(line);
		if (match === null) continue;
		pushSpan(old, Number(match[1]), Number(match[2] ?? 1));
		pushSpan(next, Number(match[3]), Number(match[4] ?? 1));
	}
	return { old, new: next };
}

function pushSpan(spans: Array<{ start: number; end: number }>, start: number, count: number): void {
	if (count > 0) spans.push({ start, end: start + count - 1 });
}

export function sourceLanguageForPath(path: string): string | null {
	return ({
		".ts": "ts", ".tsx": "ts", ".js": "ts", ".jsx": "ts", ".mjs": "ts", ".cjs": "ts",
		".rs": "rs", ".java": "java", ".py": "python", ".pyi": "python", ".go": "go",
		".c": "c", ".h": "c", ".cs": "cs", ".sql": "sql",
		".plpgsql": "sql",
	} as Record<string, string>)[path.toLowerCase().endsWith(".sql.in") ? ".sql" : extname(path).toLowerCase()] ?? null;
}

function fileCategory(
	path: string,
	language: string | null,
	omission: string | null,
): DiffImpactFileInventory["category"] {
	const lowerPath = path.toLowerCase();
	const name = basename(lowerPath);
	if (omission === "binary content") return "binary";
	if (
		name === "cargo.lock" || name === "package-lock.json" || name === "pnpm-lock.yaml" ||
		name === "yarn.lock" || name === "composer.lock" || name === "poetry.lock" ||
		name.endsWith(".lock")
	) return "lockfile";
	if (name.endsWith(".schema.json") || lowerPath.split("/").includes("schema")) return "schema";
	if (
		name === "cargo.toml" || name === "package.json" || name === "pyproject.toml" ||
		name === "go.mod" || name === "go.sum" || name === "pom.xml" ||
		name === "build.gradle" || name === "build.gradle.kts"
	) return "manifest";
	if (
		lowerPath.startsWith("docs/") || [".md", ".mdx", ".rst", ".adoc"].includes(extname(lowerPath))
	) return "documentation";
	if ([".json", ".yaml", ".yml", ".toml", ".ini", ".conf"].includes(extname(lowerPath))) {
		return "configuration";
	}
	if (language !== null) return "source";
	return null;
}

function inferProjectName(repository: string): string {
	return basename(repository.replace(/\/$/, "")).replace(/\.git$/, "") || "diff-impact";
}

function shortRevision(revision: string): string {
	return revision.slice(0, 12);
}

function compareInventory(left: DiffImpactFileInventory, right: DiffImpactFileInventory): number {
	return ordinalCompare(left.newPath ?? left.oldPath ?? "", right.newPath ?? right.oldPath ?? "");
}

function compareChanges(left: GitChange, right: GitChange): number {
	return ordinalCompare(left.newPath ?? left.oldPath ?? "", right.newPath ?? right.oldPath ?? "");
}

function byUri(left: WorkspaceSourceDocumentDto, right: WorkspaceSourceDocumentDto): number {
	return ordinalCompare(left.uri, right.uri);
}

function sortJson(value: unknown): unknown {
	if (Array.isArray(value)) return value.map(sortJson);
	if (value === null || typeof value !== "object") return value;
	return Object.fromEntries(
		Object.entries(value as Record<string, unknown>)
			.sort(([left], [right]) => ordinalCompare(left, right))
			.map(([key, child]) => [key, sortJson(child)]),
	);
}

async function isDirectory(path: string): Promise<boolean> {
	try {
		return (await stat(path)).isDirectory();
	} catch {
		return false;
	}
}

function runProcess(
	command: string,
	args: string[],
	environment: Record<string, string | undefined> | undefined,
): Promise<string> {
	return new Promise((resolveProcess, rejectProcess) => {
		execFile(command, args, {
			env: { ...process.env, ...environment },
			encoding: "utf8",
			maxBuffer: GIT_OUTPUT_LIMIT,
		}, (error, stdout, stderr) => {
			if (error !== null) {
				rejectProcess(new Error(`${command} command failed: ${redactCredentials(stderr.trim() || error.message)}`));
				return;
			}
			resolveProcess(stdout);
		});
	});
}

function messageOf(error: unknown): string {
	return error instanceof Error ? error.message : String(error);
}

function redactCredentials(value: string): string {
	return value.replace(/([a-z][a-z0-9+.-]*:\/\/)([^/@\s]+)@/gi, "$1<redacted>@");
}

function ordinalCompare(left: string, right: string): number {
	return left < right ? -1 : left > right ? 1 : 0;
}

function matchesStatus(value: string, ...expected: string[]): boolean {
	return expected.includes(value);
}
