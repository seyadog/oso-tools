import { defineConfig } from "vite";
import { execSync } from "node:child_process";
import { readFileSync } from "node:fs";

// The version people see (top right of the window) is the one in package.json, with the commit it was built from,
// so "which oso do you have?" has one answer.
const version: string = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")).version;
const build = (() => {
  try {
    return execSync("git rev-parse --short HEAD", { stdio: ["ignore", "pipe", "ignore"] }).toString().trim();
  } catch {
    return "";
  }
})();

export default defineConfig({
  clearScreen: false,
  server: { port: 1420, strictPort: true, fs: { allow: [".."] } },
  build: { target: "es2022", outDir: "dist" },
  define: { __APP_VERSION__: JSON.stringify(version), __APP_BUILD__: JSON.stringify(build) },
  test: { environment: "jsdom", include: ["tests/**/*.test.ts"] },
});
