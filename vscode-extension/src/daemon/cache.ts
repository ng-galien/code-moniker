import { DaemonSession } from "./session";

interface CacheEntry {
	generation: number | undefined;
	pending?: Promise<unknown>;
	value?: unknown;
}

// Query-result cache keyed on the workspace generation carried by every
// daemon response. A cached value is valid exactly as long as the session's
// last-seen generation matches the one the value was produced under — a
// `refreshed` event or any response from a newer snapshot invalidates it
// implicitly, with no explicit flush to keep in sync. In-flight loads are
// shared, so bursts of identical queries collapse into one RPC.
export class GenerationCache {
	private readonly entries = new Map<string, CacheEntry>();

	constructor(private readonly session: DaemonSession) {}

	async fetch<T>(key: string, load: () => Promise<T>): Promise<T> {
		const generation = this.session.generation;
		const entry = this.entries.get(key);
		if (entry && generation !== undefined && entry.generation === generation) {
			if (entry.pending) {
				return entry.pending as Promise<T>;
			}
			return entry.value as T;
		}
		const pending = load();
		this.entries.set(key, { generation, pending });
		try {
			const value = await pending;
			this.entries.set(key, { generation: this.session.generation, value });
			return value;
		} catch (error) {
			this.entries.delete(key);
			throw error;
		}
	}

	clear(): void {
		this.entries.clear();
	}
}
