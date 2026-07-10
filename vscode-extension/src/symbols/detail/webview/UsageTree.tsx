import { useEffect, useMemo } from "react";

import type { UsageSummaryDto } from "../../../daemon/model";
import { CodeBlock } from "../../../webview-lib/CodeBlock";
import type { HighlightedUsageDto } from "../panel";
import {
	actionMeta,
	bucketMeta,
	fileKind,
	groupUsages,
	kindLabel,
	occurrenceTooltip,
	splitFile,
	usageAction,
	usageBuckets,
	compactSymbol,
	type UsageContextGroup,
	type UsageDirectionScope,
	type UsageFileGroup,
	type UsageOccurrence,
	type UsageSummaryKind,
} from "../usageModel";
import { Details, OpenSourceButton, Section } from "./common";
import { useViewActions } from "./viewContext";

export function UsagesSection({
	title,
	rows,
	summary,
	scope,
	typeTarget,
}: {
	title: string;
	rows: HighlightedUsageDto[];
	summary: UsageSummaryDto | null;
	scope: UsageDirectionScope;
	typeTarget: boolean;
}) {
	return (
		<Section title={`${title} (${rows.length})`}>
			{summary?.dominant_prefix && (
				<div className="summary">{summary.shared_helper_signal + " · " + summary.dominant_prefix}</div>
			)}
			{rows.length === 0 ? (
				<div className="empty-row">none</div>
			) : (
				<UsageTree rows={rows} scope={scope} typeTarget={typeTarget} />
			)}
		</Section>
	);
}

function UsageTree({
	rows,
	scope,
	typeTarget,
}: {
	rows: HighlightedUsageDto[];
	scope: UsageDirectionScope;
	typeTarget: boolean;
}) {
	const buckets = useMemo(() => usageBuckets(rows, typeTarget), [rows, typeTarget]);
	return (
		<div className="usage-tree">
			{buckets
				.filter((bucket) => bucket.rows.length > 0)
				.map((bucket) => (
					<Details
						key={bucket.kind}
						className="usage-bucket"
						stateKey={`${scope}:bucket:${bucket.kind}`}
						summary={
							<SummaryLine
								label={bucket.label}
								meta={bucketMeta(bucket.rows)}
								count={bucket.rows.length}
								kind={bucket.kind}
							/>
						}
					>
						{groupUsages(bucket.rows, bucket.kind, scope).map((group) => (
							<FileNode key={group.file} group={group} />
						))}
					</Details>
				))}
		</div>
	);
}

function FileNode({ group }: { group: UsageFileGroup }) {
	const file = splitFile(group.file);
	return (
		<Details
			className="usage-file"
			stateKey={`${group.scope}:file:${group.bucket}:${group.file}`}
			title={group.file}
			summary={
				<span className="usage-summary-line">
					<span className="usage-summary-kind usage-kind-file">{fileKind(group.file)}</span>
					<span className="usage-summary-text">
						<span className="usage-summary-label">{file.name}</span>
						{file.dir && <span className="usage-summary-meta">{file.dir}</span>}
					</span>
					<span className="usage-summary-count">{group.rows.length}</span>
				</span>
			}
		>
			{group.contexts.map((context) => (
				<ContextNode key={context.label} group={group} context={context} />
			))}
		</Details>
	);
}

function ContextNode({ group, context }: { group: UsageFileGroup; context: UsageContextGroup }) {
	return (
		<Details
			className="usage-context"
			stateKey={`${group.scope}:context:${group.bucket}:${group.file}:${context.label}`}
			title={context.label}
			summary={
				<SummaryLine
					label={compactSymbol(context.label)}
					meta={actionMeta(context.rows)}
					count={context.rows.length}
					kind="context"
				/>
			}
		>
			{context.occurrences.map((occurrence) => (
				<UsageItem key={occurrence.key} occurrence={occurrence} />
			))}
		</Details>
	);
}

function SummaryLine({
	label,
	meta,
	count,
	kind,
}: {
	label: string;
	meta: string;
	count: number;
	kind: UsageSummaryKind;
}) {
	return (
		<span className="usage-summary-line">
			<span className={`usage-summary-kind usage-kind-${kind}`}>{kindLabel(kind)}</span>
			<span className="usage-summary-text">
				<span className="usage-summary-label">{label || "unknown"}</span>
				{meta && <span className="usage-summary-meta">{meta}</span>}
			</span>
			<span className="usage-summary-count">{count}</span>
		</span>
	);
}

function UsageItem({ occurrence }: { occurrence: UsageOccurrence }) {
	const view = useViewActions();
	const open = view.openPreviews.has(occurrence.key);
	const hasCode = Boolean(occurrence.sample.line_range);
	const refs = occurrence.rows.length;
	const hint = !hasCode
		? `${refs} ref${refs > 1 ? "s" : ""}`
		: open
			? "Hide code"
			: refs > 1
				? `Show code · ${refs} refs`
				: "Show code";
	return (
		<div className={open ? "usage-item open" : "usage-item"}>
			<button
				type="button"
				className="usage-leaf"
				title={occurrenceTooltip(occurrence)}
				aria-expanded={open}
				onClick={() => view.setPreviewOpen(occurrence.key, !open)}
			>
				<span className="usage-action">{usageAction(occurrence.kind)}</span>
				<span className="usage-actor">{occurrence.label}</span>
				<span className="usage-preview-hint">{hint}</span>
			</button>
			{open && <UsagePreview occurrence={occurrence} />}
		</div>
	);
}

function UsagePreview({ occurrence }: { occurrence: UsageOccurrence }) {
	const view = useViewActions();
	const snippet =
		occurrence.sample.snippet !== undefined
			? occurrence.sample.snippet
			: view.snippets.get(occurrence.key);
	const loading = snippet === undefined && occurrence.sample.line_range != null;
	useEffect(() => {
		if (loading) {
			view.ensureSnippet(occurrence);
		}
	}, [loading, occurrence, view]);
	return (
		<div className="usage-preview">
			{snippet ? (
				<CodeBlock source={snippet} active={occurrence.sample.line_range} compact />
			) : loading ? (
				<div className="empty-row">Loading source...</div>
			) : (
				<div className="empty-row">No preview available.</div>
			)}
			<OpenSourceButton source={occurrence.sample} text="Open source" />
		</div>
	);
}
