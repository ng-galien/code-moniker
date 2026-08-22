import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { NodeDaemonRuntime } from "@code-moniker/client/node";

import { snapshotFromIdentityGraph, type IdentityEdge, type IdentityNode } from "./scene.ts";

const PREFIX = process.env.URBAN_PLAN_PREFIX ?? "lang:rs/dir:crates";
const MIN_COUNT = Number(process.env.URBAN_PLAN_MIN_COUNT ?? "5");
const MAX_ROADS = Number(process.env.URBAN_PLAN_MAX_ROADS ?? "24");

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
		const nodes: IdentityNode[] = [];
		const edges: IdentityEdge[] = [];
		let cursor = undefined as string | number | null | undefined;
		do {
			const page = await client.graph.identity(
				PREFIX,
				{ minCount: MIN_COUNT },
				{ consistency: "stale_ok", limit: 80, cursor: cursor ?? null },
			);
			nodes.push(
				...page.data.nodes.map((node) => ({
					identity: node.identity,
					kind: node.kind,
					name: node.name,
					defs: node.defs,
					has_children: node.has_children,
				})),
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
		const snapshot = snapshotFromIdentityGraph({
			generation: status.generation,
			prefix: PREFIX,
			nodes: dedupeNodes(nodes),
			edges: mergeEdges(edges),
			maxRoads: MAX_ROADS,
		});
		mkdirSync(dirname(outFile), { recursive: true });
		writeFileSync(outFile, `${JSON.stringify(snapshot, null, "\t")}\n`);
		console.log(
			`wrote ${outFile} generation=${snapshot.generation} buildings=${snapshot.buildings.length} roads=${snapshot.roads.length}`,
		);
	} finally {
		client.close();
	}
}

function dedupeNodes(nodes: IdentityNode[]): IdentityNode[] {
	const byId = new Map<string, IdentityNode>();
	for (const node of nodes) {
		byId.set(node.identity, node);
	}
	return [...byId.values()];
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

main().catch((error) => {
	console.error(error instanceof Error ? error.message : error);
	process.exit(1);
});
