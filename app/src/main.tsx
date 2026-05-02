import React from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { HelperGate } from "./screens/HelperGate";
import { UpdateChecker } from "./components/UpdateChecker";
import "./index.css";

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <HelperGate>
      <App />
    </HelperGate>
    <UpdateChecker />
  </React.StrictMode>,
);
