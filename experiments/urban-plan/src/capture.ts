import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { NodeDaemonRuntime } from "@code-moniker/client/node";
import type { CodeMonikerClient, QueryCursor } from "@code-moniker/client";

import { snapshotFromTree } from "./layout.ts";
import type { IdentityEdge, IdentityTree } from "./scene.ts";

const PREFIX = process.env.URBAN_PLAN_PREFIX ?? "lang:rs/dir:crates";
const MIN_COUNT = Number(process.env.URBAN_PLAN_MIN_COUNT ?? "1");
const MAX_DEPTH = Number(process.env.URBAN_PLAN_MAX_DEPTH ?? "5");
const MIN_DEFS = Number(process.env.URBAN_PLAN_MIN_DEFS ?? "0");
const CHILD_LIMIT = Number(process.env.URBAN_PLAN_CHILD_LIMIT ?? "200");

const DISTRICT_KINDS = new Set(["dir", "module"]);
const BUILDING_KINDS = new Set([
	"struct",
	"enum",
	"trait",
	"type",
	"union",
	"class",
	"interface",
	"impl",
]);

const here = dirname(fileURLToPath(import.meta.url));
const outFile = join(here, "..", "public", "snapshot.json");

function workspaceRoot(): string {
	if (process.env.URBAN_PLAN_ROOT) {
		return resolve(process.env.URBAN_PLAN_ROOT);
	}
	let dir = process.cwd();
	for (let i = 0; i < 8; i += 1) {
		if (existsSync(join(dir, "Cargo.toml")) && existsSync(join(dir, "crates"))) {
			return dir;
		}
		const parent = dirname(dir);
		if (parent === dir) {
			break;
		}
		dir = parent;
	}
	return process.cwd();
}

async function main() {
	const root = workspaceRoot();
	const runtime = new NodeDaemonRuntime();
	const entry = runtime.findDaemon([root]);
	if (!entry) {
		throw new Error(
			`no daemon registered for ${root}; start one with: code-moniker query "workspace.status"`,
		);
	}
	const client = await runtime.connect(entry, {
		clientName: "urban-plan-capture",
		expectedWorkspaceRoots: [root],
	});
	try {
		const status = await client.workspace.status({ consistency: "stale_ok" });
		if (status.phase !== "ready" && status.phase !== "refreshing") {
			throw new Error(`workspace phase is ${status.phase}`);
		}
		const tree = await walk(client, {
			id: PREFIX,
			name: lastSegment(PREFIX),
			kind: "dir",
			defs: 0,
			hasChildren: true,
		}, 0);
		const edges = await collectCrateEdges(client);
		const snapshot = snapshotFromTree({
			generation: status.generation ?? null,
			prefix: PREFIX,
			tree,
			edges,
		});
		mkdirSync(dirname(outFile), { recursive: true });
		writeFileSync(outFile, `${JSON.stringify(snapshot, null, "\t")}\n`);
		console.log(
			`wrote ${outFile} generation=${snapshot.generation} districts=${snapshot.districts.length} buildings=${snapshot.buildings.length} streets=${snapshot.streets.length} roads=${snapshot.roads.length}`,
		);
	} finally {
		client.close();
	}
}

type Seed = {
	id: string;
	name: string;
	kind: string;
	defs: number;
	hasChildren: boolean;
};

async function walk(client: CodeMonikerClient, seed: Seed, depth: number): Promise<IdentityTree> {
	if (!seed.hasChildren || depth >= MAX_DEPTH || BUILDING_KINDS.has(seed.kind)) {
		return { id: seed.id, name: seed.name, kind: seed.kind, defs: seed.defs, children: [] };
	}
	if (!DISTRICT_KINDS.has(seed.kind) && depth > 0) {
		return { id: seed.id, name: seed.name, kind: seed.kind, defs: seed.defs, children: [] };
	}
	const page = await client.graph.children(
		seed.id,
		{ limit: CHILD_LIMIT },
		{ consistency: "stale_ok" },
	);
	const nested: Seed[] = [];
	const types: IdentityTree[] = [];
	for (const child of page.children) {
		const seedChild: Seed = {
			id: child.identity,
			name: child.name,
			kind: child.kind,
			defs: child.defs,
			hasChildren: child.has_children,
		};
		if (DISTRICT_KINDS.has(child.kind) && child.has_children && depth + 1 < MAX_DEPTH) {
			nested.push(seedChild);
			continue;
		}
		if (BUILDING_KINDS.has(child.kind) && child.defs >= MIN_DEFS) {
			types.push({
				id: child.identity,
				name: child.name,
				kind: child.kind,
				defs: Math.max(child.defs, 1),
				children: [],
			});
		}
	}
	const walked = await mapPool(nested, 3, (child) => walk(client, child, depth + 1));
	const children = [...walked, ...types];
	if (children.length === 0) {
		return { id: seed.id, name: seed.name, kind: seed.kind, defs: seed.defs, children: [] };
	}
	return {
		id: seed.id,
		name: seed.name,
		kind: seed.kind,
		defs: Math.max(
			seed.defs,
			children.reduce((sum, child) => sum + child.defs, 0),
		),
		children,
	};
}

async function collectCrateEdges(client: CodeMonikerClient): Promise<IdentityEdge[]> {
	const edges: IdentityEdge[] = [];
	let cursor: QueryCursor | null | undefined;
	do {
		const page = await client.graph.identity(
			PREFIX,
			{ minCount: MIN_COUNT },
			{ consistency: "stale_ok", limit: 80, cursor: cursor ?? null },
		);
		edges.push(
			...page.data.edges.map((edge) => ({
				source: edge.source,
				target: edge.target,
				kinds: edge.kinds,
				count: edge.count,
			})),
		);
		cursor = page.nextCursor;
	} while (cursor);
	return mergeEdges(edges);
}

function mergeEdges(edges: IdentityEdge[]): IdentityEdge[] {
	const byPair = new Map<string, IdentityEdge>();
	for (const edge of edges) {
		const key = `${edge.source}\0${edge.target}`;
		const current = byPair.get(key);
		if (!current) {
			byPair.set(key, { ...edge, kinds: [...edge.kinds] });
			continue;
		}
		current.count += edge.count;
		current.kinds = [...new Set([...current.kinds, ...edge.kinds])];
	}
	return [...byPair.values()];
}

async function mapPool<T, R>(items: T[], limit: number, fn: (item: T) => Promise<R>): Promise<R[]> {
	if (items.length === 0) {
		return [];
	}
	const result: R[] = new Array(items.length);
	let next = 0;
	async function worker() {
		while (next < items.length) {
			const index = next;
			next += 1;
			result[index] = await fn(items[index]!);
		}
	}
	await Promise.all(Array.from({ length: Math.min(limit, items.length) }, () => worker()));
	return result;
}

function lastSegment(identity: string): string {
	const part = identity.split("/").at(-1) ?? identity;
	const cut = part.indexOf(":");
	return cut >= 0 ? part.slice(cut + 1) : part;
}

main().catch((error) => {
	console.error(error instanceof Error ? error.message : error);
	process.exit(1);
});
