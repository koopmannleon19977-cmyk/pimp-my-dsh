import { spawnSync } from "node:child_process";
import { copyFileSync, existsSync, linkSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { apply } from "../src/plugin";
import { makeTempDir } from "./helpers";

interface Tool {
  name: string
  output?: { schema: { type?: string } }
  execute: (args: Record<string, unknown>, execution: Record<string, never>) => Promise<unknown>
}

interface GitResult {
  operation: "status" | "diff" | "log"
  output: string
  truncated: boolean
}

interface MemoryResult {
  operation: "remember" | "recall"
  records: Array<{ id: string; text: string; createdAt: string }>
}

function registerTools(): Tool[] {
  const tools: Tool[] = [];
  apply({
    systemPrompt: { section: vi.fn(), context: vi.fn() },
    tools: { register: (tool: Tool) => tools.push(tool) },
    subagents: { registerProvider: vi.fn() },
    on: vi.fn(),
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

  it("registers scoped Git, GitHub, and durable memory tools with structured results", () => {
    const tools = registerTools();
    expect(tools.map((tool) => tool.name)).toEqual(["pimp_git_read", "pimp_github_read", "pimp_memory"]);
    expect(tools.map((tool) => tool.output?.schema.type)).toEqual(["object", "object", "object"]);
  });

  it("applies fail-closed risk tiers to browser and worktree operations", async () => {
    let gate: ((exec: { name: string }, next: () => Promise<{ kind: "allow" } | { kind: "deny"; reason: string }>) => Promise<unknown>) | undefined;
    apply({
      systemPrompt: { section: vi.fn(), context: vi.fn() },
      tools: { register: vi.fn() },
      subagents: { registerProvider: vi.fn() },
      on: (event: string, listener: typeof gate) => {
        if (event === "tools/pre-execute") gate = listener;
        return vi.fn();
      },
    } as never);

    expect(gate).toBeDefined();
    const next = vi.fn(async () => ({ kind: "allow" as const }));
    await expect(gate!({ name: "mcp__browser__browser_snapshot" }, next)).resolves.toEqual({ kind: "allow" });
    await expect(gate!({ name: "mcp__browser__browser_find" }, next)).resolves.toEqual({ kind: "allow" });
    await expect(gate!({ name: "mcp__browser__browser_network_request" }, next)).resolves.toMatchObject({ kind: "ask" });
    await expect(gate!({ name: "mcp__browser__browser_navigate" }, next)).resolves.toMatchObject({ kind: "ask" });
    await expect(gate!({ name: "mcp__browser__browser_click" }, next)).resolves.toMatchObject({ kind: "ask" });
    await expect(gate!({ name: "mcp__browser__browser_future_operation" }, next)).resolves.toMatchObject({ kind: "ask" });
    await expect(gate!({ name: "mcp__browser__browser_run_code_unsafe" }, next)).resolves.toMatchObject({ kind: "deny" });
    await expect(gate!({ name: "subagent_worktree" }, next)).resolves.toMatchObject({ kind: "ask" });
    const deny = { kind: "deny" as const, reason: "blocked by owner policy" };
    await expect(gate!({ name: "subagent_worktree" }, async () => deny)).resolves.toEqual(deny);
    await expect(gate!({ name: "mcp__other__browser_click" }, next)).resolves.toEqual({ kind: "allow" });
  });

  it("rejects unscoped GitHub repositories before invoking the CLI", async () => {
    const github = registerTools().find((tool) => tool.name === "pimp_github_read");
    await expect(
      github!.execute({ operation: "repo", repository: "../private" }, {}),
    ).rejects.toThrow("exact owner/name");
  });

  it.runIf(process.platform === "win32")(
    "does not execute a repository-local git.exe",
    async () => {
      const repo = makeTempDir("hostile-git-");
      const initialized = spawnSync("git", ["init", "-b", "main"], { cwd: repo, encoding: "utf8" });
      expect(initialized.status, initialized.stderr).toBe(0);
      const hostileDirectory = join(repo, "..git-bin");
      mkdirSync(hostileDirectory);
      copyFileSync(
        join(process.env.SystemRoot ?? "C:\\Windows", "System32", "where.exe"),
        join(hostileDirectory, "git.exe"),
      );
      const git = registerTools().find((tool) => tool.name === "pimp_git_read");
      const previousDirectory = process.cwd();
      const previousPath = process.env.PATH;
      process.chdir(repo);
      process.env.PATH = `${hostileDirectory}${delimiter}${previousPath ?? ""}`;
      try {
        const result = await git!.execute({ operation: "status" }, {}) as GitResult;
        expect(result).toMatchObject({ operation: "status", truncated: false });
        expect(result.output).toContain("No commits yet on main");
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
    const log = await git!.execute({ operation: "log", limit: 1 }, {}) as GitResult;
    expect(log).toMatchObject({ operation: "log", truncated: false });
    expect(log.output).toMatch(/^[0-9a-f]{7,}\s+/);
  });

  it("reads Git from the calling agent workspace rather than the harness process", async () => {
    const repo = makeTempDir("agent-cwd-git-");
    expect(spawnSync("git", ["init", "-b", "isolated"], { cwd: repo }).status).toBe(0);
    writeFileSync(join(repo, "child-only.txt"), "before\n");
    expect(spawnSync("git", ["add", "child-only.txt"], { cwd: repo }).status).toBe(0);
    expect(spawnSync("git", [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ], { cwd: repo }).status).toBe(0);
    writeFileSync(join(repo, "child-only.txt"), "after\n");
    const git = registerTools().find((tool) => tool.name === "pimp_git_read");

    const result = await git!.execute(
      { operation: "status" },
      { agent: { session: { header: { cwd: repo } } } } as never,
    ) as GitResult;

    expect(result.output).toContain("isolated");
    expect(result.output).toContain("child-only.txt");
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
      const diff = await gitTool!.execute({ operation: "diff" }, {}) as GitResult;
      expect(diff.operation).toBe("diff");
      expect(diff.output).toContain("-before");
      expect(diff.output).toContain("+after");
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
    const remembered = await memory!.execute(
      { operation: "remember", text: "Use exact upstream pins" },
      {},
    ) as MemoryResult;
    expect(remembered.operation).toBe("remember");
    expect(remembered.records).toHaveLength(1);
    const stored = remembered.records[0]!;
    expect(stored.text).toBe("Use exact upstream pins");

    const recalled = await memory!.execute(
      { operation: "recall", query: "upstream" },
      {},
    ) as MemoryResult;
    expect(recalled.operation).toBe("recall");
    expect(recalled.records).toHaveLength(1);
    expect(recalled.records[0]).toMatchObject({ id: stored.id, text: stored.text });
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
  it("rejects a dangling memory-log symlink without creating its target", async () => {
    const memory = registerTools().find((tool) => tool.name === "pimp_memory");
    const directory = join(process.env.DSH_HOME!, "pimp-my-dsh");
    const outside = join(makeTempDir(), "outside-memory.jsonl");
    mkdirSync(directory);
    symlinkSync(outside, join(directory, "memory.jsonl"), "file");

    await expect(memory!.execute({ operation: "remember", text: "must stay private" }, {})).rejects.toThrow(
      "memory log must be one regular, non-linked file",
    );
    expect(existsSync(outside)).toBe(false);
  });

});
