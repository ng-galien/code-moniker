import * as vscode from "vscode";

// A file watcher whose three events all mean the same thing: "re-read". The
// wiring is identical everywhere it appears, and forgetting one of the three
// leaves a view stale only for that kind of change.
export function watchAndRefresh(
	glob: vscode.GlobPattern,
	onChange: () => void,
): vscode.Disposable[] {
	const watcher = vscode.workspace.createFileSystemWatcher(glob);
	return [
		watcher,
		watcher.onDidCreate(onChange),
		watcher.onDidChange(onChange),
		watcher.onDidDelete(onChange),
	];
}
