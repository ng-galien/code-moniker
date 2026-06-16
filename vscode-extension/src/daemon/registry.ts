import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";

import { DaemonRegistryEntry } from "./model";

// The daemon records each running instance as a JSON file under a shared registry
// directory (crates/query/src/discovery.rs). We read it directly rather than
// shelling out, and match the opened workspace by canonical (realpath) root.

export function registryDir(): string {
	return path.join(os.tmpdir(), "code-moniker-daemons");
}

export function listDaemons(): DaemonRegistryEntry[] {
	const dir = registryDir();
	let names: string[];
	try {
		names = fs.readdirSync(dir);
	} catch {
		return [];
	}
	const entries: DaemonRegistryEntry[] = [];
	for (const name of names) {
		if (!name.endsWith(".json")) {
			continue;
		}
		try {
			const raw = fs.readFileSync(path.join(dir, name), "utf8");
			entries.push(JSON.parse(raw) as DaemonRegistryEntry);
		} catch {
			// Skip partially-written or corrupt entries.
		}
	}
	return entries.sort((a, b) => a.workspace_root.localeCompare(b.workspace_root));
}

export function findDaemonForRoots(roots: string[]): DaemonRegistryEntry | undefined {
	const wanted = new Set(roots.map(canonical));
	return listDaemons().find((entry) =>
		entry.workspace_roots.some((root) => wanted.has(canonical(root))),
	);
}

export function entryMatchesRoots(entry: DaemonRegistryEntry, roots: string[]): boolean {
	const wanted = new Set(roots.map(canonical));
	return entry.workspace_roots.some((root) => wanted.has(canonical(root)));
}

function canonical(p: string): string {
	try {
		return fs.realpathSync(p);
	} catch {
		return path.resolve(p);
	}
}
