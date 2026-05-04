import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { HelperGate } from "./screens/HelperGate";
import { UpdateChecker } from "./components/UpdateChecker";
import { loadPlatform } from "./platform";
import "./index.css";

// Resolve target OS before mounting so platform-aware copy renders correctly
// on first paint instead of flashing macOS strings on Windows.
await loadPlatform();

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <HelperGate>
      <App />
    </HelperGate>
    <UpdateChecker />
  </React.StrictMode>,
);
