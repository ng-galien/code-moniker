import * as vscode from "vscode";

import { DaemonSession } from "../daemon/session";
import { ChangeDecorationModel } from "./decorations";
import { ChangesRepository } from "./repository";
import { ChangesProvider, reviewSummaryLabel } from "./tree";

export interface ChangesFeature {
	provider: ChangesProvider;
	repository: ChangesRepository;
	decorations: ChangeDecorationModel;
}

const FACTS_DEBOUNCE_MS = 250;

export function registerChanges(
	context: vscode.ExtensionContext,
	session: DaemonSession,
): ChangesFeature {
	const repository = new ChangesRepository(session);
	const provider = new ChangesProvider(session, repository);
	const decorations = new ChangeDecorationModel(session.workspaceRoots);
	const status = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 90);
	status.name = "Code Moniker Changes";

	let pending: NodeJS.Timeout | undefined;
	const scheduleFacts = () => {
		if (pending) {
			return;
		}
		pending = setTimeout(() => {
			pending = undefined;
			void publishFacts(session, repository, decorations, status);
		}, FACTS_DEBOUNCE_MS);
	};

	context.subscriptions.push(
		decorations,
		status,
		vscode.window.registerFileDecorationProvider(decorations),
		session.onDidChangeStatus(() => {
			provider.refresh();
			scheduleFacts();
		}),
		session.onWorkspaceEvent((event) => {
			if (event.kind === "refreshed" || event.kind === "git_base") {
				scheduleFacts();
			}
		}),
		new vscode.Disposable(() => {
			if (pending) {
				clearTimeout(pending);
			}
		}),
	);

	return { provider, repository, decorations };
}

async function publishFacts(
	session: DaemonSession,
	repository: ChangesRepository,
	decorations: ChangeDecorationModel,
	status: vscode.StatusBarItem,
): Promise<void> {
	if (!session.ready) {
		decorations.update(undefined);
		status.hide();
		return;
	}
	try {
		const review = await repository.review();
		decorations.update(review);
		if (review && review.summary.files > 0) {
			status.text = `$(git-compare) ${review.summary.symbol_changes} symbol change(s)`;
			status.tooltip = reviewSummaryLabel(review);
			status.show();
		} else {
			status.hide();
		}
	} catch {
		decorations.update(undefined);
		status.hide();
	}
}
