export interface ExpansionRequestToken {
	uri: string;
	requestId: string;
	rootFocus: string;
	generation: number;
}

export interface PendingExpansion extends ExpansionRequestToken {
	direction: "incoming" | "outgoing";
}

// Owns the lifecycle of asynchronous graph expansions. reset() invalidates
// every response that was issued before undo, restore, or a root refocus.
export class ExpansionCoordinator {
	private generation = 0;
	private sequence = 0;
	private readonly pending = new Map<string, PendingExpansion>();

	reset(): void {
		this.generation++;
		this.pending.clear();
	}

	begin(uri: string, rootFocus: string, direction: PendingExpansion["direction"]): PendingExpansion {
		const request = {
			uri,
			requestId: `${this.generation}:${++this.sequence}`,
			rootFocus,
			generation: this.generation,
			direction,
		};
		this.pending.set(uri, request);
		return request;
	}

	take(response: ExpansionRequestToken, currentRootFocus: string): PendingExpansion | undefined {
		const request = this.pending.get(response.uri);
		if (
			!request ||
			request.requestId !== response.requestId ||
			request.rootFocus !== response.rootFocus ||
			request.generation !== response.generation ||
			response.rootFocus !== currentRootFocus ||
			response.generation !== this.generation
		) return undefined;
		this.pending.delete(response.uri);
		return request;
	}
}
