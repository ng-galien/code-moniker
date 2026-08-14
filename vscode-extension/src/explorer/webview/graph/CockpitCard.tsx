import { Handle, Position, useStore } from "@xyflow/react";

import type { SymbolDto } from "../../../daemon/model";
import { parseCallableName } from "../../../webview-lib/parse";
import { glyphClass, symbolGlyph } from "../../../webview-lib/symbolGlyph";
import { cockpitZoomLevel } from "./zoom";

export interface CockpitCardData extends Record<string, unknown> {
	symbol: SymbolDto;
	focus: boolean;
	depth: number;
	direction: "focus" | "incoming" | "outgoing" | "pinned";
	loaded: boolean;
	loading: boolean;
	hiddenIncoming: number;
	hiddenOutgoing: number;
	truncated: number;
	expandable: boolean;
	incomingLimited: boolean;
	outgoingLimited: boolean;
	pinned: boolean;
	selected: boolean;
	onFocus: (uri: string) => void;
	onInspect: (uri: string) => void;
	onOpen: (symbol: SymbolDto) => void;
	onExpand: (uri: string, direction: "incoming" | "outgoing") => void;
	onTogglePin: (symbol: SymbolDto, pinned: boolean) => void;
}

export function CockpitCard({ data, dragging = false }: { data: CockpitCardData; dragging?: boolean }) {
	const { symbol } = data;
	const name = parseCallableName(symbol.name);
	const zoomLevel = useStore((state) => cockpitZoomLevel(state.transform[2]));
	const role = data.direction === "incoming"
		? "caller"
		: data.direction === "outgoing"
			? "dependency"
			: data.direction;
	return (
		<div
			className={[
				"cockpit-card",
				`zoom-${zoomLevel}`,
				`direction-${data.direction}`,
				data.focus ? "focus" : "",
				data.selected ? "selected" : "",
				data.pinned ? "pinned" : "",
				dragging ? "is-dragging" : "",
			].filter(Boolean).join(" ")}
		>
			<Handle type="target" position={Position.Left} className="cockpit-port" />
			<span className="cockpit-drag-handle" aria-hidden="true" title="Drag to reposition">
				⠿
			</span>
			<button
				type="button"
				className="cockpit-card-main nodrag"
				title={`${symbol.name} — inspect code without changing the graph focus`}
				onClick={() => data.onInspect(symbol.uri)}
			>
				<span className={glyphClass(symbol.kind)}>{symbolGlyph(symbol.kind)}</span>
				<span className="cockpit-card-copy">
					<span className="cockpit-card-name">{name.base}</span>
					{!data.focus && <span className="cockpit-card-role">{role}</span>}
					<span className="cockpit-card-meta">
						{symbol.kind} · {symbol.file.split("/").pop()}
					</span>
				</span>
				{data.focus && <span className="cockpit-focus-label">focus</span>}
			</button>
			<div className="cockpit-card-actions nodrag" role="toolbar" aria-label={`Actions for ${symbol.name}`}>
				<button type="button" className="cockpit-card-code" title="Review code beside the graph" onClick={() => data.onInspect(symbol.uri)}>Inspect</button>
				{!data.focus && (
					<button type="button" className="cockpit-card-refocus" title="Make this symbol the graph focus" onClick={() => data.onFocus(symbol.uri)}>
						Refocus
					</button>
				)}
				<button type="button" onClick={() => data.onOpen(symbol)}>Editor</button>
				{!data.focus && (
					<button
						type="button"
						aria-pressed={data.pinned}
						onClick={() => data.onTogglePin(symbol, !data.pinned)}
					>
						{data.pinned ? "Unpin" : "Pin"}
					</button>
				)}
			</div>
			{(data.hiddenIncoming > 0 || data.hiddenOutgoing > 0 || data.loading || data.truncated > 0) && (
				<div className="cockpit-expand-row nodrag">
					{data.loading && <span className="cockpit-loading-neighbors">Loading…</span>}
					{data.hiddenIncoming > 0 && (
						<ExpandButton
							direction="incoming"
							count={data.hiddenIncoming}
							disabled={data.loading || data.incomingLimited}
							onClick={() => data.onExpand(symbol.uri, "incoming")}
						/>
					)}
					{data.hiddenOutgoing > 0 && (
						<ExpandButton
							direction="outgoing"
							count={data.hiddenOutgoing}
							disabled={data.loading || data.outgoingLimited}
							onClick={() => data.onExpand(symbol.uri, "outgoing")}
						/>
					)}
					{data.truncated > 0 && (
						<span className="cockpit-index-limit" title="Additional indexed relations were not returned">
							▲ {data.truncated} limited
						</span>
					)}
				</div>
			)}
			<Handle type="source" position={Position.Right} className="cockpit-port" />
		</div>
	);
}

function ExpandButton({
	direction,
	count,
	disabled,
	onClick,
}: {
	direction: "incoming" | "outgoing";
	count: number;
	disabled: boolean;
	onClick: () => void;
}) {
	const label = direction === "incoming" ? "upstream" : "downstream";
	return (
		<button
			type="button"
			className={`cockpit-expand ${direction}`}
			disabled={disabled}
			title={`${count} hidden ${label} symbols; reveal the next ${Math.min(4, count)}`}
			onClick={onClick}
		>
			{direction === "incoming" ? "←" : "→"} +{count}
		</button>
	);
}
