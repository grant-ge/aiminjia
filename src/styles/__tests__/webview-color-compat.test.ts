import fs from "node:fs";
import path from "node:path";

import { describe, expect, it } from "vitest";

const SRC_ROOT = path.resolve(process.cwd(), "src");
const SOURCE_EXTENSIONS = new Set([".ts", ".tsx", ".css"]);
const TOKEN_COLOR_NAMES = [
  "accent",
  "background",
  "border",
  "card",
  "destructive",
  "foreground",
  "input",
  "muted",
  "muted-foreground",
  "popover",
  "primary",
  "primary-foreground",
  "secondary",
  "sidebar-accent",
  "sidebar-foreground",
  "success",
  "warning",
].join("|");

const TOKEN_OPACITY_UTILITY_RE = new RegExp(
  String.raw`(?:^|[^A-Za-z0-9_\[\]-])((?:[A-Za-z0-9_\[\]=:.-]+:)*(?:bg|border|text|ring|stroke|fill|divide|outline|from|via|to)-(?:${TOKEN_COLOR_NAMES}|\[var\([^\]]+\)\])/[0-9][A-Za-z0-9.]*)`,
  "g",
);

function isProductionSource(filePath: string) {
  const ext = path.extname(filePath);
  if (!SOURCE_EXTENSIONS.has(ext)) return false;
  if (filePath.includes(`${path.sep}__tests__${path.sep}`)) return false;
  if (/\.test\.[^.]+$/.test(filePath)) return false;
  return true;
}

function walkSourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.name === "dist") continue;
    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      files.push(...walkSourceFiles(fullPath));
      continue;
    }
    if (isProductionSource(fullPath)) {
      files.push(fullPath);
    }
  }
  return files;
}

function scanForbiddenColorSyntax() {
  const findings: string[] = [];
  for (const filePath of walkSourceFiles(SRC_ROOT)) {
    const rel = path.relative(process.cwd(), filePath);
    const lines = fs.readFileSync(filePath, "utf8").split(/\r?\n/);
    lines.forEach((line, index) => {
      if (line.includes("color-mix(")) {
        findings.push(`${rel}:${index + 1} color-mix()`);
      }

      TOKEN_OPACITY_UTILITY_RE.lastIndex = 0;
      for (let match = TOKEN_OPACITY_UTILITY_RE.exec(line); match; match = TOKEN_OPACITY_UTILITY_RE.exec(line)) {
        findings.push(`${rel}:${index + 1} ${match[1]}`);
      }
    });
  }
  return findings;
}

describe("WebView color compatibility", () => {
  it("keeps production source off CSS syntax that old Intel WebView renders incorrectly", () => {
    expect(scanForbiddenColorSyntax()).toEqual([]);
  });
});
