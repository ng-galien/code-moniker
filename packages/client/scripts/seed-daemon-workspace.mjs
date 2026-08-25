import { mkdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";

export const DAEMON_SMOKE_FILE_COUNT = 512;

export function seedDaemonWorkspace(workspaceRoot) {
	for (let fileIndex = 0; fileIndex < DAEMON_SMOKE_FILE_COUNT; fileIndex += 1) {
		const bucket = String(Math.floor(fileIndex / 32)).padStart(2, "0");
		const directory = join(
			workspaceRoot,
			"fixture with spaces",
			"sources été",
			"generated",
			bucket,
		);
		mkdirSync(directory, { recursive: true });
		const functions = Array.from(
			{ length: 16 },
			(_, functionIndex) => `
export function entity_${fileIndex}_${functionIndex}(value: number): number {
	return value + ${fileIndex} + ${functionIndex};
}
`,
		).join("");
		writeFileSync(
			join(directory, `entity-${String(fileIndex).padStart(4, "0")}.ts`),
			functions,
		);
	}
}

export function assertDaemonWorkspaceIndexed(status, label) {
	if (status.files !== DAEMON_SMOKE_FILE_COUNT) {
		throw new Error(
			`${label} indexed ${status.files} files instead of ${DAEMON_SMOKE_FILE_COUNT}`,
		);
	}
	if (status.symbols === 0 || status.references === 0) {
		throw new Error(
			`${label} published an empty index: ${status.symbols} symbols, ${status.references} references`,
		);
	}
}

export async function assertPostReadyMutation(
	client,
	workspaceRoot,
	initialStatus,
	label,
) {
	if (typeof initialStatus.generation !== "number") {
		throw new Error(`${label} did not publish an initial generation`);
	}
	const events = [];
	const subscription = await client.events.subscribe((event) => events.push(event));
	try {
		writeFileSync(
			join(
				workspaceRoot,
				"fixture with spaces",
				"sources été",
				"generated",
				"00",
				"entity-0000.ts",
			),
			`export function windows_post_ready_mutation(): number {
	return 9001;
}
`,
		);
		await waitForEvent(events, (event) => event.kind === "stale", label);
		const symbols = await client.symbols.search(
			{ name: "^windows_post_ready_mutation\\(\\)$" },
			{ consistency: "refresh_if_stale" },
		);
		if (
			!symbols.data.rows.some(
				(symbol) => symbol.name === "windows_post_ready_mutation()",
			)
		) {
			throw new Error(
				`${label} lost the post-ready filesystem mutation: ${JSON.stringify(symbols.data.rows.map((symbol) => symbol.name))}`,
			);
		}
		if (
			typeof symbols.generation !== "number" ||
			symbols.generation <= initialStatus.generation
		) {
			throw new Error(
				`${label} did not advance generation after the post-ready mutation`,
			);
		}
		return symbols.generation;
	} finally {
		subscription.dispose();
	}
}

async function waitForEvent(events, predicate, label) {
	const deadline = Date.now() + 10_000;
	while (Date.now() <= deadline) {
		const event = events.find(predicate);
		if (event) {
			return event;
		}
		await new Promise((resolveDelay) => setTimeout(resolveDelay, 25));
	}
	throw new Error(`${label} did not publish the expected workspace event`);
}
