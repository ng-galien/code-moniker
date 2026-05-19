import { Injectable, Inject, Logged, Tagged } from "./di";
import { Logger } from "./logger";
import { makeGuard } from "./guards";

export { format, parse } from "./format";
// cm: ref wildcard errors reexport
export * from "./errors";

// cm: def stock enum
export enum Stock {
	Empty = 0,
	Low = 1,
	Ample = 2,
}

// cm: def item repo interface
export interface ItemRepo {
	get(id: string): Promise<Item | null>;
	put(item: Item): Promise<void>;
}

// cm: def item interface
export interface Item {
	id: string;
	stock: Stock;
}

// cm: def make id helper
const makeId = (prefix: string, n: number): string => `${prefix}-${n}`;

const guard = makeGuard("inventory");

guard();

@Logged
// cm: def base item class
export class BaseItem {
	constructor(protected readonly id: string) {}

	describe(): string {
		// cm: ref describe calls make id
		return makeId(this.id, 0).toUpperCase();
	}
}

@Logged
@Tagged("audit")
// cm: def audited item class
export class AuditedItem extends BaseItem {
	log(): string {
		return super.describe();
	}
}

// cm: def memory repo class
export class MemoryRepo implements ItemRepo {
	private items: Map<string, Item> = new Map();

	async get(id: string): Promise<Item | null> {
		return this.items.get(id) ?? null;
	}

	async put(item: Item): Promise<void> {
		this.items.set(item.id, item);
	}
}

// code-moniker: ignore[ts.class.name-pascalcase]
@Injectable
// cm: def default inventory service
export default class InventoryService {
	private readonly cache = new Map<string, Item>();
	protected next = 0;

	constructor(
		@Inject("repo") private readonly repo: ItemRepo,
		public readonly logger: Logger,
	) {}

	// cm: def restock method
	async restock(prefix: string, count: number): Promise<Item[]> {
		const out: Item[] = [];
		for (let i = 0; i < count; i += 1) {
			// cm: ref restock calls make id
			const id = makeId(prefix, this.advance());
			const item: Item = { id, stock: Stock.Ample };
			// cm: ref restock calls repo put
			await this.repo.put(item);
			this.cache.set(id, item);
			out.push(item);
		}
		// cm: ref restock calls logger info
		this.logger.info(`restocked ${out.length}`);
		return out;
	}

	// cm: def find method
	async find(id: string): Promise<Item | null> {
		const hit = this.cache.get(id);
		if (hit) return hit;
		return this.repo.get(id);
	}

	// cm: def advance method
	private advance(): number {
		this.next += 1;
		return this.next;
	}
}
