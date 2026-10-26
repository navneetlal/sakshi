// Palettes and light/dark mode. Components only use the CSS tokens; which
// colours those tokens hold is decided here and in palettes/*.css.
//
// To add a palette: copy a file in palettes/, give it a new id and colours,
// import it below, and add it to PALETTES. To change the default, change
// DEFAULT_PALETTE.

import "./base.css";
import "./palettes/graphite-amber.css";
import "./palettes/paper-ink.css";
import "./palettes/sakshi.css";
import "./palettes/stone-sage.css";

export interface Palette {
  id: string;
  name: string;
  description: string;
}

export const PALETTES: Palette[] = [
  {
    id: "sakshi",
    name: "Sakshi",
    description: "The brand: Ink and Paper, with Tally for recording only.",
  },
  {
    id: "graphite-amber",
    name: "Graphite & Amber",
    description: "Warm neutrals with amber for actions.",
  },
  {
    id: "paper-ink",
    name: "Paper & Ink",
    description: "Neutral paper and charcoal with a muted ink blue.",
  },
  {
    id: "stone-sage",
    name: "Stone & Sage",
    description: "Soft stone with a quiet sage green.",
  },
];

export const DEFAULT_PALETTE = "sakshi";

export type Mode = "system" | "light" | "dark";
export type Scheme = "light" | "dark";

export interface ThemeChoice {
  palette: string;
  mode: Mode;
}

const STORAGE_KEY = "sakshi.theme";
const darkQuery = window.matchMedia("(prefers-color-scheme: dark)");

/** The saved choice, or the defaults. Storage can be unavailable. */
export function loadTheme(): ThemeChoice {
  const fallback: ThemeChoice = { palette: DEFAULT_PALETTE, mode: "system" };
  try {
    const saved = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null") as Partial<ThemeChoice> | null;
    return {
      palette: PALETTES.some((p) => p.id === saved?.palette) ? saved!.palette! : fallback.palette,
      mode: saved?.mode === "light" || saved?.mode === "dark" ? saved.mode : "system",
    };
  } catch {
    return fallback;
  }
}

export function saveTheme(choice: ThemeChoice): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(choice));
  } catch {
    // Not remembered this time; the theme still applies.
  }
}

export function resolveScheme(mode: Mode): Scheme {
  if (mode === "system") return darkQuery.matches ? "dark" : "light";
  return mode;
}

/** Sets the palette and scheme on <html>, which every token reads. */
export function applyTheme(choice: ThemeChoice): void {
  const root = document.documentElement;
  root.dataset.palette = choice.palette;
  root.dataset.scheme = resolveScheme(choice.mode);
}

/** Calls `onChange` when the system switches between light and dark. */
export function onSystemSchemeChange(onChange: () => void): () => void {
  darkQuery.addEventListener("change", onChange);
  return () => darkQuery.removeEventListener("change", onChange);
}
