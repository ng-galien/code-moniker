import { execFileSync } from "node:child_process";
import { resolve } from "node:path";

import { prepareAcceptanceVSCode } from "./fixtures/vscodeDownload";

export default async function globalSetup(): Promise<void> {
	const repositoryRoot = resolve(__dirname, "../..");
	execFileSync("cargo", ["build", "-p", "code-moniker"], {
		cwd: repositoryRoot,
		stdio: "inherit",
	});
	await prepareAcceptanceVSCode();
}
