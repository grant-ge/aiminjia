#!/usr/bin/env node
// Cross-platform guard that runs before `tauri dev` / `tauri build`.
// Fast-path: if src-tauri/resources/runtime/<platform>/bundled-version.json
// already matches scripts/runtime-sources.json, exit 0 immediately.
// Slow-path: dispatch to prepare-bundled-runtime.{sh,ps1} to download + extract.
//
// Re-running on a populated tree costs only two JSON reads — safe to keep as a
// "prebuild" hook so new worktrees / fresh checkouts self-heal without the user
// having to remember to invoke the prepare script.
//
// Set SKIP_BUNDLED_RUNTIME=1 to bypass entirely (e.g. CI jobs that prepare it
// out-of-band).

import { existsSync, readFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { platform, arch } from "node:os";

if (process.env.SKIP_BUNDLED_RUNTIME === "1") {
  console.log("[ensure-runtime] SKIP_BUNDLED_RUNTIME=1, skipping");
  process.exit(0);
}

const scriptDir = dirname(fileURLToPath(import.meta.url));
const projectDir = dirname(scriptDir);

function detectPlatform() {
  const p = platform();
  const a = arch();
  if (p === "darwin" && a === "arm64") return "darwin-arm64";
  if (p === "darwin" && a === "x64") return "darwin-x64";
  if (p === "win32" && a === "x64") return "win32-x64";
  return null;
}

const plat = detectPlatform();
if (!plat) {
  console.log(
    `[ensure-runtime] unsupported platform ${platform()}-${arch()}, skipping (build will fail if bundled runtime is required)`,
  );
  process.exit(0);
}

const sourcesPath = join(scriptDir, "runtime-sources.json");
if (!existsSync(sourcesPath)) {
  console.error(`[ensure-runtime] missing ${sourcesPath}`);
  process.exit(1);
}
const wantVersion = JSON.parse(readFileSync(sourcesPath, "utf8")).bundleVersion;

const outDir = join(projectDir, "src-tauri", "resources", "runtime", plat);
const versionFile = join(outDir, "bundled-version.json");
if (existsSync(versionFile)) {
  try {
    const got = JSON.parse(readFileSync(versionFile, "utf8")).bundleVersion;
    if (got === wantVersion) {
      // Quiet — this is the hot path on every dev/build invocation.
      process.exit(0);
    }
    console.log(
      `[ensure-runtime] ${plat} at ${got}, want ${wantVersion} — rebuilding`,
    );
  } catch (err) {
    console.log(
      `[ensure-runtime] ${versionFile} unreadable (${err.message}) — rebuilding`,
    );
  }
} else {
  console.log(
    `[ensure-runtime] ${plat} runtime missing — running prepare script (~85MB first time, cached afterwards)`,
  );
}

const isWin = platform() === "win32";
const cmd = isWin ? "powershell" : "bash";
const args = isWin
  ? [
      "-NoProfile",
      "-ExecutionPolicy",
      "Bypass",
      "-File",
      join(scriptDir, "prepare-bundled-runtime.ps1"),
    ]
  : [join(scriptDir, "prepare-bundled-runtime.sh")];

const result = spawnSync(cmd, args, {
  stdio: "inherit",
  cwd: projectDir,
  env: process.env,
});
if (result.error) {
  console.error(`[ensure-runtime] failed to spawn ${cmd}: ${result.error.message}`);
  process.exit(1);
}
process.exit(result.status ?? 1);
