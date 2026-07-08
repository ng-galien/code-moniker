import * as fs from "node:fs";
import * as vscode from "vscode";

import { registerDaemonCommands } from "./commands";
import { themeColor } from "../shared/appIcons";
import { registryDir } from "./registry";
import { DaemonSession, DaemonStatus } from "./session";
import { DaemonListProvider } from "./tree";

export interface DaemonContext {
	session: DaemonSession;
	provider: DaemonListProvider;
}

// Boots the daemon foundation: a single session for the open workspace, the
// daemon list view, a status-bar indicator, a registry watcher, and auto-connect.
// Returns the session so the symbol and rules features can share it.
export function registerDaemon(context: vscode.ExtensionContext): DaemonContext {
	const roots = (vscode.workspace.workspaceFolders ?? []).map((folder) => folder.uri.fsPath);
	const session = new DaemonSession(roots);
	const provider = new DaemonListProvider(session, roots);

	const statusItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
	statusItem.command = "codeMoniker.daemon.connect";
	updateStatusItem(statusItem, session.status);
	statusItem.show();

	const watcher = watchRegistry(() => provider.refresh());

	context.subscriptions.push(
		statusItem,
		watcher,
		session,
		session.onDidChangeStatus((status) => {
			updateStatusItem(statusItem, status);
			provider.refresh();
			void vscode.commands.executeCommand("setContext", "codeMoniker.daemonReady", status === "ready");
		}),
	);

	registerDaemonCommands(context, session, provider);

	if (roots.length > 0 && autoConnect()) {
		void session.connectOrStart().then(() => provider.refresh());
	}

	return { session, provider };
}

function autoConnect(): boolean {
	return vscode.workspace.getConfiguration("codeMoniker").get<boolean>("daemon.autoConnect", true);
}

function updateStatusItem(item: vscode.StatusBarItem, status: DaemonStatus): void {
	switch (status) {
		case "ready":
			item.text = "$(server-process) Moniker: ready";
			item.tooltip = "Workspace daemon connected and indexed";
			item.backgroundColor = undefined;
			break;
		case "loading":
			item.text = "$(loading~spin) Moniker: indexing…";
			item.tooltip = "Workspace daemon is building the index";
			item.backgroundColor = undefined;
			break;
		case "connecting":
			item.text = "$(loading~spin) Moniker: connecting…";
			item.tooltip = "Connecting to the workspace daemon";
			item.backgroundColor = undefined;
			break;
		case "error":
			item.text = "$(warning) Moniker: error";
			item.tooltip = "Daemon connection failed — click to retry";
			item.backgroundColor = themeColor("statusBarItem.warningBackground");
			break;
		default:
			item.text = "$(plug) Moniker: connect";
			item.tooltip = "Connect to the workspace daemon";
			item.backgroundColor = undefined;
	}
}

function watchRegistry(onChange: () => void): vscode.Disposable {
	let watcher: fs.FSWatcher | undefined;
	let timer: ReturnType<typeof setTimeout> | undefined;
	const fire = (): void => {
		if (timer) {
			clearTimeout(timer);
		}
		timer = setTimeout(onChange, 150);
	};
	const dir = registryDir();
	try {
		fs.mkdirSync(dir, { recursive: true });
		watcher = fs.watch(dir, () => fire());
	} catch {
		watcher = undefined;
	}
	return {
		dispose: () => {
			if (timer) {
				clearTimeout(timer);
			}
			watcher?.close();
		},
	};
}
