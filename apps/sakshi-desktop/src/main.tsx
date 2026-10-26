import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import { loadHome } from "./paths";
import { AppStateProvider } from "./state";
import { applyTheme, loadTheme, onSystemSchemeChange } from "./theme";
// The brand's typefaces, bundled into the app (licences in public/licenses).
import "@fontsource-variable/inter";
import "@fontsource/space-grotesk/500.css";
import "@fontsource-variable/noto-sans-devanagari";
import "./styles.css";

// Before the first render, so the window never flashes the wrong colours
// and paths show "~" from the start.
applyTheme(loadTheme());
onSystemSchemeChange(() => applyTheme(loadTheme()));
await loadHome();

const root = document.getElementById("root");
if (!root) throw new Error("index.html has no #root element");

createRoot(root).render(
  <StrictMode>
    <AppStateProvider>
      <App />
    </AppStateProvider>
  </StrictMode>,
);
