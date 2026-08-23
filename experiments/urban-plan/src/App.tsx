import { useEffect, useMemo, useState } from "react";

import { City, type CityPick } from "./City.tsx";
import { Controls } from "./Controls.tsx";
import { DEFAULT_LAYOUT, loadLayoutConfig, saveLayoutConfig, type LayoutConfig } from "./config.ts";
import { crateOptions, snapshotFromTree } from "./layout.ts";
import type { SceneDistrict, SceneSnapshot } from "./scene.ts";

export function App() {
	const [raw, setRaw] = useState<SceneSnapshot | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [picked, setPicked] = useState<CityPick>(null);
	const [pinned, setPinned] = useState<CityPick>(null);
	const [focus, setFocus] = useState<SceneDistrict | null>(null);
	const [config, setConfig] = useState<LayoutConfig>(DEFAULT_LAYOUT);

	useEffect(() => {
		setConfig(loadLayoutConfig());
		fetch("/snapshot.json")
			.then((response) => {
				if (!response.ok) {
					throw new Error(`snapshot ${response.status}`);
				}
				return response.json() as Promise<SceneSnapshot>;
			})
			.then(setRaw)
			.catch((cause: unknown) => {
				setError(cause instanceof Error ? cause.message : String(cause));
			});
	}, []);

	useEffect(() => {
		saveLayoutConfig(config);
	}, [config]);

	const snapshot = useMemo(() => {
		if (!raw?.tree) {
			return raw;
		}
		return snapshotFromTree({
			generation: raw.generation,
			prefix: raw.prefix,
			tree: raw.tree,
			edges: raw.edges ?? [],
			config,
		});
	}, [raw, config]);

	useEffect(() => {
		setFocus(null);
	}, [config.maxDepth, config.citySize, config.hiddenIds, config.includeSrc, config.includeTests]);

	const crates = raw?.tree ? crateOptions(raw.tree) : [];
	const shown = pinned ?? picked;

	return (
		<div className="app">
			<div className="stage">
			<header className="hud">
				<h1>code-moniker · 2.5D urban plan</h1>
				<p className="meta">
					{error
						? error
						: snapshot
							? [
									`prefix ${snapshot.prefix}`,
									`generation ${snapshot.generation ?? "none"}`,
									`${snapshot.districts.length} quartiers`,
									`${snapshot.buildings.length} bâtiments`,
									`${snapshot.streets.length} rues`,
									`${snapshot.roads.length} artères`,
								].join(" · ")
							: "loading snapshot…"}
				</p>
				{shown?.kind === "road" ? (
					<div className="card">
						<p className="card-title">
							{shown.road.fromLabel} <span>→</span> {shown.road.toLabel}
						</p>
						<p className="card-meta">
							{shown.road.count} liens
							{shown.road.kinds.length > 0 ? ` · ${shown.road.kinds.join(", ")}` : ""}
						</p>
						<p className="card-hint">artère sélectionnée · clic dans le vide pour tout réafficher</p>
					</div>
				) : (
					<p className="pick">
						{shown
							? describePick(shown)
							: "drag = pan · clic droit = orbit · clic artère = isoler"}
					</p>
				)}
				{focus || pinned ? (
					<button
						type="button"
						className="reset"
						onClick={() => {
							setFocus(null);
							setPicked(null);
							setPinned(null);
						}}
					>
						vue ville
					</button>
				) : null}
			</header>
			{snapshot ? (
				<City
					snapshot={snapshot}
					picked={picked}
					pinned={pinned}
					focus={focus}
					onPick={setPicked}
					onPin={setPinned}
					onFocus={setFocus}
				/>
			) : null}
			</div>
			{raw ? <Controls config={config} crates={crates} onChange={setConfig} /> : null}
		</div>
	);
}

function describePick(picked: CityPick): string | null {
	if (!picked) {
		return null;
	}
	if (picked.kind === "building") {
		const building = picked.building;
		return `${building.label}  ${building.kind}  defs=${building.defs}  ${building.id}`;
	}
	if (picked.kind === "road") {
		const road = picked.road;
		return `artère  ${shortId(road.from)} → ${shortId(road.to)}  count=${road.count}  ${road.kinds.join(",")}`;
	}
	const district = picked.district;
	return `quartier ${district.label}  ${district.kind}  defs=${district.defs}  ${district.id}`;
}

function shortId(id: string): string {
	const part = id.split("/").at(-1) ?? id;
	const cut = part.indexOf(":");
	return cut >= 0 ? part.slice(cut + 1) : part;
}
