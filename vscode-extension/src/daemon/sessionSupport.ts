import {
	PROTOCOL_VERSION,
	ProtocolMismatchError,
} from "@code-moniker/client";

export function nonEmptyRoots(
	roots: string[],
): [string, ...string[]] {
	if (roots.length === 0) {
		throw new Error("the workspace has no roots");
	}
	return roots as [string, ...string[]];
}

export function nonEmptyBinaryCandidates(
	candidates: string[],
): [string, ...string[]] {
	if (candidates.length === 0) {
		throw new Error("no code-moniker binary candidate is configured");
	}
	return candidates as [string, ...string[]];
}

export function protocolFaultMessage(error: ProtocolMismatchError): string {
	return error.direction === "daemon_older"
		? `the workspace daemon speaks protocol ${error.actual} but the extension expects ${PROTOCOL_VERSION}; the installed code-moniker CLI is outdated — update it (or point codeMoniker.binaryPath at a matching build), then reconnect`
		: `the workspace daemon speaks protocol ${error.actual}, newer than this extension's ${PROTOCOL_VERSION}; update the Code Moniker extension, then reconnect (the daemon was left running)`;
}
