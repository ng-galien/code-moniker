import { useEffect, useState } from "react";

import { City } from "./City.tsx";
import type { SceneBuilding, SceneSnapshot } from "./scene.ts";

export function App() {
	const [snapshot, setSnapshot] = useState<SceneSnapshot | null>(null);
	const [error, setError] = useState<string | null>(null);
	const [picked, setPicked] = useState<SceneBuilding | null>(null);

	useEffect(() => {
		fetch("/snapshot.json")
			.then((response) => {
				if (!response.ok) {
					throw new Error(`snapshot ${response.status}`);
				}
				return response.json() as Promise<SceneSnapshot>;
			})
			.then(setSnapshot)
			.catch((cause: unknown) => {
				setError(cause instanceof Error ? cause.message : String(cause));
			});
	}, []);

	return (
		<div className="app">
			<header className="hud">
				<h1>code-moniker · 2.5D urban plan</h1>
				<p className="meta">
					{error
						? error
						: snapshot
							? [
									`prefix ${snapshot.prefix}`,
									`generation ${snapshot.generation ?? "none"}`,
									`${snapshot.buildings.length} buildings`,
									`${snapshot.roads.length} roads`,
									snapshot.coverage.roadsOmitted
										? `${snapshot.coverage.roadsOmitted} omitted`
										: "",
								]
									.filter(Boolean)
									.join(" · ")
							: "loading snapshot…"}
				</p>
				<p className="pick">
					{picked
						? `${picked.label}  ${picked.kind}  defs=${picked.defs}  ${picked.id}`
						: "hover a building · drag to orbit · scroll to zoom"}
				</p>
			</header>
			{snapshot ? (
				<City snapshot={snapshot} picked={picked} onPick={setPicked} />
			) : null}
		</div>
	);
}
