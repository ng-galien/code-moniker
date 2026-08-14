import { expect, type Locator, type Page } from "@playwright/test";

export class WorkbenchPage {
	constructor(
		readonly page: Page,
		readonly workspace: string,
		private readonly resizeNativeWindow: (width: number, height: number) => Promise<void>,
		private readonly resetNativeWorkbench: () => Promise<void>,
	) {}

	async reset(): Promise<void> {
		await this.resetNativeWorkbench();
		await expect(this.page.locator(".editor-group-container .tabs-container .tab:visible")).toHaveCount(0, {
			timeout: 5_000,
		});
		await this.resizeNativeWindow(1_440, 900);
		await this.collapseAll();
	}

	async openCockpit(): Promise<void> {
		const pane = this.workspacePane();
		await pane.locator(".pane-header").hover();
		const action = pane.getByLabel(/Open Code Cockpit/i).first();
		await action.waitFor({ state: "visible", timeout: 10_000 });
		await action.click();
	}

	item(label: RegExp): Locator {
		return this.workspacePane().getByRole("treeitem").filter({ hasText: label }).first();
	}

	selectedItem(): Locator {
		return this.workspacePane().locator('[role="treeitem"][aria-selected="true"]:visible').first();
	}

	async selectSymbol(label: RegExp): Promise<void> {
		const item = await this.expandUntilVisible(label);
		await item.scrollIntoViewIfNeeded();
		await item.click();
		await expect(item).toHaveAttribute("aria-selected", "true", { timeout: 5_000 });
	}

	async openSourceFile(line?: number): Promise<void> {
		await this.page.keyboard.press(process.platform === "darwin" ? "Meta+P" : "Control+P");
		const input = this.page.locator(".quick-input-widget input").first();
		await input.waitFor({ state: "visible", timeout: 5_000 });
		await input.fill(`src/lib.rs${line === undefined ? "" : `:${line}`}`);
		await this.page.locator(".quick-input-list .monaco-list-row").filter({ hasText: /lib\.rs/ }).first().waitFor({
			state: "visible",
			timeout: 5_000,
		});
		await this.page.keyboard.press("Enter");
		await expect(input).toBeHidden({ timeout: 5_000 });
		await expect(this.page.getByRole("tab", { name: /lib\.rs/ }).first()).toBeVisible({ timeout: 5_000 });
	}

	async focusSymbolAtCursor(): Promise<void> {
		await this.page.keyboard.press(process.platform === "darwin" ? "Meta+P" : "Control+P");
		const input = this.page.locator(".quick-input-widget input").first();
		await input.waitFor({ state: "visible", timeout: 5_000 });
		await input.fill(">Focus Symbol at Cursor in Code Cockpit");
		await this.page.locator(".quick-input-list .monaco-list-row").filter({
			hasText: /Focus Symbol at Cursor in Code Cockpit/,
		}).first().waitFor({ state: "visible", timeout: 5_000 });
		await this.page.keyboard.press("Enter");
		await expect(input).toBeHidden({ timeout: 5_000 });
	}

	async showCockpitTab(): Promise<void> {
		const tab = this.page.getByRole("tab", { name: /Code Cockpit/ }).first();
		await tab.waitFor({ state: "visible", timeout: 5_000 });
		await tab.click();
	}

	private async collapseAll(): Promise<void> {
		const pane = this.workspacePane();
		await pane.locator(".pane-header").hover();
		const action = pane.getByLabel(/^Collapse All$/).first();
		if (await action.isVisible()) {
			if (await action.isEnabled()) await action.click();
			await expect(this.item(/^Symbols/)).toHaveAttribute("aria-expanded", "false", { timeout: 5_000 });
		}
	}

	private async expandUntilVisible(label: RegExp): Promise<Locator> {
		const target = this.item(label);
		if (await target.isVisible()) return target;
		for (let step = 0; step < 64; step += 1) {
			const collapsed = this.workspacePane().locator('[role="treeitem"][aria-expanded="false"]:visible');
			const count = await collapsed.count();
			if (count === 0) break;
			let expanded = false;
			for (let index = 0; index < count; index += 1) {
				const row = collapsed.nth(index);
				const text = (await row.innerText()).trim();
				if (!/^(Symbols|rs|Rust|src|lib|lib\.rs|Widget)/i.test(text)) continue;
				await row.locator(".monaco-tl-twistie").click();
				expanded = true;
				if (await target.isVisible()) return target;
				break;
			}
			if (!expanded) {
				const row = collapsed.first();
				await row.locator(".monaco-tl-twistie").click();
			}
			if (await target.isVisible()) return target;
		}
		throw new Error(`The symbol TreeView row ${label} did not become visible`);
	}

	private workspacePane(): Locator {
		return this.page.locator(".pane").filter({
			has: this.page.locator(".pane-header", { hasText: /WORKSPACE/i }),
		}).first();
	}
}
