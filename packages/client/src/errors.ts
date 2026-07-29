export class DaemonRpcError extends Error {
	constructor(
		message: string,
		readonly code?: string,
	) {
		super(message);
		this.name = "DaemonRpcError";
	}
}

export class ProtocolMismatchError extends Error {
	readonly direction: "daemon_older" | "daemon_newer";

	constructor(
		readonly expected: number,
		readonly actual: number,
	) {
		const direction = actual < expected ? "daemon_older" : "daemon_newer";
		super(
			direction === "daemon_older"
				? `the daemon speaks protocol ${actual}, but the client expects ${expected}; update the Code Moniker daemon`
				: `the daemon speaks protocol ${actual}, but the client expects ${expected}; update @code-moniker/client`,
		);
		this.name = "ProtocolMismatchError";
		this.direction = direction;
	}
}

export class WorkspaceMismatchError extends Error {
	constructor(
		readonly expected: readonly string[],
		readonly actual: readonly string[],
	) {
		super(
			`daemon workspace mismatch: expected [${expected.join(", ")}], daemon serves [${actual.join(", ")}]`,
		);
		this.name = "WorkspaceMismatchError";
	}
}

export class WorkspaceTargetRequiredError extends Error {
	constructor() {
		super(
			"workspace targeting must set exactly one of expectedWorkspaceRoots or acceptAnyWorkspace: true",
		);
		this.name = "WorkspaceTargetRequiredError";
	}
}

export class UnexpectedQueryResultError extends Error {
	constructor(
		readonly expected: string,
		readonly actual: string,
	) {
		super(`query returned ${actual}, expected ${expected}`);
		this.name = "UnexpectedQueryResultError";
	}
}

export class WebSocketUnavailableError extends Error {
	constructor() {
		super(
			"this runtime has no global WebSocket; pass webSocketFactory when connecting",
		);
		this.name = "WebSocketUnavailableError";
	}
}
