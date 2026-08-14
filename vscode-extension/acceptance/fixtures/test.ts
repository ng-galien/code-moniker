import { test as base } from "@playwright/test";

import { CockpitPage } from "../pages/CockpitPage";
import { WorkbenchPage } from "../pages/WorkbenchPage";
import { launchVSCode, type VSCodeInstance } from "./vscode";

interface AcceptanceFixtures {
	workbench: WorkbenchPage;
	cockpit: CockpitPage;
}

interface WorkerFixtures {
	vscode: VSCodeInstance;
}

export const test = base.extend<AcceptanceFixtures, WorkerFixtures>({
	vscode: [
		// Playwright requires worker fixtures to use object destructuring even when no dependency is needed.
		async ({}, use) => {
			const instance = await launchVSCode();
			try {
				await use(instance);
			} finally {
				await instance.dispose();
			}
		},
		{ scope: "worker" },
	],
	workbench: async ({ vscode }, use, testInfo) => {
		const workbench = new WorkbenchPage(vscode.page, vscode.workspace, vscode.resizeWindow, vscode.resetWorkbenchUI);
		await workbench.reset();
		try {
			await use(workbench);
		} finally {
			if (testInfo.status !== testInfo.expectedStatus) {
				await vscode.page.screenshot({ path: testInfo.outputPath("failure.png"), fullPage: true }).catch(() => undefined);
			}
			await workbench.reset();
		}
	},
	cockpit: async ({ vscode }, use) => {
		await use(new CockpitPage(vscode.page));
	},
});

export { expect } from "@playwright/test";
