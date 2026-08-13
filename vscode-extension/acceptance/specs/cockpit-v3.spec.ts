import { expect, test } from "../fixtures/test";
import type { CockpitPage } from "../pages/CockpitPage";
import type { WorkbenchPage } from "../pages/WorkbenchPage";

async function openDenseCockpit(workbench: WorkbenchPage, cockpit: CockpitPage): Promise<void> {
	await workbench.openCockpit();
	await cockpit.waitUntilOpen();
	await cockpit.searchAndFocus("fan_out");
}

test.describe("Code Cockpit V3 user journeys", () => {
	test("opens one symbol cockpit from the Workspace header and renders an operable graph", async ({ workbench, cockpit }) => {
		await test.step("open the global Cockpit and search like a user", async () => {
			await openDenseCockpit(workbench, cockpit);
			await expect(cockpit.focusNode()).toContainText("fan_out");
			await expect(cockpit.minimap).toBeVisible();
			await expect(cockpit.controls.locator(".react-flow__controls-button")).toHaveCount(3);
			await expect(cockpit.canvas.locator(".react-flow__node")).toHaveCount(7);
			await expect(cockpit.edges()).toHaveCount(6);
		});

		await test.step("require every initial relation to be mounted, painted and on-screen", async () => {
			await expect
				.poll(() => cockpit.edgeEvidence(), {
					timeout: 5_000,
					message: "The real React Flow DOM must expose six visible relation paths",
				})
				.toEqual({ total: 6, mounted: 6, painted: 6, visible: 6 });
		});
	});

	test("navigates, expands, inspects and refocuses without implicit context changes", async ({ workbench, cockpit }, testInfo) => {
		await openDenseCockpit(workbench, cockpit);

		await test.step("pan and zoom through the actual React Flow surface", async () => {
			const initial = await cockpit.viewport();
			await cockpit.zoomByWheel(-180);
			await expect.poll(async () => (await cockpit.viewport()).zoom, { timeout: 5_000 }).toBeGreaterThan(initial.zoom + 0.02);
			const zoomed = await cockpit.viewport();
			await cockpit.panBy({ x: 75, y: 45 });
			await expect
				.poll(async () => {
					const current = await cockpit.viewport();
					return Math.hypot(current.x - zoomed.x, current.y - zoomed.y);
				}, { timeout: 5_000, message: "Dragging the graph background must pan the viewport" })
				.toBeGreaterThan(20);
			await cockpit.selectFirstEdge();
			await expect(cockpit.relationInspector).toContainText(/calls|references|data|types/i);
		});

		await test.step("expand progressive disclosure and undo it through visible controls", async () => {
			await cockpit.expandOutgoing("fan_out");
			await expect.poll(() => cockpit.edges().count(), { timeout: 10_000 }).toBeGreaterThan(6);
			await cockpit.undoExpansion();
			await expect.poll(() => cockpit.edges().count(), { timeout: 5_000 }).toBe(6);
		});

		await test.step("filter relations through the toolbar and restore them", async () => {
			await cockpit.toggleRelation("calls");
			await expect.poll(() => cockpit.edges().count(), { timeout: 5_000 }).toBe(0);
			await cockpit.toggleRelation("calls");
			await expect.poll(() => cockpit.edges().count(), { timeout: 5_000 }).toBe(6);
		});

		await test.step("inspect code beside the still-mounted graph and resize the inspector", async () => {
			await cockpit.inspectNode("dep_a");
			await expect(cockpit.inspector).toContainText(/dep_a/);
			await expect(cockpit.canvas).toBeVisible();
			const initialWidth = await cockpit.inspectorWidth();
			await cockpit.resizeInspector(70);
			await expect.poll(() => cockpit.inspectorWidth(), { timeout: 5_000 }).toBeGreaterThan(initialWidth + 25);
			const screenshotPath = testInfo.outputPath("cockpit-code-inspector.png");
			await workbench.page.screenshot({ path: screenshotPath });
			await testInfo.attach("cockpit-code-inspector.png", {
				path: screenshotPath,
				contentType: "image/png",
			});
			await cockpit.closeInspectorWithEscape();
		});

		await test.step("pin a neighbor, then make refocus an explicit tree-synchronizing action", async () => {
			await cockpit.recenter();
			await cockpit.pinNode("dep_a");
			await cockpit.unpinNode("dep_a");
			await cockpit.refocusNode("dep_a");
			await expect(workbench.selectedItem()).toContainText("dep_a", { timeout: 10_000 });
		});
	});

	test("synchronizes TreeView, cockpit and editor through real selections", async ({ workbench, cockpit }) => {
		await test.step("select a Symbols row and focus the existing Cockpit", async () => {
			await workbench.openCockpit();
			await cockpit.waitUntilOpen();
			await workbench.selectSymbol(/^fan_out(?:\(\))?/);
			await cockpit.waitUntilOpen();
			await cockpit.expectFocus("fan_out");
		});

		await test.step("move the editor cursor to a visible neighbor and return to its selected card", async () => {
			await workbench.openSourceFile(19);
			await expect(workbench.page.getByRole("tab", { name: /lib\.rs/ }).first()).toBeVisible({ timeout: 5_000 });
			await expect(workbench.selectedItem()).toContainText("dep_a", { timeout: 10_000 });
			await workbench.showCockpitTab();
			await cockpit.waitUntilOpen();
			await expect.poll(() => cockpit.selectedNodeName(), { timeout: 5_000 }).toBe("dep_a");
		});

		await test.step("focus the symbol at the editor cursor through the Command Palette", async () => {
			await workbench.openSourceFile(28);
			await workbench.focusSymbolAtCursor();
			await workbench.showCockpitTab();
			await cockpit.waitUntilOpen();
			await cockpit.expectFocus("fan_out");
			await expect(workbench.selectedItem()).toContainText("fan_out", { timeout: 10_000 });
		});
	});
});
