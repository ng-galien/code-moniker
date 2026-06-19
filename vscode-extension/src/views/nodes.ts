import {
	ViewBoundaryDto,
	ViewEvidenceDto,
	ViewGotchaDto,
	ViewReadResult,
	ViewRuleDto,
	ViewSummaryDto,
} from "../daemon/model";

export type ViewDetail = Extract<ViewReadResult, { kind: "detail" }>;

export type ViewTreeNode =
	| ViewSummaryNode
	| ViewSectionNode
	| ViewBoundaryNode
	| ViewGotchaNode
	| ViewEvidenceNode
	| ViewRuleNode
	| ViewInfoNode;

export interface ViewSummaryNode {
	kind: "view";
	view: ViewSummaryDto;
}

export interface ViewSectionNode {
	kind: "section";
	id: "rules" | "boundaries" | "gotchas";
	view: ViewDetail;
	label: string;
}

export interface ViewBoundaryNode {
	kind: "boundary";
	boundary: ViewBoundaryDto;
}

export interface ViewGotchaNode {
	kind: "gotcha";
	gotcha: ViewGotchaDto;
}

export interface ViewEvidenceNode {
	kind: "evidence";
	evidence: ViewEvidenceDto;
}

export interface ViewRuleNode {
	kind: "rule";
	rule: ViewRuleDto;
}

export interface ViewInfoNode {
	kind: "info";
	label: string;
}
