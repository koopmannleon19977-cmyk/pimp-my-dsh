import { spawnSync } from "node:child_process";
import { chmodSync, existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { createWorktree } from "../src/worktree-subagent";
import { makeTempDir } from "./helpers";

function git(cwd: string, args: string[]): string {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  expect(result.status, result.stderr).toBe(0);
  return result.stdout.trim();
}

describe("worktree subagent workspace", () => {
  let previousHome: string | undefined;

  beforeEach(() => {
    previousHome = process.env.DSH_HOME;
    process.env.DSH_HOME = makeTempDir("worktree-home-");
  });

  afterEach(() => {
    if (previousHome === undefined) delete process.env.DSH_HOME;
    else process.env.DSH_HOME = previousHome;
  });

  it("creates a branch whose index matches HEAD and copies only tracked workspace state", () => {
    const repository = makeTempDir("worktree-repo-");
    git(repository, ["init", "-b", "main"]);
    writeFileSync(join(repository, "tracked.txt"), "committed\n");
    git(repository, ["add", "tracked.txt"]);
    git(repository, [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ]);
    writeFileSync(join(repository, "tracked.txt"), "working copy\n");
    writeFileSync(join(repository, "untracked.txt"), "must not copy\n");

    const worktree = createWorktree(repository);
    try {
      expect(readFileSync(join(worktree.path, "tracked.txt"), "utf8")).toBe("working copy\n");
      expect(existsSync(join(worktree.path, "untracked.txt"))).toBe(false);
      expect(git(worktree.path, ["status", "--porcelain=v2"])).toMatch(/^1 \.M /);
      expect(git(worktree.path, ["branch", "--show-current"])).toBe(worktree.branch);
    } finally {
      git(repository, ["worktree", "remove", "--force", worktree.path]);
      git(repository, ["branch", "-D", worktree.branch]);
    }
  });

  it("rejects linked private storage before creating worktree contents", () => {
    const repository = makeTempDir("worktree-repo-");
    const outside = makeTempDir("worktree-outside-");
    git(repository, ["init", "-b", "main"]);
    writeFileSync(join(repository, "tracked.txt"), "committed\n");
    git(repository, ["add", "tracked.txt"]);
    git(repository, [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ]);
    symlinkSync(outside, join(process.env.DSH_HOME!, "pimp-my-dsh"), "junction");

    expect(() => createWorktree(repository)).toThrow(/private, non-linked directories/);
    expect(existsSync(join(outside, "worktrees"))).toBe(false);
  });

  it("rejects tracked paths reached through a linked workspace parent", () => {
    const repository = makeTempDir("worktree-repo-");
    const outside = makeTempDir("worktree-source-outside-");
    git(repository, ["init", "-b", "main"]);
    mkdirSync(join(repository, "nested"));
    writeFileSync(join(repository, "nested", "tracked.txt"), "committed\n");
    git(repository, ["add", "nested/tracked.txt"]);
    git(repository, [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ]);
    const hooks = join(repository, ".malicious-hooks");
    const hookSentinel = join(makeTempDir("worktree-hook-"), "invoked");
    const hookScript = `#!/bin/sh\nprintf invoked >> '${hookSentinel.replaceAll("\\", "/")}'\n`;
    mkdirSync(hooks);
    for (const name of ["post-index-change", "reference-transaction"]) {
      const hook = join(hooks, name);
      writeFileSync(hook, hookScript);
      chmodSync(hook, 0o755);
    }
    git(repository, ["config", "core.hooksPath", ".malicious-hooks"]);
    rmSync(join(repository, "nested"), { recursive: true });
    writeFileSync(join(outside, "tracked.txt"), "outside\n");
    symlinkSync(outside, join(repository, "nested"), "junction");

    expect(() => createWorktree(repository)).toThrow(/linked or non-directory workspace parent/);
    expect(git(repository, ["branch", "--list", "pimp-agent/*"])).toBe("");
    expect(existsSync(hookSentinel)).toBe(false);
  });

  it("rejects skip-worktree index entries instead of converting them to deletions", () => {
    const repository = makeTempDir("worktree-sparse-repo-");
    git(repository, ["init", "-b", "main"]);
    writeFileSync(join(repository, "tracked.txt"), "committed\n");
    git(repository, ["add", "tracked.txt"]);
    git(repository, [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ]);
    git(repository, ["update-index", "--skip-worktree", "tracked.txt"]);
    rmSync(join(repository, "tracked.txt"));

    expect(() => createWorktree(repository)).toThrow(/sparse or skip-worktree indexes/);
    expect(git(repository, ["branch", "--list", "pimp-agent/*"])).toBe("");
  });
});
