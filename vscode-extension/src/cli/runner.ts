import { spawn } from "node:child_process";
import * as os from "node:os";
import * as path from "node:path";
import * as vscode from "vscode";

// Low-level access to the code-moniker CLI: binary discovery, ENOENT fallback,
// and a single spawn helper shared by every feature.

export type CliOutcome =
	| { kind: "done"; code: number; stdout: string; stderr: string }
	| { kind: "missing"; tried: string }
	| { kind: "spawnError"; message: string };

function configuredBinary(): string {
	return (
		vscode.workspace
			.getConfiguration("codeMoniker")
			.get<string>("binaryPath")
			?.trim() || "code-moniker"
	);
}

function cargoFallback(): string {
	return path.join(os.homedir(), ".cargo", "bin", "code-moniker");
}

// Ordered binary candidates: the configured/PATH binary first, then the cargo
// install fallback. Shared by the CLI runner and the detached daemon launcher.
export function binaryCandidates(): string[] {
	const primary = configuredBinary();
	const fallback = cargoFallback();
	return primary === fallback ? [primary] : [primary, fallback];
}

export function launchDetached(args: string[]): void {
	tryLaunchDetached(binaryCandidates(), 0, args);
}

export function missingBinaryMessage(tried: string): string {
	return (
		`Could not find the code-moniker binary (\`${tried}\`). ` +
		"Install it with `cargo install --path crates/cli` or set `codeMoniker.binaryPath`."
	);
}

// Runs the CLI, retrying the cargo path if `code-moniker` is not on PATH.
export async function runCli(args: string[], input?: string): Promise<CliOutcome> {
	const primary = configuredBinary();
	let result = await spawnOnce(primary, args, input);
	if (result.kind === "missing" && primary === "code-moniker") {
		result = await spawnOnce(cargoFallback(), args, input);
		if (result.kind === "missing") {
			return { kind: "missing", tried: primary };
		}
	}
	return result;
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
