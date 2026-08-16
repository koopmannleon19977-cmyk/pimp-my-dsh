import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, linkSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { apply } from "../src/plugin";
import { makeTempDir } from "./helpers";

type Tool = {
  name: string;
  execute: (args: Record<string, unknown>, execution: Record<string, never>) => Promise<string>;
};

function registerTools(): Tool[] {
  const tools: Tool[] = [];
  apply({
    systemPrompt: { section: vi.fn(), context: vi.fn() },
    tools: { register: (tool: Tool) => tools.push(tool) },
  } as never);
  return tools;
}

describe("distribution-owned tools", () => {
  let previousHome: string | undefined;

  beforeEach(() => {
    previousHome = process.env.DSH_HOME;
    process.env.DSH_HOME = makeTempDir();
  });

  afterEach(() => {
    if (previousHome === undefined) delete process.env.DSH_HOME;
    else process.env.DSH_HOME = previousHome;
  });

  it("registers scoped Git and durable memory tools", () => {
    expect(registerTools().map((tool) => tool.name)).toEqual(["pimp_git_read", "pimp_memory"]);
  });

  it.runIf(process.platform === "win32")(
    "does not execute a repository-local git.exe",
    async () => {
      const repo = makeTempDir("hostile-git-");
      const initialized = spawnSync("git", ["init", "-b", "main"], { cwd: repo, encoding: "utf8" });
      expect(initialized.status, initialized.stderr).toBe(0);
      copyFileSync(
        join(process.env.SystemRoot ?? "C:\\Windows", "System32", "where.exe"),
        join(repo, "git.exe"),
      );
      const git = registerTools().find((tool) => tool.name === "pimp_git_read");
      const previousDirectory = process.cwd();
      const previousPath = process.env.PATH;
      process.chdir(repo);
      process.env.PATH = `${repo}${delimiter}${previousPath ?? ""}`;
      try {
        await expect(git!.execute({ operation: "status" }, {})).resolves.toContain("No commits yet on main");
      } finally {
        process.chdir(previousDirectory);
        if (previousPath === undefined) delete process.env.PATH;
        else process.env.PATH = previousPath;
      }
    },
  );

  it("reads Git without mutating the repository", async () => {
    const git = registerTools().find((tool) => tool.name === "pimp_git_read");
    expect(git).toBeDefined();
    const log = await git!.execute({ operation: "log", limit: 1 }, {});
    expect(log).toMatch(/^[0-9a-f]{7,}\s+/);
  });

  it("renders a working-tree diff without repository filter execution", async () => {
    const repo = makeTempDir("filtered-git-");
    expect(spawnSync("git", ["init", "-b", "main"], { cwd: repo }).status).toBe(0);
    writeFileSync(join(repo, ".gitattributes"), "sample.txt filter=hostile\n");
    writeFileSync(join(repo, "sample.txt"), "before\n");
    expect(spawnSync("git", ["add", "."], { cwd: repo }).status).toBe(0);
    expect(spawnSync("git", [
      "-c",
      "user.name=Smoke",
      "-c",
      "user.email=smoke@example.invalid",
      "commit",
      "-m",
      "base",
    ], { cwd: repo }).status).toBe(0);
    expect(spawnSync("git", ["update-index", "--index-version=2"], { cwd: repo }).status).toBe(0);
    expect(spawnSync("git", ["config", "extensions.worktreeConfig", "true"], { cwd: repo }).status).toBe(0);
    expect(spawnSync("git", [
      "config",
      "--worktree",
      "filter.hostile.clean",
      "definitely-not-a-command",
    ], { cwd: repo }).status).toBe(0);
    writeFileSync(join(repo, "sample.txt"), "after\n");

    const gitTool = registerTools().find((tool) => tool.name === "pimp_git_read");
    const previousDirectory = process.cwd();
    process.chdir(repo);
    try {
      const diff = await gitTool!.execute({ operation: "diff" }, {});
      expect(diff).toContain("-before");
      expect(diff).toContain("+after");
    } finally {
      process.chdir(previousDirectory);
    }
  });

  it("rejects Git inspection from a nested working directory", async () => {
    const git = registerTools().find((tool) => tool.name === "pimp_git_read");
    const previousDirectory = process.cwd();
    process.chdir(join(previousDirectory, "src"));
    try {
      await expect(git!.execute({ operation: "status" }, {})).rejects.toThrow(
        "current working directory must be the Git repository root",
      );
    } finally {
      process.chdir(previousDirectory);
    }
  });

  it("appends and recalls durable notes", async () => {
    const memory = registerTools().find((tool) => tool.name === "pimp_memory");
    expect(memory).toBeDefined();
    const stored = JSON.parse(
      await memory!.execute({ operation: "remember", text: "Use exact upstream pins" }, {}),
    );
    expect(stored.text).toBe("Use exact upstream pins");

    const recalled = JSON.parse(
      await memory!.execute({ operation: "recall", query: "upstream" }, {}),
    );
    expect(recalled).toHaveLength(1);
    expect(recalled[0]).toMatchObject({ id: stored.id, text: stored.text });
    await expect(
      memory!.execute({ operation: "recall", query: "x".repeat(4_097) }, {}),
    ).rejects.toThrow("memory query exceeds 4096 characters");
  });

  it("rejects a multiply-linked memory log", async () => {
    const memory = registerTools().find((tool) => tool.name === "pimp_memory");
    await memory!.execute({ operation: "remember", text: "private record" }, {});
    const log = join(process.env.DSH_HOME!, "pimp-my-dsh", "memory.jsonl");
    const alias = join(process.env.DSH_HOME!, "memory-alias.jsonl");
    linkSync(log, alias);
    expect(existsSync(alias)).toBe(true);
    await expect(memory!.execute({ operation: "recall" }, {})).rejects.toThrow(
      "memory log must be one regular, non-linked file",
    );
  });
});
