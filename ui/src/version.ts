// The version of this build, shown at the top right of the window. It comes from package.json (see vite.config.ts).

declare const __APP_VERSION__: string | undefined;
declare const __APP_BUILD__: string | undefined;

export const VERSION: string = typeof __APP_VERSION__ === "string" ? __APP_VERSION__ : "dev";
export const BUILD: string = typeof __APP_BUILD__ === "string" ? __APP_BUILD__ : "";

/** `v0.3.0` and, when it is known, the commit: `v0.3.0 · a1b2c3d`. */
export function versionLabel(version = VERSION, build = BUILD): string {
  return build ? `v${version} · ${build}` : `v${version}`;
}
