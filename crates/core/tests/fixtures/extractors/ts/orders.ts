import { randomUUID } from "crypto";
import type { ProductCatalog } from "./catalog";

// cm: def OrderStatus
export enum OrderStatus {
	Draft = "draft",
	Paid = "paid",
	Cancelled = "cancelled",
}

// cm: def Money
export type Money = {
	cents: number;
	currency: string;
};

// cm: def OrderLine
export interface OrderLine {
	sku: string;
	quantity: number;
	price: Money;
}

// cm: def Order
export class Order {
	public readonly id: string;
	private status: OrderStatus = OrderStatus.Draft;

	constructor(public readonly lines: OrderLine[]) {
		this.id = randomUUID();
	}

	// cm: def Order.total
	total(): Money {
		const cents = this.lines.reduce((sum, line) => sum + line.price.cents * line.quantity, 0);
		return { cents, currency: "USD" };
	}

	markPaid(): void {
		this.status = OrderStatus.Paid;
	}
}

// cm: def priceOrder
export async function priceOrder(catalog: ProductCatalog, skus: string[]): Promise<Order> {
	const lines = await Promise.all(
		skus.map(async (sku) => {
			const product = await catalog.findBySku(sku);
			return { sku, quantity: 1, price: product.price };
		}),
	);
	// cm: ref priceOrder.instantiates.Order
	return new Order(lines);
}
