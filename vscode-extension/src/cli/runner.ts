import { spawn } from "node:child_process";
import { existsSync } from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";

// Low-level access to the code-moniker CLI: binary discovery, ENOENT fallback,
// and a single spawn helper shared by every feature.

export type CliOutcome =
	| { kind: "done"; code: number; stdout: string; stderr: string }
	| { kind: "missing"; tried: string }
	| { kind: "spawnError"; message: string };

function configuredBinary(): string | undefined {
	const setting = vscode.workspace.getConfiguration("codeMoniker");
	const inspected = setting.inspect<string>("binaryPath");
	const explicit =
		inspected?.workspaceFolderValue ?? inspected?.workspaceValue ?? inspected?.globalValue;
	return typeof explicit === "string" && explicit.trim() ? explicit.trim() : undefined;
}

function cargoFallback(): string {
	return path.join(os.homedir(), ".cargo", "bin", "code-moniker");
}

function bundledBinary(): string | undefined {
	const executable = process.platform === "win32" ? "code-moniker.exe" : "code-moniker";
	const binary = path.join(__dirname, "..", "bin", executable);
	return existsSync(binary) ? binary : undefined;
}

// An explicit path is an override. Otherwise, a platform-specific VSIX uses
// its bundled CLI before falling back to development installations on PATH or
// in Cargo's default bin directory.
export function binaryCandidates(): string[] {
	const configured = configuredBinary();
	if (configured) {
		return [configured];
	}
	return [bundledBinary(), "code-moniker", cargoFallback()].filter(
		(candidate): candidate is string => candidate !== undefined,
	);
}

export function launchDetached(args: string[]): void {
	tryLaunchDetached(binaryCandidates(), 0, args);
}

export function missingBinaryMessage(tried: string): string {
	return (
		`Could not find the code-moniker binary (tried: \`${tried}\`). ` +
		"Install a platform-specific Code Moniker VSIX, install `code-moniker` with Cargo, or set `codeMoniker.binaryPath`."
	);
}

// Runs the CLI through the ordered candidates. Every CLI surface and detached
// daemon launch shares the same resolver.
export async function runCli(args: string[], input?: string): Promise<CliOutcome> {
	const candidates = binaryCandidates();
	for (const binary of candidates) {
		const result = await spawnOnce(binary, args, input);
		if (result.kind !== "missing") {
			return result;
		}
	}
	return { kind: "missing", tried: candidates.join(", ") };
}

function spawnOnce(binary: string, args: string[], input?: string): Promise<CliOutcome> {
	return new Promise((resolve) => {
		const child = spawn(binary, args, { stdio: ["pipe", "pipe", "pipe"] });
		let stdout = "";
		let stderr = "";
		let settled = false;

		child.on("error", (err: NodeJS.ErrnoException) => {
			if (settled) {
				return;
			}
			settled = true;
			if (err.code === "ENOENT") {
				resolve({ kind: "missing", tried: binary });
			} else {
				resolve({ kind: "spawnError", message: err.message });
			}
		});
		child.stdout.on("data", (chunk) => {
			stdout += chunk.toString();
		});
		child.stderr.on("data", (chunk) => {
			stderr += chunk.toString();
		});
		child.on("close", (code) => {
			if (settled) {
				return;
			}
			settled = true;
			resolve({ kind: "done", code: code ?? 0, stdout, stderr });
		});

		if (input !== undefined) {
			child.stdin.end(input);
		} else {
			child.stdin.end();
		}
	});
}

function tryLaunchDetached(candidates: string[], index: number, args: string[]): void {
	if (index >= candidates.length) {
		return;
	}
	const child = spawn(candidates[index], args, {
		detached: true,
		stdio: "ignore",
	});
	child.once("error", (err: NodeJS.ErrnoException) => {
		if (err.code === "ENOENT") {
			tryLaunchDetached(candidates, index + 1, args);
		}
	});
	child.unref();
}
