import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { ChangesRepository } from "./repository";
import { ChangesProvider } from "./tree";

export interface ChangesFeature {
	provider: ChangesProvider;
	repository: ChangesRepository;
}

export function registerChanges(
	context: vscode.ExtensionContext,
	session: DaemonSession,
): ChangesFeature {
	const repository = new ChangesRepository(session);
	const provider = new ChangesProvider(session, repository);

	context.subscriptions.push(session.onDidChangeStatus(() => provider.refresh()));

	return { provider, repository };
}
