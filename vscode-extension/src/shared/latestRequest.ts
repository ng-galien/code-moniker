// Small latest-wins coordinator for asynchronous UI lookups. A token is
// captured before work starts and checked after every await boundary.
export class LatestRequest {
	private generation = 0;

	begin(): number {
		return ++this.generation;
	}

	invalidate(): void {
		this.generation++;
	}

	isCurrent(token: number): boolean {
		return token === this.generation;
	}
}
