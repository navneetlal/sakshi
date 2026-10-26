import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Tauri loads the dev server from a fixed port and the built files from dist/.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: "127.0.0.1",
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    target: "safari16",
    outDir: "dist",
  },
});
