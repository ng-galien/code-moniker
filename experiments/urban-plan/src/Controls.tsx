import { BUILDING_KIND_OPTIONS, DEFAULT_LAYOUT, type LayoutConfig } from "./config.ts";

type ControlsProps = {
	config: LayoutConfig;
	crates: { id: string; name: string }[];
	onChange: (next: LayoutConfig) => void;
};

export function Controls({ config, crates, onChange }: ControlsProps) {
	const set = <K extends keyof LayoutConfig>(key: K, value: LayoutConfig[K]) => {
		onChange({ ...config, [key]: value });
	};

	return (
		<aside className="panel">
			<h2>réglages</h2>
			<p className="hint">live · HMR · localStorage</p>

			<section>
				<h3>agrégation</h3>
				<Slider
					label="profondeur"
					value={config.maxDepth}
					min={1}
					max={6}
					step={1}
					onChange={(value) => set("maxDepth", value)}
				/>
				<Slider
					label="defs min"
					value={config.minDefs}
					min={0}
					max={40}
					step={1}
					onChange={(value) => set("minDefs", value)}
				/>
				<label className="check">
					<input
						type="checkbox"
						checked={config.includeSrc}
						onChange={(event) => set("includeSrc", event.target.checked)}
					/>
					src
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.includeTests}
						onChange={(event) => set("includeTests", event.target.checked)}
					/>
					tests
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.includeBenches}
						onChange={(event) => set("includeBenches", event.target.checked)}
					/>
					benches
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.includeExamples}
						onChange={(event) => set("includeExamples", event.target.checked)}
					/>
					examples
				</label>
			</section>

			<section>
				<h3>frontières</h3>
				{crates.map((crate) => {
					const hidden = config.hiddenIds.includes(crate.id);
					return (
						<label className="check" key={crate.id}>
							<input
								type="checkbox"
								checked={!hidden}
								onChange={(event) => {
									onChange({
										...config,
										hiddenIds: event.target.checked
											? config.hiddenIds.filter((id) => id !== crate.id)
											: [...config.hiddenIds, crate.id],
									});
								}}
							/>
							{crate.name}
						</label>
					);
				})}
			</section>

			<section>
				<h3>métrique hauteur</h3>
				<label className="check">
					<input
						type="radio"
						name="heightMode"
						checked={config.heightMode === "log"}
						onChange={() => set("heightMode", "log")}
					/>
					log(defs)
				</label>
				<label className="check">
					<input
						type="radio"
						name="heightMode"
						checked={config.heightMode === "linear"}
						onChange={() => set("heightMode", "linear")}
					/>
					linéaire
				</label>
				<Slider
					label="échelle"
					value={config.heightScale}
					min={0.1}
					max={2}
					step={0.05}
					onChange={(value) => set("heightScale", value)}
				/>
				<Slider
					label="hauteur max"
					value={config.maxHeight}
					min={1}
					max={20}
					step={0.5}
					onChange={(value) => set("maxHeight", value)}
				/>
				<Slider
					label="emprise"
					value={config.footprintScale}
					min={0.4}
					max={2.5}
					step={0.05}
					onChange={(value) => set("footprintScale", value)}
				/>
			</section>

			<section>
				<h3>rues</h3>
				<Slider
					label="ville"
					value={config.citySize}
					min={40}
					max={220}
					step={5}
					onChange={(value) => set("citySize", value)}
				/>
				<Slider
					label="artères"
					value={config.avenueGap}
					min={1}
					max={10}
					step={0.1}
					onChange={(value) => set("avenueGap", value)}
				/>
				<Slider
					label="rues"
					value={config.streetGap}
					min={0.2}
					max={4}
					step={0.05}
					onChange={(value) => set("streetGap", value)}
				/>
				<Slider
					label="venelles"
					value={config.laneGap}
					min={0.1}
					max={2}
					step={0.05}
					onChange={(value) => set("laneGap", value)}
				/>
				<Slider
					label="côté crate min"
					value={config.minCrateSide}
					min={2}
					max={24}
					step={0.5}
					onChange={(value) => set("minCrateSide", value)}
				/>
				<Slider
					label="quartier min"
					value={config.minDistrictSide}
					min={0.4}
					max={8}
					step={0.1}
					onChange={(value) => set("minDistrictSide", value)}
				/>
			</section>

			<section>
				<h3>couplage</h3>
				<Slider
					label="artères max"
					value={config.maxRoads}
					min={0}
					max={40}
					step={1}
					onChange={(value) => set("maxRoads", value)}
				/>
				<Slider
					label="min count"
					value={config.minRoadCount}
					min={1}
					max={200}
					step={1}
					onChange={(value) => set("minRoadCount", value)}
				/>
				<label className="check">
					<input
						type="checkbox"
						checked={config.showAvenues}
						onChange={(event) => set("showAvenues", event.target.checked)}
					/>
					artères couplage
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.showStreets}
						onChange={(event) => set("showStreets", event.target.checked)}
					/>
					asphalte
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.showDistricts}
						onChange={(event) => set("showDistricts", event.target.checked)}
					/>
					dalles quartiers
				</label>
				<label className="check">
					<input
						type="checkbox"
						checked={config.showLabels}
						onChange={(event) => set("showLabels", event.target.checked)}
					/>
					labels
				</label>
			</section>

			<section>
				<h3>bâtiments</h3>
				{BUILDING_KIND_OPTIONS.map((kind) => (
					<label className="check" key={kind}>
						<input
							type="checkbox"
							checked={config.buildingKinds.includes(kind)}
							onChange={(event) => {
								onChange({
									...config,
									buildingKinds: event.target.checked
										? [...config.buildingKinds, kind]
										: config.buildingKinds.filter((item) => item !== kind),
								});
							}}
						/>
						{kind}
					</label>
				))}
			</section>

			<button type="button" className="reset" onClick={() => onChange(DEFAULT_LAYOUT)}>
				reset
			</button>
		</aside>
	);
}

function Slider({
	label,
	value,
	min,
	max,
	step,
	onChange,
}: {
	label: string;
	value: number;
	min: number;
	max: number;
	step: number;
	onChange: (value: number) => void;
}) {
	return (
		<label className="slider">
			<span>
				{label}
				<b>{format(value)}</b>
			</span>
			<input
				type="range"
				min={min}
				max={max}
				step={step}
				value={value}
				onChange={(event) => onChange(Number(event.target.value))}
			/>
		</label>
	);
}

function format(value: number): string {
	return Number.isInteger(value) ? String(value) : value.toFixed(2);
}
