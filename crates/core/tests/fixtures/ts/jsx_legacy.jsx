// Plain JSX (no types) — exercises .jsx extension stripping and the same
// intrinsic / uppercase / expression-identifier surface as the .tsx variant.

import { render } from "react-dom";
import { Greeting } from "./greeting";

function App() {
	const name = "world";
	return (
		<main className="app">
			<Greeting>{name}</Greeting>
			<footer>built</footer>
		</main>
	);
}

render(<App />, document.getElementById("root"));
