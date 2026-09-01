import { spawn } from "node:child_process";
import { access, mkdir, mkdtemp, realpath, rm, stat } from "node:fs/promises";
import { constants as fsConstants } from "node:fs";
import { tmpdir } from "node:os";
import { basename, delimiter, extname, isAbsolute, join, resolve } from "node:path";

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
const GIT_PROBE_OUTPUT_LIMIT = 64 * 1024;
const GIT_PROBE_TIMEOUT_MS = 2_000;
const GIT_COMMAND_TIMEOUT_MS = 30_000;
const PROCESS_CLEANUP_TIMEOUT_MS = 1_000;
const GIT_BINARY_ENV = "CODE_MONIKER_GIT_BINARY";
const SUPPORTED_GIT_VERSION_RANGE = ">=2.22.0";
const GIT_RESOLUTION_RETRY_BACKOFF_MS = 1_000;
const gitResolutionFlights = new Map<string, Promise<ResolvedGitExecutable>>();
const gitResolutionRetryAfter = new Map<string, number>();

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
	schemaVersion: 2;
	kind: "code-moniker.diff-impact";
	repository: string;
	project: string;
	ticket: string | null;
	revisions: {
		base: { requested: string; resolved: string };
		head: { requested: string; resolved: string };
	};
	scope: string;
	runtimeDependencies: {
		git: GitRuntimeDiagnostic;
	};
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

export type GitRuntimeDiagnosticState =
	| "available"
	| "unavailable"
	| "incompatible"
	| "timed_out";

export interface GitRuntimeDiagnostic {
	state: GitRuntimeDiagnosticState;
	processScope: "client";
	resolutionSource: "explicit_configuration" | "inherited_path";
	executable: string | null;
	version: string | null;
	supportedRange: typeof SUPPORTED_GIT_VERSION_RANGE;
	compatible: boolean;
	failure: { category: string; message: string } | null;
	checkedAt: string;
	durationMs: number;
	repositoryState: "worktree" | "repository_only" | "not_repository" | "unavailable";
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
	gitDiagnostic: GitRuntimeDiagnostic;
	files: DiffImpactCompareFile[];
	baseDocuments: WorkspaceSourceDocumentDto[];
	headDocuments: WorkspaceSourceDocumentDto[];
	inventory: DiffImpactFileInventory[];
}

type SupervisorCandidates = () => readonly [string, ...string[]];

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
		const supervisorCandidates: SupervisorCandidates | undefined = process.platform === "win32"
			? () => options.binaryCandidates ?? runtime.resolveBinaryCandidates()
			: undefined;
		source = await GitRevisionSource.open(options, supervisorCandidates);
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
		private readonly client: GitClient,
		private readonly repositoryArguments: string[],
		private readonly temporaryDirectory?: string,
	) {}

	static async open(
		options: GitDiffImpactOptions,
		supervisorCandidates?: SupervisorCandidates,
	): Promise<GitRevisionSource> {
		const client = await GitClient.open(options, supervisorCandidates);
		const local = await isDirectory(options.repository);
		if (local) {
			const repository = resolve(options.repository);
			await client.probeRepository(repository, "worktree");
			return new GitRevisionSource(options, client, ["-C", repository]);
		}
		const temporaryDirectory = await mkdtemp(join(tmpdir(), "code-moniker-git-"));
		const gitDirectory = join(temporaryDirectory, "repository.git");
		const source = new GitRevisionSource(
			options,
			client,
			["--git-dir", gitDirectory],
			temporaryDirectory,
		);
		try {
			await source.git(["init", "--bare", gitDirectory], false);
			await client.probeRepository(gitDirectory, "repository_only", true);
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
			gitDiagnostic: this.diagnostic(),
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
		return (await this.client.runFastMetadata([
			...this.repositoryArguments,
			"rev-parse",
			"--verify",
			`${revision}^{commit}`,
		])).trim();
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
		return this.client.run(
			includeRepository ? [...this.repositoryArguments, ...args] : args,
			GIT_COMMAND_TIMEOUT_MS,
			GIT_OUTPUT_LIMIT,
		);
	}

	diagnostic(): GitRuntimeDiagnostic {
		return this.client.diagnostic();
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
		schemaVersion: 2,
		kind: "code-moniker.diff-impact",
		repository: redactCredentials(options.repository),
		project,
		ticket: options.ticket ?? null,
		revisions: {
			base: { requested: options.base, resolved: prepared.base },
			head: { requested: options.head, resolved: prepared.head },
		},
		scope: `${prepared.base}..${prepared.head}`,
		runtimeDependencies: {
			git: prepared.gitDiagnostic,
		},
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
		".ts": "ts", ".mts": "ts", ".cts": "ts", ".tsx": "tsx",
		".js": "js", ".mjs": "js", ".cjs": "js", ".jsx": "jsx",
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

class GitClient {
	private constructor(
		private readonly executable: string,
		private readonly environment: NodeJS.ProcessEnv,
		private readonly supervisorCandidates: SupervisorCandidates | undefined,
		private readonly source: GitRuntimeDiagnostic["resolutionSource"],
		private currentDiagnostic: GitRuntimeDiagnostic,
	) {}

	static async open(
		options: GitDiffImpactOptions,
		supervisorCandidates?: SupervisorCandidates,
	): Promise<GitClient> {
		const started = performance.now();
		const checkedAt = new Date().toISOString();
		const environment = gitEnvironment(options.environment);
		const source = options.gitBinary !== undefined || environment[GIT_BINARY_ENV] !== undefined
			? "explicit_configuration"
			: "inherited_path";
		let resolved: ResolvedGitExecutable;
		try {
			resolved = await resolveGitExecutableBounded(
				options.gitBinary,
				environment,
				GIT_PROBE_TIMEOUT_MS,
			);
		} catch (error) {
			throw gitResolutionDiagnosticError(
				error,
				source,
				checkedAt,
				performance.now() - started,
			);
		}
		let versionOutput: string;
		try {
			const remaining = Math.max(1, Math.ceil(
				GIT_PROBE_TIMEOUT_MS - (performance.now() - started),
			));
			versionOutput = await runProcess(
				resolved.executable,
				["--version"],
				environment,
				remaining,
				GIT_PROBE_OUTPUT_LIMIT,
				supervisorCandidates,
			);
		} catch (error) {
			throw gitDiagnosticError(error, resolved, checkedAt, performance.now() - started);
		}
		const version = parseGitVersion(versionOutput);
		if (version === null) {
			throw new GitRuntimeError("unavailable", {
				state: "unavailable",
				processScope: "client",
				resolutionSource: resolved.source,
				executable: resolved.executable,
				version: versionOutput.trim() || null,
				supportedRange: SUPPORTED_GIT_VERSION_RANGE,
				compatible: false,
				failure: {
					category: "malformed_version",
					message: `Git returned an unrecognized version: ${JSON.stringify(versionOutput.trim())}`,
				},
				checkedAt,
				durationMs: Math.ceil(performance.now() - started),
				repositoryState: "unavailable",
			}, `Git returned an unrecognized version: ${JSON.stringify(versionOutput.trim())}`);
		}
		const compatible = compareVersion(version, [2, 22, 0]) >= 0;
		const diagnostic: GitRuntimeDiagnostic = {
			state: compatible ? "available" : "incompatible",
			processScope: "client",
			resolutionSource: resolved.source,
			executable: resolved.executable,
			version: version.text,
			supportedRange: SUPPORTED_GIT_VERSION_RANGE,
			compatible,
			failure: compatible ? null : {
				category: "incompatible_version",
				message: `Git ${version.text} is outside the supported range ${SUPPORTED_GIT_VERSION_RANGE}`,
			},
			checkedAt,
			durationMs: Math.ceil(performance.now() - started),
			repositoryState: "unavailable",
		};
		if (!compatible) {
			throw new GitRuntimeError(
				"incompatible",
				diagnostic,
				`Git ${version.text} is outside the supported range ${SUPPORTED_GIT_VERSION_RANGE}`,
			);
		}
		return new GitClient(
			resolved.executable,
			environment,
			supervisorCandidates,
			resolved.source,
			diagnostic,
		);
	}

	async probeRepository(
		repository: string,
		expected: "worktree" | "repository_only",
		bare = false,
	): Promise<void> {
		const started = performance.now();
		const prefix = bare ? ["--git-dir", repository] : ["-C", repository];
		let output: string;
		try {
			output = await this.run(
				[...prefix, "rev-parse", "--is-inside-work-tree", "--is-bare-repository"],
				GIT_PROBE_TIMEOUT_MS,
				GIT_PROBE_OUTPUT_LIMIT,
			);
		} catch (error) {
			const failure = error instanceof GitRuntimeError
				? { ...(error.diagnostic.failure ?? processFailure(error)) }
				: processFailure(error);
			const notRepository = failure.category === "command_failed"
				&& /not a git repository/i.test(failure.message);
			if (notRepository) failure.category = "not_repository";
			const state = notRepository
				? "unavailable"
				: error instanceof GitRuntimeError ? error.state : processFailureState(error);
			this.currentDiagnostic = {
				...this.currentDiagnostic,
				state,
				checkedAt: new Date().toISOString(),
				durationMs: Math.ceil(performance.now() - started),
				repositoryState: notRepository ? "not_repository" : "unavailable",
				failure,
			};
			throw new GitRuntimeError(
				state,
				this.diagnostic(),
				messageOf(error),
			);
		}
		const repositoryFlags = output.trim().split(/\r?\n/);
		const state = repositoryFlags.length === 2
			&& repositoryFlags[0] === "true"
			&& repositoryFlags[1] === "false"
			? "worktree"
			: repositoryFlags.length === 2
				&& repositoryFlags[0] === "false"
				&& repositoryFlags[1] === "true"
					? "repository_only"
					: null;
		if (state === null) {
			const message = `Git repository probe returned unexpected output: ${JSON.stringify(output.trim())}`;
			this.currentDiagnostic = {
				...this.currentDiagnostic,
				state: "unavailable",
				checkedAt: new Date().toISOString(),
				durationMs: Math.ceil(performance.now() - started),
				repositoryState: "unavailable",
				failure: { category: "malformed_output", message },
			};
			throw new GitRuntimeError("unavailable", this.diagnostic(), message);
		}
		this.currentDiagnostic = {
			...this.currentDiagnostic,
			checkedAt: new Date().toISOString(),
			durationMs: Math.ceil(performance.now() - started),
			repositoryState: state,
			failure: state === expected ? null : {
				category: "not_repository",
				message: `repository ${repository} is ${state}, expected ${expected}`,
			},
		};
		if (state !== expected) {
			this.currentDiagnostic.state = "unavailable";
			throw new GitRuntimeError(
				"unavailable",
				this.diagnostic(),
				`repository ${repository} is ${state}, expected ${expected}`,
			);
		}
	}

	async run(args: string[], timeoutMs: number, maxBuffer: number): Promise<string> {
		try {
			return await runProcess(
				this.executable,
				args,
				this.environment,
				timeoutMs,
				maxBuffer,
				this.supervisorCandidates,
			);
		} catch (error) {
			const failure = processFailure(error);
			if (failure.category === "command_failed") {
				throw new GitRuntimeError(
					this.currentDiagnostic.state,
					{ ...this.diagnostic(), failure },
					messageOf(error),
				);
			}
			const state = processFailureState(error);
			this.currentDiagnostic = {
				...this.currentDiagnostic,
				state,
				failure,
				checkedAt: new Date().toISOString(),
			};
			throw new GitRuntimeError(state, this.diagnostic(), messageOf(error));
		}
	}

	runFastMetadata(args: string[]): Promise<string> {
		return this.run(args, GIT_PROBE_TIMEOUT_MS, GIT_PROBE_OUTPUT_LIMIT);
	}

	diagnostic(): GitRuntimeDiagnostic {
		return { ...this.currentDiagnostic, resolutionSource: this.source };
	}
}

export class GitRuntimeError extends Error {
	constructor(
		readonly state: GitRuntimeDiagnosticState,
		readonly diagnostic: GitRuntimeDiagnostic,
		message: string,
	) {
		super(redactCredentials(message));
		this.name = "GitRuntimeError";
	}
}

interface ResolvedGitExecutable {
	executable: string;
	source: GitRuntimeDiagnostic["resolutionSource"];
}

async function resolveGitExecutable(
	explicitOption: string | undefined,
	environment: NodeJS.ProcessEnv,
): Promise<ResolvedGitExecutable> {
	const explicit = explicitOption ?? environment[GIT_BINARY_ENV];
	if (explicit !== undefined) {
		if (explicit.length === 0) {
			throw unavailableGitError(
				"explicit_configuration",
				`${GIT_BINARY_ENV} and gitBinary must not be empty`,
				"invalid_configuration",
			);
		}
		if (!isAbsolute(explicit)) {
			throw unavailableGitError(
				"explicit_configuration",
				`${GIT_BINARY_ENV} and gitBinary must name an absolute executable path`,
				"invalid_configuration",
			);
		}
		try {
			return {
				executable: await validateGitExecutable(explicit),
				source: "explicit_configuration",
			};
		} catch (error) {
			throw unavailableGitError(
				"explicit_configuration",
				`configured Git executable ${explicit} is unavailable: ${messageOf(error)}`,
				processErrorCategory(error),
				explicit,
			);
		}
	}
	const pathValue = inheritedPath(environment);
	if (pathValue === undefined) {
		throw unavailableGitError(
			"inherited_path",
			"cannot resolve Git because PATH is unavailable",
			"path_unavailable",
		);
	}
	const executableName = process.platform === "win32" ? "git.exe" : "git";
	let permissionFailure: { candidate: string; error: unknown } | undefined;
	for (const directory of pathValue.split(delimiter)) {
		if (directory.length === 0) continue;
		const candidate = join(directory, executableName);
		try {
			return {
				executable: await validateGitExecutable(candidate),
				source: "inherited_path",
			};
		} catch (error) {
			if (processErrorCategory(error) === "permission_denied" && permissionFailure === undefined) {
				permissionFailure = { candidate, error };
			}
			// Continue through the inherited PATH only; no registry or standard-path fallback.
		}
	}
	if (permissionFailure !== undefined) {
		throw unavailableGitError(
			"inherited_path",
			`Git candidate ${permissionFailure.candidate} is not executable: ${messageOf(permissionFailure.error)}`,
			"permission_denied",
			permissionFailure.candidate,
		);
	}
	throw unavailableGitError("inherited_path", `Git was not found on the inherited PATH`);
}

async function validateGitExecutable(candidate: string): Promise<string> {
	const details = await stat(candidate);
	if (!details.isFile()) {
		throw new ProcessExecutionError(`${candidate} is not a file`, false, "not_found");
	}
	if (process.platform !== "win32") await access(candidate, fsConstants.X_OK);
	return realpath(candidate);
}

function inheritedPath(environment: NodeJS.ProcessEnv): string | undefined {
	const key = Object.keys(environment).find((candidate) => candidate.toLowerCase() === "path");
	return key === undefined ? undefined : environment[key];
}

function unavailableGitError(
	source: GitRuntimeDiagnostic["resolutionSource"],
	message: string,
	category = "not_found",
	executable: string | null = null,
): GitRuntimeError {
	return new GitRuntimeError("unavailable", {
		state: "unavailable",
		processScope: "client",
		resolutionSource: source,
		executable,
		version: null,
		supportedRange: SUPPORTED_GIT_VERSION_RANGE,
		compatible: false,
		failure: { category, message },
		checkedAt: new Date().toISOString(),
		durationMs: 0,
		repositoryState: "unavailable",
	}, message);
}

function processErrorCategory(error: unknown): string {
	if (error instanceof ProcessExecutionError) return error.category;
	const code = error instanceof Error && "code" in error
		? (error as NodeJS.ErrnoException).code
		: undefined;
	if (code === "ENOENT") return "not_found";
	if (code === "EACCES" || code === "EPERM") return "permission_denied";
	return "command_failed";
}

function gitDiagnosticError(
	error: unknown,
	resolved: ResolvedGitExecutable,
	checkedAt: string,
	durationMs: number,
): GitRuntimeError {
	const state = processFailureState(error);
	return new GitRuntimeError(state, {
		state,
		processScope: "client",
		resolutionSource: resolved.source,
		executable: resolved.executable,
		version: null,
		supportedRange: SUPPORTED_GIT_VERSION_RANGE,
		compatible: false,
		failure: processFailure(error),
		checkedAt,
		durationMs: Math.ceil(durationMs),
		repositoryState: "unavailable",
	}, messageOf(error));
}

function gitResolutionDiagnosticError(
	error: unknown,
	source: GitRuntimeDiagnostic["resolutionSource"],
	checkedAt: string,
	durationMs: number,
): GitRuntimeError {
	if (error instanceof GitRuntimeError) {
		return new GitRuntimeError(error.state, {
			...error.diagnostic,
			checkedAt,
			durationMs: Math.ceil(durationMs),
		}, error.message);
	}
	const state = processFailureState(error);
	return new GitRuntimeError(state, {
		state,
		processScope: "client",
		resolutionSource: source,
		executable: null,
		version: null,
		supportedRange: SUPPORTED_GIT_VERSION_RANGE,
		compatible: false,
		failure: processFailure(error),
		checkedAt,
		durationMs: Math.ceil(durationMs),
		repositoryState: "unavailable",
	}, messageOf(error));
}

function withResolutionDeadline<T>(operation: Promise<T>, timeoutMs: number): Promise<T> {
	return new Promise((resolveOperation, rejectOperation) => {
		let settled = false;
		const timer = setTimeout(() => {
			if (settled) return;
			settled = true;
			rejectOperation(new ProcessExecutionError(
				`Git executable resolution timed out after ${timeoutMs} ms`,
				true,
				"timed_out",
			));
		}, timeoutMs);
		operation.then(
			(value) => {
				if (settled) return;
				settled = true;
				clearTimeout(timer);
				resolveOperation(value);
			},
			(error: unknown) => {
				if (settled) return;
				settled = true;
				clearTimeout(timer);
				rejectOperation(error);
			},
		);
	});
}

async function resolveGitExecutableBounded(
	explicitOption: string | undefined,
	environment: NodeJS.ProcessEnv,
	timeoutMs: number,
): Promise<ResolvedGitExecutable> {
	const key = JSON.stringify([
		process.platform,
		explicitOption ?? environment[GIT_BINARY_ENV] ?? null,
		inheritedPath(environment) ?? null,
	]);
	const retryAfter = gitResolutionRetryAfter.get(key);
	if (retryAfter !== undefined && Date.now() < retryAfter) {
		throw new ProcessExecutionError(
			"Git executable resolution is in timeout backoff",
			true,
			"timed_out",
		);
	}
	if (retryAfter !== undefined) gitResolutionRetryAfter.delete(key);
	let operation = gitResolutionFlights.get(key);
	if (operation === undefined) {
		operation = resolveGitExecutable(explicitOption, environment);
		gitResolutionFlights.set(key, operation);
		void operation.then(
			() => gitResolutionFlights.delete(key),
			() => gitResolutionFlights.delete(key),
		);
	}
	try {
		const resolved = await withResolutionDeadline(operation, timeoutMs);
		gitResolutionRetryAfter.delete(key);
		return resolved;
	} catch (error) {
		if (error instanceof ProcessExecutionError && error.timedOut) {
			gitResolutionRetryAfter.set(key, Date.now() + GIT_RESOLUTION_RETRY_BACKOFF_MS);
		}
		throw error;
	}
}

function processFailureState(error: unknown): GitRuntimeDiagnosticState {
	return error instanceof ProcessExecutionError && error.timedOut ? "timed_out" : "unavailable";
}

function processFailure(error: unknown): { category: string; message: string } {
	return {
		category: error instanceof ProcessExecutionError
			? error.category
			: "process_failed",
		message: redactCredentials(messageOf(error)),
	};
}

function parseGitVersion(output: string): { text: string; parts: [number, number, number] } | null {
	const match = /^git version (\d+)\.(\d+)\.(\d+)(?:[.\s]|$)/i.exec(output.trim());
	if (match === null) return null;
	return {
		text: output.trim(),
		parts: [Number(match[1]), Number(match[2]), Number(match[3])],
	};
}

function compareVersion(
	left: { parts: [number, number, number] },
	right: [number, number, number],
): number {
	for (let index = 0; index < right.length; index += 1) {
		const comparison = left.parts[index] - right[index];
		if (comparison !== 0) return comparison;
	}
	return 0;
}

class ProcessExecutionError extends Error {
	constructor(
		message: string,
		readonly timedOut: boolean,
		readonly category: string,
	) {
		super(message);
		this.name = "ProcessExecutionError";
	}
}

function gitEnvironment(overrides: Record<string, string | undefined> | undefined): NodeJS.ProcessEnv {
	const environment: NodeJS.ProcessEnv = { ...process.env, ...overrides };
	const canonicalPathKey = process.platform === "win32" ? "Path" : "PATH";
	const overriddenPath = overrides?.[canonicalPathKey]
		?? Object.entries(overrides ?? {}).find(([key]) => key.toLowerCase() === "path")?.[1];
	const inherited = overriddenPath ?? inheritedPath(process.env);
	for (const key of Object.keys(environment)) {
		if (key.toLowerCase() === "path") delete environment[key];
	}
	if (inherited !== undefined) environment[canonicalPathKey] = inherited;
	environment.GIT_OPTIONAL_LOCKS = "0";
	environment.LC_ALL = "C";
	environment.LANG = "C";
	return environment;
}

function runProcess(
	command: string,
	args: string[],
	environment: NodeJS.ProcessEnv,
	timeoutMs: number,
	maxBuffer: number,
	supervisorCandidates?: SupervisorCandidates,
): Promise<string> {
	if (process.platform !== "win32") {
		return runDirectProcess(command, args, environment, timeoutMs, maxBuffer);
	}
	if (supervisorCandidates === undefined) {
		return Promise.reject(new ProcessExecutionError(
			"the packaged Code Moniker Git supervisor is unavailable on Windows",
			false,
			"supervisor_incompatible",
		));
	}
	return runWindowsSupervisedProcess(
		supervisorCandidates(),
		command,
		args,
		environment,
		timeoutMs,
		maxBuffer,
	);
}

function runDirectProcess(
	command: string,
	args: string[],
	environment: NodeJS.ProcessEnv,
	timeoutMs: number,
	maxBuffer: number,
): Promise<string> {
	return new Promise((resolveProcess, rejectProcess) => {
		const child = spawn(command, args, {
			detached: process.platform !== "win32",
			env: environment,
			stdio: ["ignore", "pipe", "pipe"],
			windowsHide: true,
		});
		const stdout: Buffer[] = [];
		const stderr: Buffer[] = [];
		let stdoutBytes = 0;
		let stderrBytes = 0;
		let timedOut = false;
		let outputExceeded = false;
		let settled = false;
		let cleanupStarted = false;
		let cleanupTimer: NodeJS.Timeout | undefined;
		let terminationUnconfirmed = false;
		let cleanupComplete = false;
		let closeObserved = false;
		let exitCode: number | null | undefined;
		let exitSignal: NodeJS.Signals | null | undefined;
		let closeCode: number | null | undefined;
		let closeSignal: NodeJS.Signals | null | undefined;
		const timer = setTimeout(() => {
			timedOut = true;
			stopProcess();
		}, timeoutMs);
		const collect = (chunks: Buffer[], stream: "stdout" | "stderr") => (chunk: Buffer) => {
			if (stream === "stdout") stdoutBytes += chunk.length;
			else stderrBytes += chunk.length;
			if (stdoutBytes > maxBuffer || stderrBytes > maxBuffer) {
				outputExceeded = true;
				stopProcess();
				return;
			}
			chunks.push(chunk);
		};
		child.stdout.on("data", collect(stdout, "stdout"));
		child.stderr.on("data", collect(stderr, "stderr"));
		child.once("error", (error) => finish(error));
		child.once("exit", (code, signal) => {
			exitCode = code;
			exitSignal = signal;
			if (timedOut || outputExceeded) {
				stopProcess();
				return;
			}
		});
		child.once("close", (code, signal) => {
			closeObserved = true;
			closeCode = code;
			closeSignal = signal;
			if (!cleanupStarted || cleanupComplete) finish(null, code, signal);
		});

		function stopProcess() {
			if (cleanupStarted) return;
			cleanupStarted = true;
			child.stdout.destroy();
			child.stderr.destroy();
			void terminateProcess(child.pid, child.kill.bind(child)).finally(() => {
				cleanupComplete = true;
				if (closeObserved) finish(null, closeCode, closeSignal);
			});
			cleanupTimer = setTimeout(() => {
				terminationUnconfirmed = true;
				finish(null, exitCode, exitSignal);
			}, PROCESS_CLEANUP_TIMEOUT_MS);
		}

		function finish(error: Error | null, code?: number | null, signal?: NodeJS.Signals | null) {
			if (settled) return;
			settled = true;
			clearTimeout(timer);
			if (cleanupTimer !== undefined) clearTimeout(cleanupTimer);
			const stdoutText = Buffer.concat(stdout).toString("utf8");
			const stderrText = Buffer.concat(stderr).toString("utf8").trim();
			if (timedOut) {
				rejectProcess(new ProcessExecutionError(
					`${command} command timed out after ${timeoutMs} ms${terminationUnconfirmed ? "; process termination was not observed" : ""}`,
					true,
					"timed_out",
				));
				return;
			}
			if (outputExceeded) {
				rejectProcess(new ProcessExecutionError(
					`${command} command exceeded the ${maxBuffer}-byte output limit${terminationUnconfirmed ? "; process termination was not observed" : ""}`,
					false,
					"output_limit",
				));
				return;
			}
			if (error !== null || code !== 0) {
				const detail = stderrText || error?.message || `exited with code ${code} signal ${signal}`;
				rejectProcess(new ProcessExecutionError(
					`${command} command failed: ${redactCredentials(detail)}`,
					false,
					error === null ? "command_failed" : processErrorCategory(error),
				));
				return;
			}
			resolveProcess(stdoutText);
		}
	});
}

interface GitSupervisorEnvelope {
	protocolVersion: number;
	executable: string;
	outcome: "ok" | "error";
	stdoutBase64: string | null;
	category: string | null;
	message: string | null;
}

async function runWindowsSupervisedProcess(
	supervisorCandidates: readonly [string, ...string[]],
	command: string,
	args: string[],
	environment: NodeJS.ProcessEnv,
	timeoutMs: number,
	maxBuffer: number,
): Promise<string> {
	let response: string | undefined;
	for (const candidate of supervisorCandidates) {
		try {
			response = await runDirectProcess(
				candidate,
				[
					"__git-runtime",
					"--executable", command,
					"--timeout-ms", String(timeoutMs),
					"--output-limit", String(maxBuffer),
					"--",
					...args,
				],
				environment,
				timeoutMs + PROCESS_CLEANUP_TIMEOUT_MS,
				supervisorEnvelopeLimit(maxBuffer),
			);
			break;
		} catch (error) {
			if (error instanceof ProcessExecutionError && error.category === "not_found") continue;
			if (error instanceof ProcessExecutionError && error.category === "timed_out") throw error;
			throw new ProcessExecutionError(
				`Code Moniker Git supervisor is incompatible or failed: ${messageOf(error)}`,
				false,
				"supervisor_incompatible",
			);
		}
	}
	if (response === undefined) {
		throw new ProcessExecutionError(
			`Code Moniker Git supervisor was not found (tried: ${supervisorCandidates.join(", ")})`,
			false,
			"supervisor_unavailable",
		);
	}
	const envelope = parseGitSupervisorEnvelope(response, command, maxBuffer);
	if (envelope.outcome === "error") {
		throw new ProcessExecutionError(
			envelope.message ?? "supervised Git command failed without a message",
			envelope.category === "timed_out",
			envelope.category ?? "supervisor_protocol_error",
		);
	}
	if (envelope.category !== null || envelope.message !== null) {
		throw supervisorProtocolError("successful response contains failure fields");
	}
	return decodeSupervisorOutput(envelope.stdoutBase64, "stdout", maxBuffer).toString("utf8");
}

function parseGitSupervisorEnvelope(
	response: string,
	executable: string,
	maxBuffer: number,
): GitSupervisorEnvelope {
	let value: unknown;
	try {
		value = JSON.parse(response);
	} catch {
		throw supervisorProtocolError("response is not complete JSON");
	}
	if (value === null || typeof value !== "object" || Array.isArray(value)) {
		throw supervisorProtocolError("response is not an object");
	}
	const envelope = value as Partial<GitSupervisorEnvelope>;
	if (envelope.protocolVersion !== 1) throw supervisorProtocolError("unsupported protocol version");
	if (envelope.executable !== executable) throw supervisorProtocolError("executable identity mismatch");
	if (envelope.outcome !== "ok" && envelope.outcome !== "error") throw supervisorProtocolError("invalid outcome");
	if (envelope.outcome === "error") {
		if (envelope.stdoutBase64 !== null) {
			throw supervisorProtocolError("failure response contains output fields");
		}
		if (typeof envelope.category !== "string" || typeof envelope.message !== "string") {
			throw supervisorProtocolError("failure response lacks category or message");
		}
	} else {
		decodeSupervisorOutput(envelope.stdoutBase64, "stdout", maxBuffer);
	}
	return envelope as GitSupervisorEnvelope;
}

function decodeSupervisorOutput(value: unknown, stream: string, maxBuffer: number): Buffer {
	if (typeof value !== "string" || !/^(?:[A-Za-z0-9+/]{4})*(?:[A-Za-z0-9+/]{2}==|[A-Za-z0-9+/]{3}=)?$/.test(value)) {
		throw supervisorProtocolError(`${stream} is not canonical base64`);
	}
	const decoded = Buffer.from(value, "base64");
	if (decoded.toString("base64") !== value) throw supervisorProtocolError(`${stream} is not canonical base64`);
	if (decoded.length > maxBuffer) throw supervisorProtocolError(`${stream} exceeds the declared limit`);
	return decoded;
}

function supervisorEnvelopeLimit(maxBuffer: number): number {
	return (Math.ceil(maxBuffer / 3) * 4) + (64 * 1024);
}

function supervisorProtocolError(detail: string): ProcessExecutionError {
	return new ProcessExecutionError(
		`Code Moniker Git supervisor protocol error: ${detail}`,
		false,
		"supervisor_incompatible",
	);
}

async function terminateProcess(
	pid: number | undefined,
	kill: (signal?: NodeJS.Signals) => boolean,
): Promise<void> {
	if (pid !== undefined && process.platform !== "win32") {
		try {
			process.kill(-pid, "SIGKILL");
		} catch {
			// The process may have exited between the timer and termination.
		}
	}
	try {
		kill("SIGKILL");
	} catch {
		// Exit observation below remains the source of truth.
	}
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
