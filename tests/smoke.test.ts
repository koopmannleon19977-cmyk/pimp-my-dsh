import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { makeTempDir, ROOT } from "./helpers";

const FIXTURE = join(ROOT, "tests", "fixtures", "buggy-repo");

function copyTree(src: string, dest: string): void {
  mkdirSync(dest, { recursive: true });
  for (const entry of readdirSync(src, { withFileTypes: true })) {
    const s = join(src, entry.name);
    const d = join(dest, entry.name);
    if (entry.isDirectory()) copyTree(s, d);
    else copyFileSync(s, d);
  }
}

describe("distribution smoke: filesystem/search/edit/shell contract", () => {
  it("finds and fixes the seeded bug without a model", () => {
    const repo = makeTempDir("buggy-repo-");
    copyTree(FIXTURE, repo);

    const calcPath = join(repo, "src", "calc.js");

    // 1. filesystem read contract
    const original = readFileSync(calcPath, "utf8");
    expect(original).toContain("return a - b");

    // 2. search contract: locate the buggy line
    const buggyLine = original.split("\n").find((l: string) => l.includes("a - b"));
    expect(buggyLine).toBeTruthy();

    // 3. edit contract: fix the bug via string replacement
    const fixed = original.replace("return a - b", "return a + b");
    expect(fixed).not.toBe(original);
    writeFileSync(calcPath, fixed);

    // 4. shell contract: run the verification script
    const check = spawnSync(process.execPath, [join(repo, "check.js")], { encoding: "utf8" });
    expect(check.status, check.stderr).toBe(0);
    expect(check.stdout).toContain("OK");
  });

  it("the fixture is deterministic (bug is present before the fix)", () => {
    const calc = readFileSync(join(FIXTURE, "src", "calc.js"), "utf8");
    expect(calc).toContain("return a - b");
  });
});
