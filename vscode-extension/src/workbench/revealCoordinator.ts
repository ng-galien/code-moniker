export interface RevealToken {
	generation: number;
	target: string;
}

// Coordinates asynchronous tree reveals without a global "ignore selection"
// flag. Only the exact selection caused by reveal() is consumed; every other
// selection remains a real user action and invalidates older work.
export class RevealCoordinator {
	private generation = 0;
	private pendingTarget?: string;
	private programmaticTarget?: string;

	get pending(): string | undefined {
		return this.pendingTarget;
	}

	begin(target: string): RevealToken {
		this.pendingTarget = target;
		return { generation: ++this.generation, target };
	}

	isCurrent(token: RevealToken): boolean {
		return token.generation === this.generation && token.target === this.pendingTarget;
	}

	markProgrammatic(token: RevealToken): boolean {
		if (!this.isCurrent(token)) return false;
		this.programmaticTarget = token.target;
		return true;
	}

	finish(token: RevealToken): void {
		if (this.isCurrent(token)) this.pendingTarget = undefined;
	}

	revealFailed(token: RevealToken): void {
		if (this.programmaticTarget === token.target) this.programmaticTarget = undefined;
		this.finish(token);
	}

	consumeSelection(uri: string): boolean {
		if (this.programmaticTarget === uri) {
			this.programmaticTarget = undefined;
			return true;
		}
		this.cancel();
		return false;
	}

	cancel(): void {
		this.generation++;
		this.pendingTarget = undefined;
		this.programmaticTarget = undefined;
	}
}
