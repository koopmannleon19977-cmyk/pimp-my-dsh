import { spawnSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdtempSync, readFileSync, readdirSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));

export const ROOT = resolve(here, "..");
export const PACKAGE_JSON = join(ROOT, "package.json");
export const CORDIS_PATCH = join(ROOT, "cordis.patch.yml");
export const PROFILES_DIR = join(ROOT, "profiles");
export const SRC_DIR = join(ROOT, "src");
export const DIST_CLI = join(ROOT, "dist", "cli.js");
export const DIST_PLUGIN = join(ROOT, "dist", "plugin.js");


export function readPackageJson(): Record<string, unknown> {
  return JSON.parse(readFileSync(PACKAGE_JSON, "utf8")) as Record<string, unknown>;
}

export function readText(p: string): string {
  return readFileSync(p, "utf8");
}

export function makeTempDir(prefix = "pimp-dsh-test-"): string {
  return mkdtempSync(join(tmpdir(), prefix));
}

export function collectFiles(dir: string, out: string[] = []): string[] {
  if (!existsSync(dir)) return out;
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (entry.name === "node_modules" || entry.isSymbolicLink()) continue;
    const p = join(dir, entry.name);
    if (entry.isDirectory()) collectFiles(p, out);
    else out.push(p);
  }
  return out;
}

export function snapshotTree(dir: string): Map<string, string> {
  const map = new Map<string, string>();
  if (!existsSync(dir)) return map;
  for (const f of collectFiles(dir)) {
    const rel = relative(dir, f);
    map.set(rel, createHash("sha256").update(readFileSync(f)).digest("hex"));
  }
  return map;
}

export function treesEqual(a: Map<string, string>, b: Map<string, string>): boolean {
  if (a.size !== b.size) return false;
  for (const [k, v] of a) {
    if (b.get(k) !== v) return false;
  }
  return true;
}

export function runCli(args: string[], env: Record<string, string> = {}, cwd = ROOT) {
  return spawnSync(process.execPath, [DIST_CLI, ...args], {
    cwd,
    env: { ...process.env, NO_COLOR: "1", FORCE_COLOR: "0", ...env },
    encoding: "utf8",
    timeout: 30_000,
  });
}
