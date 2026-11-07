import { defineConfig } from "@playwright/test";

// Runs against the production build in a plain browser: the app detects that
// it is outside Tauri and renders labelled demo data (see src/lib/demo.ts).
export default defineConfig({
  testDir: "e2e",
  outputDir: "e2e/.results",
  reporter: [["list"]],
  // The frame-gap test measures real vsync timing, which shared CI runners jitter.
  // Two retries absorb that; a real regression fails every attempt.
  retries: process.env.CI ? 2 : 0,
  use: { baseURL: "http://localhost:1420", browserName: "webkit" },
  webServer: { command: "npm run preview", url: "http://localhost:1420", reuseExistingServer: true },
});
