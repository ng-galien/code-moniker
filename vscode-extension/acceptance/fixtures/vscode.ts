import { randomUUID } from "node:crypto";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";

import type { ElectronApplication, Page } from "@playwright/test";
import { _electron as electron } from "playwright";

import { preparedAcceptanceVSCode } from "./vscodeDownload";
import { seedAcceptanceWorkspace } from "./workspace";

const extensionRoot = resolve(__dirname, "../..");
const repositoryRoot = resolve(extensionRoot, "..");
const artifactsRoot = resolve(extensionRoot, "test-results", "acceptance-worker");

interface ReadyState {
	activationId: string;
	commandNonce?: string;
	result?: unknown;
	status: "ready";
}

function readReadyState(path: string): ReadyState | undefined {
	try {
		const state = JSON.parse(readFileSync(path, "utf8")) as Partial<ReadyState>;
		if (state.status === "ready" && typeof state.activationId === "string") return state as ReadyState;
	} catch {
		// The extension and the poller may overlap while replacing the file.
	}
	return undefined;
}

async function waitForReadyState(
	path: string,
	predicate: (state: ReadyState) => boolean,
	timeout: number,
	description: string,
): Promise<ReadyState> {
	const deadline = Date.now() + timeout;
	while (Date.now() < deadline) {
		const state = readReadyState(path);
		if (state && predicate(state)) return state;
		await new Promise((resolvePoll) => setTimeout(resolvePoll, 50));
	}
	throw new Error(`${description} did not complete within ${timeout} ms`);
}

interface ElectronWindowState {
	area: number;
	focused: boolean;
	title: string;
	visible: boolean;
}

async function electronWindowState(app: ElectronApplication, page: Page): Promise<ElectronWindowState> {
	const browserWindow = await app.browserWindow(page);
	try {
		return await browserWindow.evaluate((window) => {
			const [width, height] = window.getContentSize();
			return {
				area: width * height,
				focused: window.isFocused(),
				title: window.getTitle(),
				visible: window.isVisible(),
			};
		});
	} finally {
		await browserWindow.dispose();
	}
}

async function waitForVSCodeWindow(app: ElectronApplication, timeout: number): Promise<Page> {
	const deadline = Date.now() + timeout;
	let lastStates: ElectronWindowState[] = [];
	while (Date.now() < deadline) {
		const candidates: Array<{ page: Page; state: ElectronWindowState }> = [];
		for (const page of app.windows()) {
			if (page.isClosed()) continue;
			const state = await electronWindowState(app, page).catch(() => undefined);
			if (state?.visible) candidates.push({ page, state });
		}
		lastStates = candidates.map(({ state }) => state);
		const focused = candidates.find(({ state }) => state.focused);
		if (focused) return focused.page;
		candidates.sort((left, right) => right.state.area - left.state.area);
		if (candidates[0]) return candidates[0].page;
		await new Promise((resolvePoll) => setTimeout(resolvePoll, 50));
	}
	throw new Error(`No visible VS Code Electron window appeared within ${timeout} ms: ${JSON.stringify(lastStates)}`);
}

async function resizeWindow(app: ElectronApplication, page: Page, width: number, height: number): Promise<void> {
	const browserWindow = await app.browserWindow(page);
	try {
		await browserWindow.evaluate((window, size) => window.setContentSize(size.width, size.height), { width, height });
		await page.waitForFunction(
			(size) => window.innerWidth === size.width && window.innerHeight === size.height,
			{ width, height },
			{ timeout: 5_000 },
		);
	} finally {
		await browserWindow.dispose();
	}
}

export interface VSCodeInstance {
	app: ElectronApplication;
	page: Page;
	workspace: string;
	resetWorkbenchUI(): Promise<void>;
	resizeWindow(width: number, height: number): Promise<void>;
	dispose(): Promise<void>;
}

export async function launchVSCode(): Promise<VSCodeInstance> {
	const { executablePath } = preparedAcceptanceVSCode();
	const profileRoot = mkdtempSync(join(process.platform === "darwin" ? "/tmp" : tmpdir(), "cm-acceptance-"));
	const workspace = join(profileRoot, "workspace");
	const userDataDir = join(profileRoot, "user");
	const extensionsDir = join(profileRoot, "extensions");
	const controlFile = join(profileRoot, "acceptance-command.json");
	const readyFile = `${controlFile}.ready`;
	const settingsPath = join(userDataDir, "User", "settings.json");
	const binaryName = process.platform === "win32" ? "code-moniker.exe" : "code-moniker";
	const binaryPath = join(repositoryRoot, "target", "debug", binaryName);
	rmSync(artifactsRoot, { recursive: true, force: true });
	mkdirSync(dirname(settingsPath), { recursive: true });
	mkdirSync(extensionsDir, { recursive: true });
	mkdirSync(artifactsRoot, { recursive: true });
	seedAcceptanceWorkspace(workspace);
	writeFileSync(settingsPath, JSON.stringify({
		"codeMoniker.binaryPath": binaryPath,
		"security.workspace.trust.enabled": false,
		"telemetry.telemetryLevel": "off",
		"update.mode": "none",
		"git.openRepositoryInParentFolders": "never",
		"window.dialogStyle": "custom",
		"workbench.startupEditor": "none",
		"workbench.colorTheme": "Default Light Modern",
		"workbench.secondarySideBar.defaultVisibility": "hidden",
	}));

	let app: ElectronApplication | undefined;
	let tracingStarted = false;
	let disposed = false;
	const dispose = async () => {
		if (disposed) return;
		disposed = true;
		if (app && tracingStarted) {
			await app.context().tracing.stop({ path: join(artifactsRoot, "trace.zip") }).catch(() => undefined);
		}
		await app?.close().catch(() => undefined);
		rmSync(profileRoot, { recursive: true, force: true });
	};

	try {
		app = await electron.launch({
			executablePath,
			env: { ...process.env, CODE_MONIKER_ACCEPTANCE_CONTROL_FILE: controlFile },
			args: [
				"--disable-gpu-sandbox",
				"--disable-updates",
				"--force-disable-user-env",
				"--locale=en",
				"--new-window",
				"--skip-release-notes",
				"--skip-welcome",
				"--no-sandbox",
				`--user-data-dir=${userDataDir}`,
				`--extensions-dir=${extensionsDir}`,
				`--extensionDevelopmentPath=${extensionRoot}`,
				workspace,
			],
			recordVideo: { dir: join(artifactsRoot, "video"), size: { width: 1440, height: 900 } },
		});
		await app.context().tracing.start({ screenshots: true, snapshots: true });
		tracingStarted = true;
		const page = await waitForVSCodeWindow(app, 30_000);
		await resizeWindow(app, page, 1440, 900);
		const activity = page.locator('.activitybar a[aria-label*="Code Moniker"], .activitybar [role="tab"][aria-label*="Code Moniker"]').first();
		await activity.waitFor({ state: "visible", timeout: 10_000 });
		await activity.click();
		let ready = await waitForReadyState(readyFile, () => true, 30_000, "Code Moniker extension activation");
		const runningApp = app;
		const runInfrastructureCommand = async (command: string): Promise<ReadyState> => {
			const nonce = randomUUID();
			writeFileSync(controlFile, JSON.stringify({ command, nonce }));
			ready = await waitForReadyState(
				readyFile,
				(state) => state.activationId === ready.activationId && state.commandNonce === nonce,
				10_000,
				`VS Code infrastructure command ${command}`,
			);
			return ready;
		};
		return {
			app: runningApp,
			page,
			workspace,
			async resetWorkbenchUI() {
				const state = await runInfrastructureCommand("codeMoniker.acceptance.resetWorkbench");
				const result = state.result as { remainingTabCount?: unknown } | undefined;
				if (result?.remainingTabCount !== 0) {
					throw new Error(`VS Code still exposes ${String(result?.remainingTabCount)} tabs after reset`);
				}
			},
			async resizeWindow(width, height) {
				await resizeWindow(runningApp, page, width, height);
			},
			dispose,
		};
	} catch (error) {
		await dispose();
		throw error;
	}
}
