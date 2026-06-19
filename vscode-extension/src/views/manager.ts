import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { ViewsRepository } from "./repository";
import { ViewsProvider } from "./tree";

export interface ViewsFeature {
	provider: ViewsProvider;
}

export function registerViews(
	context: vscode.ExtensionContext,
	session: DaemonSession,
): ViewsFeature {
	const repository = new ViewsRepository(session);
	const provider = new ViewsProvider(repository);

	context.subscriptions.push(
		session.onDidChangeStatus(() => provider.refresh()),
		session.onWorkspaceEvent((event) => {
			if (event.kind === "stale" || event.kind === "refreshed") {
				provider.refresh();
			}
		}),
	);

	return { provider };
}
