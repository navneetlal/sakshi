// Paths for display: the home folder shown as "~", as Finder and the
// terminal do. Snippets meant for copying keep their absolute paths,
// because client config files don't expand "~".

import { homeDir } from "@tauri-apps/api/path";

let home: string | null = null;

/** Looks up the home folder once, before the first render. */
export async function loadHome(): Promise<void> {
  try {
    home = (await homeDir()).replace(/\/+$/, "");
  } catch {
    home = null;
  }
}

/** `text` with every path under the home folder starting at "~/". */
export function tidy(text: string): string {
  return home ? text.split(`${home}/`).join("~/") : text;
}
