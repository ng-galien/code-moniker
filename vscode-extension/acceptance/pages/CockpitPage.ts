import { expect, type Frame, type Locator, type Page } from "@playwright/test";

export class CockpitPage {
	private frame?: Frame;

	constructor(private readonly page: Page) {}

	async waitUntilOpen(): Promise<void> {
		this.frame = await this.findFrame();
		await expect(this.search).toBeVisible({ timeout: 10_000 });
	}

	get search(): Locator {
		return this.requireFrame().getByRole("searchbox", { name: "Find a symbol" });
	}

	get canvas(): Locator {
		return this.requireFrame().locator(".cockpit-canvas");
	}

	get inspector(): Locator {
		return this.requireFrame().getByLabel("Contextual code inspector");
	}

	get relationInspector(): Locator {
		return this.requireFrame().getByLabel("Selected relation");
	}

	get minimap(): Locator {
		return this.requireFrame().locator(".react-flow__minimap");
	}

	get controls(): Locator {
		return this.requireFrame().locator(".react-flow__controls");
	}

	node(label: string): Locator {
		return this.requireFrame()
			.locator(".cockpit-card")
			.filter({ has: this.requireFrame().locator(".cockpit-card-name", { hasText: label }) })
			.first();
	}

	focusNode(): Locator {
		return this.requireFrame().locator(".cockpit-card.focus").first();
	}

	edges(): Locator {
		return this.requireFrame().locator(".react-flow__edge");
	}

	async searchAndFocus(symbol: string): Promise<void> {
		await this.search.fill(symbol);
		const option = this.requireFrame().getByRole("option").filter({ hasText: symbol }).first();
		await option.waitFor({ state: "visible", timeout: 10_000 });
		await option.click();
		await this.expectFocus(symbol);
	}

	async expectFocus(symbol: string): Promise<void> {
		await expect(this.focusNode().locator(".cockpit-card-name")).toHaveText(symbol, { timeout: 10_000 });
		await expect(this.canvas).toBeVisible({ timeout: 10_000 });
	}

	async inspectNode(symbol: string): Promise<void> {
		await this.node(symbol).getByRole("button", { name: "Inspect", exact: true }).click();
		await expect(this.inspector).toBeVisible({ timeout: 5_000 });
	}

	async refocusNode(symbol: string): Promise<void> {
		await this.node(symbol).getByRole("button", { name: "Refocus", exact: true }).click();
		await this.expectFocus(symbol);
	}

	async pinNode(symbol: string): Promise<void> {
		await this.node(symbol).getByRole("button", { name: "Pin", exact: true }).click();
		await expect(this.node(symbol).getByRole("button", { name: "Unpin", exact: true })).toBeVisible();
	}

	async unpinNode(symbol: string): Promise<void> {
		await this.node(symbol).getByRole("button", { name: "Unpin", exact: true }).click();
	}

	async expandOutgoing(symbol: string): Promise<void> {
		const expand = this.node(symbol).locator(".cockpit-expand.outgoing");
		await expand.waitFor({ state: "visible", timeout: 5_000 });
		await expand.click();
	}

	async undoExpansion(): Promise<void> {
		await this.requireFrame().getByRole("button", { name: "↶ Expand", exact: true }).click();
	}

	async recenter(): Promise<void> {
		await this.requireFrame().getByRole("button", { name: /Recenter/ }).click();
		await this.waitForViewportSettled();
	}

	async toggleRelation(relation: "calls" | "data" | "types" | "references"): Promise<void> {
		await this.requireFrame().getByRole("group", { name: "Relations" }).getByRole("button", { name: relation, exact: true }).click();
	}

	async selectFirstEdge(): Promise<void> {
		const canvas = await this.canvas.boundingBox();
		expect(canvas, "The Cockpit canvas must have screen coordinates").not.toBeNull();
		const interactions = this.requireFrame().locator(".react-flow__edge-interaction");
		let target: { x: number; y: number } | undefined;
		for (let index = 0; index < await interactions.count(); index += 1) {
			const box = await interactions.nth(index).boundingBox();
			if (!box) continue;
			const point = { x: box.x + box.width / 2, y: box.y + box.height / 2 };
			if (
				point.x > canvas!.x + 24 && point.x < canvas!.x + canvas!.width - 24 &&
				point.y > canvas!.y + 24 && point.y < canvas!.y + canvas!.height - 24
			) {
				target = point;
				break;
			}
		}
		expect(target, "At least one relation hit area must lie inside the Cockpit canvas").toBeDefined();
		await this.page.mouse.click(target!.x, target!.y);
		await expect(this.relationInspector).toBeVisible({ timeout: 5_000 });
	}

	async closeInspectorWithEscape(): Promise<void> {
		await this.page.keyboard.press("Escape");
		await expect(this.inspector).toBeHidden({ timeout: 5_000 });
	}

	async resizeInspector(delta: number): Promise<void> {
		const handle = this.inspector.getByRole("separator", { name: "Resize source inspector" });
		const box = await handle.boundingBox();
		expect(box, "The code inspector resize handle must have screen coordinates").not.toBeNull();
		await this.page.mouse.move(box!.x + box!.width / 2, box!.y + 40);
		await this.page.mouse.down();
		await this.page.mouse.move(box!.x + box!.width / 2 - delta, box!.y + 40, { steps: 8 });
		await this.page.mouse.up();
	}

	async zoomByWheel(deltaY: number): Promise<void> {
		const box = await this.canvas.boundingBox();
		expect(box, "The Cockpit canvas must have screen coordinates").not.toBeNull();
		await this.page.mouse.move(box!.x + box!.width * 0.72, box!.y + box!.height * 0.72);
		await this.page.mouse.wheel(0, deltaY);
	}

	async panBy(delta: { x: number; y: number }): Promise<void> {
		const pane = this.requireFrame().locator(".react-flow__pane");
		const box = await pane.boundingBox();
		expect(box, "The React Flow pane must have screen coordinates").not.toBeNull();
		const occupied = this.requireFrame().locator([
			".react-flow__node",
			".react-flow__controls",
			".react-flow__minimap",
			".cockpit-rollup",
			".cockpit-canvas-actions",
		].join(","));
		const rectangles = (await Promise.all(
			Array.from({ length: await occupied.count() }, (_, index) => occupied.nth(index).boundingBox()),
		)).filter((candidate): candidate is NonNullable<typeof candidate> => Boolean(candidate));
		let start: { x: number; y: number } | undefined;
		for (const yRatio of [0.85, 0.65, 0.35, 0.2]) {
			for (const xRatio of [0.8, 0.6, 0.4, 0.2]) {
				const candidate = { x: box!.x + box!.width * xRatio, y: box!.y + box!.height * yRatio };
				if (!rectangles.some((rect) => candidate.x >= rect.x - 12 && candidate.x <= rect.x + rect.width + 12 && candidate.y >= rect.y - 12 && candidate.y <= rect.y + rect.height + 12)) {
					start = candidate;
					break;
				}
			}
			if (start) break;
		}
		if (!start) throw new Error("The Cockpit does not expose an empty pane point for panning");
		await this.page.mouse.move(start.x, start.y);
		await this.page.mouse.down();
		await this.page.mouse.move(start.x + delta.x, start.y + delta.y, { steps: 10 });
		await this.page.mouse.up();
	}

	async viewport(): Promise<{ x: number; y: number; zoom: number }> {
		return this.requireFrame().evaluate(() => {
			const viewport = document.querySelector<HTMLElement>(".react-flow__viewport");
			if (!viewport) throw new Error("React Flow viewport is missing");
			const matrix = new DOMMatrixReadOnly(getComputedStyle(viewport).transform);
			return { x: matrix.e, y: matrix.f, zoom: matrix.a };
		});
	}

	async edgeEvidence(): Promise<{ total: number; mounted: number; painted: number; visible: number }> {
		return this.requireFrame().evaluate(() => {
			const canvas = document.querySelector(".cockpit-canvas")?.getBoundingClientRect();
			const paths = [...document.querySelectorAll<SVGPathElement>(".react-flow__edge-path")];
			const mounted = paths.filter((path) => Boolean(path.getAttribute("d")?.trim()) && path.getTotalLength() > 0);
			return {
				total: document.querySelectorAll(".react-flow__edge").length,
				mounted: mounted.length,
				painted: mounted.filter((path) => {
					const style = getComputedStyle(path);
					return style.stroke !== "none" && Number.parseFloat(style.strokeWidth) > 0 && Number.parseFloat(style.strokeOpacity || "1") > 0;
				}).length,
				visible: canvas
					? mounted.filter((path) => {
						const rect = path.getBoundingClientRect();
						return rect.right >= canvas.left && rect.left <= canvas.right && rect.bottom >= canvas.top && rect.top <= canvas.bottom;
					}).length
					: 0,
			};
		});
	}

	async inspectorWidth(): Promise<number> {
		const box = await this.inspector.boundingBox();
		if (!box) throw new Error("The contextual code inspector is not measurable");
		return box.width;
	}

	async selectedNodeName(): Promise<string | undefined> {
		const selected = this.requireFrame().locator(".cockpit-card.selected .cockpit-card-name").first();
		return (await selected.count()) > 0 ? (await selected.textContent())?.trim() : undefined;
	}

	private requireFrame(): Frame {
		if (!this.frame) throw new Error("CockpitPage.waitUntilOpen() must be called first");
		return this.frame;
	}

	private async findFrame(): Promise<Frame> {
		const deadline = Date.now() + 10_000;
		while (Date.now() < deadline) {
			for (const frame of this.page.frames()) {
				if (frame === this.page.mainFrame() || (await frame.locator(".explorer-shell").count()) === 0) continue;
				const frameElement = await frame.frameElement();
				try {
					const box = await frameElement.boundingBox();
					if ((await frameElement.isVisible()) && box && box.width > 100 && box.height > 100) return frame;
				} finally {
					await frameElement.dispose();
				}
			}
			await this.page.waitForTimeout(50);
		}
		throw new Error("The Code Cockpit webview frame did not become available");
	}

	private async waitForViewportSettled(): Promise<void> {
		await this.requireFrame().evaluate(() => new Promise<void>((resolve, reject) => {
			const viewport = document.querySelector<HTMLElement>(".react-flow__viewport");
			if (!viewport) {
				reject(new Error("React Flow viewport is missing"));
				return;
			}
			const deadline = performance.now() + 5_000;
			let previous = "";
			let stableFrames = 0;
			const sample = () => {
				const current = getComputedStyle(viewport).transform;
				stableFrames = current === previous ? stableFrames + 1 : 0;
				previous = current;
				if (stableFrames >= 4) resolve();
				else if (performance.now() >= deadline) reject(new Error("React Flow viewport did not settle"));
				else requestAnimationFrame(sample);
			};
			requestAnimationFrame(sample);
		}));
	}
}
