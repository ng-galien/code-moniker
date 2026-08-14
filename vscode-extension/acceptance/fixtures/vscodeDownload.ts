import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";

import { downloadAndUnzipVSCode } from "@vscode/test-electron";

const preparedRuntimeFile = resolve(__dirname, "../../test-results/acceptance-runtime.json");

interface PreparedAcceptanceRuntime {
	executablePath: string;
	version: string;
}

export async function prepareAcceptanceVSCode(): Promise<void> {
	const version = process.env.CODE_MONIKER_ACCEPTANCE_VSCODE_VERSION ?? "stable";
	let lastError: unknown;
	for (let attempt = 1; attempt <= 3; attempt += 1) {
		try {
			const downloadedPath = await downloadAndUnzipVSCode({ version, timeout: 30_000 });
			const executablePath = process.platform === "darwin" && !existsSync(downloadedPath)
				? join(dirname(downloadedPath), "Code")
				: downloadedPath;
			mkdirSync(dirname(preparedRuntimeFile), { recursive: true });
			writeFileSync(preparedRuntimeFile, JSON.stringify({ executablePath, version }));
			return;
		} catch (error) {
			lastError = error;
			if (attempt < 3) await new Promise((resolveAttempt) => setTimeout(resolveAttempt, attempt * 1_000));
		}
	}
	throw new Error(`Unable to prepare VS Code ${version} after 3 attempts`, { cause: lastError });
}

export function preparedAcceptanceVSCode(): PreparedAcceptanceRuntime {
	if (!existsSync(preparedRuntimeFile)) {
		throw new Error("VS Code was not prepared by the Playwright global setup");
	}
	const runtime = JSON.parse(readFileSync(preparedRuntimeFile, "utf8")) as PreparedAcceptanceRuntime;
	if (!runtime.executablePath || !existsSync(runtime.executablePath)) {
		throw new Error(`Prepared VS Code ${runtime.version} is unavailable at ${runtime.executablePath}`);
	}
	return runtime;
}
