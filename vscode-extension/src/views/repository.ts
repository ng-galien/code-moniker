import { ViewSummaryDto } from "../daemon/model";
import { DaemonSession } from "../daemon/session";
import { ViewDetail } from "./nodes";

export class ViewsRepository {
	constructor(private readonly session: DaemonSession) {}

	get ready(): boolean {
		return this.session.ready;
	}

	async listViews(): Promise<ViewSummaryDto[]> {
		const response = await this.session.query({
			op: "view_read",
			uri: "workspace/views",
			scheme: null,
			context_lines: 2,
			include_code: false,
		});
		return response.result.kind === "view_read" && response.result.data.kind === "list"
			? response.result.data.views
			: [];
	}

	async readView(id: string): Promise<ViewDetail | undefined> {
		const response = await this.session.query({
			op: "view_read",
			uri: `workspace/views/${id}`,
			scheme: null,
			context_lines: 2,
			include_code: false,
		});
		return response.result.kind === "view_read" && response.result.data.kind === "detail"
			? response.result.data
			: undefined;
	}
}
