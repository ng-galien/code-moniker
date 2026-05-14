// Counter widget — exercises TSX surface: typed props, intrinsic tags,
// uppercase components, JSX expression identifiers, event handlers.

import { useState } from "react";
import { Button } from "./button";

export interface CounterProps {
	label: string;
	initial?: number;
}

export function Counter({ label, initial = 0 }: CounterProps) {
	const [count, setCount] = useState(initial);
	return (
		<div className="counter">
			<span>
				{label}: {count}
			</span>
			<Button onClick={() => setCount(count + 1)}>+</Button>
		</div>
	);
}

export function CounterList({ labels }: { labels: string[] }) {
	return (
		<ul>
			{labels.map((l) => (
				<li key={l}>
					<Counter label={l} />
				</li>
			))}
		</ul>
	);
}
