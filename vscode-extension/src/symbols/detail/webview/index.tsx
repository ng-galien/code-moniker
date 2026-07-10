import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./detail.css";

const container = document.getElementById("root");
if (container) {
	createRoot(container).render(<App />);
}
