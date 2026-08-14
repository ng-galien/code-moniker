import { randomUUID } from "node:crypto";
import { rmSync, unwatchFile, watchFile, writeFileSync } from "node:fs";
import { readFile, rm } from "node:fs/promises";

import * as vscode from "vscode";

const RESET_COMMAND = "codeMoniker.acceptance.resetWorkbench";
const SUPPORTED_COMMANDS = new Set([RESET_COMMAND]);

export interface AcceptanceControl extends vscode.Disposable {
	markReady(): void;
}

// Playwright needs one infrastructure-only rendezvous with the Extension Host:
// activation readiness and deterministic editor cleanup. Product scenarios do
// not cross this boundary; all feature actions are performed in the visible UI.
export function registerAcceptanceControl(context: vscode.ExtensionContext): AcceptanceControl | undefined {
	const controlFile = process.env.CODE_MONIKER_ACCEPTANCE_CONTROL_FILE;
	if (!controlFile || context.extensionMode === vscode.ExtensionMode.Production) return undefined;
	const readyFile = `${controlFile}.ready`;
	const activationId = randomUUID();
	const markReady = (commandNonce?: string, result?: unknown) => {
		writeFileSync(readyFile, JSON.stringify({ activationId, commandNonce, result, status: "ready" }));
	};
	let pending = Promise.resolve();
	const consume = () => {
		pending = pending
			.then(async () => {
				const instruction = JSON.parse(await readFile(controlFile, "utf8")) as {
					command?: unknown;
					nonce?: unknown;
				};
				await rm(controlFile, { force: true });
				if (typeof instruction.command !== "string" || !SUPPORTED_COMMANDS.has(instruction.command)) {
					throw new Error(`Unsupported acceptance command: ${String(instruction.command)}`);
				}
				if (typeof instruction.nonce !== "string") throw new Error("Acceptance command is missing its nonce");
				await vscode.commands.executeCommand("workbench.action.files.saveAll");
				const tabs = vscode.window.tabGroups.all.flatMap((group) => group.tabs);
				if (tabs.length > 0) await vscode.window.tabGroups.close(tabs, false);
				await vscode.commands.executeCommand("codeMoniker.workspace.focus");
				markReady(instruction.nonce, {
					closedTabCount: tabs.length,
					remainingTabCount: vscode.window.tabGroups.all.reduce((count, group) => count + group.tabs.length, 0),
				});
			})
			.catch((error: unknown) => {
				if ((error as NodeJS.ErrnoException).code !== "ENOENT") console.error(error);
			});
	};
	watchFile(controlFile, { interval: 50 }, consume);
	return {
		markReady() {
			markReady();
		},
		dispose() {
			unwatchFile(controlFile, consume);
			rmSync(readyFile, { force: true });
		},
	};
}
