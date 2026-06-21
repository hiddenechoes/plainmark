import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and a predictable dev server; see tauri.conf.json.
export default defineConfig({
  plugins: [react()],
  // Tauri controls the window; don't let Vite hijack the terminal.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
  build: {
    // Match the Tauri webview baseline so syntax stays compatible.
    target: "es2021",
    sourcemap: true,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.{ts,tsx}"],
  },
});
