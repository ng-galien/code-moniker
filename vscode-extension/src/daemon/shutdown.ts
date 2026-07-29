export async function withShutdownCleanup(
	stop: () => Promise<void>,
	cleanup: () => void,
): Promise<void> {
	try {
		await stop();
	} finally {
		cleanup();
	}
}
