import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { AppStateProvider } from "./state";
import { applyTheme, loadTheme, onSystemSchemeChange } from "./theme";
import "./styles.css";

// Before the first render, so the window never flashes the wrong colours.
applyTheme(loadTheme());
onSystemSchemeChange(() => applyTheme(loadTheme()));

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

createRoot(root).render(
  <StrictMode>
    <AppStateProvider>
      <App />
    </AppStateProvider>
  </StrictMode>,
);
