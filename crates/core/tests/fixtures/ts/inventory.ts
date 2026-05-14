import { Injectable, Inject, Logged, Tagged } from "./di";
import { Logger } from "./logger";
import { makeGuard } from "./guards";

export { format, parse } from "./format";
export * from "./errors";

export enum Stock {
	Empty = 0,
	Low = 1,
	Ample = 2,
}

export interface ItemRepo {
	get(id: string): Promise<Item | null>;
	put(item: Item): Promise<void>;
}

export interface Item {
	id: string;
	stock: Stock;
}

const makeId = (prefix: string, n: number): string => `${prefix}-${n}`;

const guard = makeGuard("inventory");

guard();

@Logged
export class BaseItem {
	constructor(protected readonly id: string) {}

	describe(): string {
		return makeId(this.id, 0).toUpperCase();
	}
}

@Logged
@Tagged("audit")
export class AuditedItem extends BaseItem {
	log(): string {
		return super.describe();
	}
}

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
export default class InventoryService {
	private readonly cache = new Map<string, Item>();
	protected next = 0;

	constructor(
		@Inject("repo") private readonly repo: ItemRepo,
		public readonly logger: Logger,
	) {}

	async restock(prefix: string, count: number): Promise<Item[]> {
		const out: Item[] = [];
		for (let i = 0; i < count; i += 1) {
			const id = makeId(prefix, this.advance());
			const item: Item = { id, stock: Stock.Ample };
			await this.repo.put(item);
			this.cache.set(id, item);
			out.push(item);
		}
		this.logger.info(`restocked ${out.length}`);
		return out;
	}

	async find(id: string): Promise<Item | null> {
		const hit = this.cache.get(id);
		if (hit) return hit;
		return this.repo.get(id);
	}

	private advance(): number {
		this.next += 1;
		return this.next;
	}
}
