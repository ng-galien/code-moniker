import { GenerationCache } from "../daemon/cache";
import { ChangeReviewResult } from "../daemon/model";
import { DaemonSession } from "../daemon/session";

// Data access for the semantic change review (HEAD..worktree facts served
// from the live snapshot). Cached per workspace generation like every other
// daemon-backed repository.
export class ChangesRepository {
	private readonly cache: GenerationCache;

	constructor(private readonly session: DaemonSession) {
		this.cache = new GenerationCache(session);
	}

	get ready(): boolean {
		return this.session.ready;
	}

	async review(): Promise<ChangeReviewResult | undefined> {
		return this.cache.fetch("review", async () => {
			const response = await this.session.query({
				op: "change_review",
				workspace: null,
			});
			return response.result.kind === "change_review" ? response.result.data : undefined;
		});
	}
}
