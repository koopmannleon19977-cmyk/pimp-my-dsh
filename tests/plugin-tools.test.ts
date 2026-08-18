import { spawnSync } from "node:child_process";
import type * as NodeChildProcess from "node:child_process";
import { copyFileSync, existsSync, linkSync, mkdirSync, symlinkSync, writeFileSync } from "node:fs";
import { delimiter, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { apply } from "../src/plugin";
import { makeTempDir } from "./helpers";

vi.mock("node:child_process", async (importOriginal) => {
  const actual = await importOriginal<typeof NodeChildProcess>();
  return { ...actual, spawnSync: vi.fn(actual.spawnSync) };
});

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

interface WebSearchResult {
  answer: string
  results: Array<{ title: string; url: string; content: string; score: number | null }>
}

interface GitHubWriteResult {
  operation: "pr" | "issue" | "comment"
  url?: string
  truncated: boolean
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
    expect(tools.map((tool) => tool.name)).toEqual(["pimp_git_read", "pimp_github_read", "pimp_memory", "pimp_github_write"]);
    expect(tools.map((tool) => tool.output?.schema.type)).toEqual(["object", "object", "object", "object"]);
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

describe("pimp_web_search", () => {
  let previousEnable: string | undefined;
  let previousKey: string | undefined;

  beforeEach(() => {
    previousEnable = process.env.DSH_PIMP_ENABLE_WEB_SEARCH;
    previousKey = process.env.DSH_PIMP_WEB_SEARCH_KEY;
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    if (previousEnable === undefined) delete process.env.DSH_PIMP_ENABLE_WEB_SEARCH;
    else process.env.DSH_PIMP_ENABLE_WEB_SEARCH = previousEnable;
    if (previousKey === undefined) delete process.env.DSH_PIMP_WEB_SEARCH_KEY;
    else process.env.DSH_PIMP_WEB_SEARCH_KEY = previousKey;
  });

  it("registers pimp_web_search only when DSH_PIMP_ENABLE_WEB_SEARCH=1", () => {
    expect(registerTools().map((tool) => tool.name)).not.toContain("pimp_web_search");
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    expect(registerTools().map((tool) => tool.name)).toContain("pimp_web_search");
  });

  it("normalizes a successful search response into answer and capped results", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    process.env.DSH_PIMP_WEB_SEARCH_KEY = "test-key";
    const fetchMock = vi.fn(async () => new Response(
      JSON.stringify({ answer: "A", results: [{ title: "T", url: "https://example.com", content: "C", score: 0.9 }] }),
      { status: 200, headers: { "content-type": "application/json" } },
    ));
    vi.stubGlobal("fetch", fetchMock);

    const result = await registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "hello" }, {}) as WebSearchResult;
    expect(result).toEqual({
      answer: "A",
      results: [{ title: "T", url: "https://example.com", content: "C", score: 0.9 }],
    });
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("rejects a query longer than 512 characters before fetching", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    process.env.DSH_PIMP_WEB_SEARCH_KEY = "test-key";
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "x".repeat(513) }, {})).rejects.toThrow(
      "query must be 1-512 characters",
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("throws on a redirect response without following it", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    process.env.DSH_PIMP_WEB_SEARCH_KEY = "test-key";
    const fetchMock = vi.fn(async () => new Response(
      null,
      { status: 302, headers: { location: "http://127.0.0.1:9/ssrf" } },
    ));
    vi.stubGlobal("fetch", fetchMock);

    await expect(registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "hello" }, {})).rejects.toThrow(
      "search provider error (HTTP 302)",
    );
    expect(fetchMock).toHaveBeenCalledTimes(1);
  });

  it("throws a clear error when the key is missing without fetching", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    const fetchMock = vi.fn();
    vi.stubGlobal("fetch", fetchMock);

    await expect(registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "hello" }, {})).rejects.toThrow(
      "web search is enabled but DSH_PIMP_WEB_SEARCH_KEY is not set (set PIMP_DSH_WEB_SEARCH_KEY)",
    );
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it("aborts when the streamed response body exceeds 1 MiB", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    process.env.DSH_PIMP_WEB_SEARCH_KEY = "test-key";
    const fetchMock = vi.fn(async () => new Response(
      new ReadableStream({
        start(controller) {
          for (let i = 0; i < 1100; i++) controller.enqueue(new Uint8Array(1024));
          controller.close();
        },
      }),
      { status: 200 },
    ));
    vi.stubGlobal("fetch", fetchMock);

    await expect(registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "hello" }, {})).rejects.toThrow(
      "search response exceeds",
    );
  });

  it("never leaks the API key in thrown errors", async () => {
    process.env.DSH_PIMP_ENABLE_WEB_SEARCH = "1";
    process.env.DSH_PIMP_WEB_SEARCH_KEY = "secret-key-123";
    const fetchMock = vi.fn(async () => new Response(
      JSON.stringify({ error: "invalid key secret-key-123" }),
      { status: 401 },
    ));
    vi.stubGlobal("fetch", fetchMock);

    let message = "";
    try {
      await registerTools().find((tool) => tool.name === "pimp_web_search")!.execute({ query: "hello" }, {});
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).toBe("search provider error (HTTP 401)");
    expect(message).not.toContain("secret-key-123");
  });
});

describe("pimp_github_write", () => {
  const spawnSyncMock = vi.mocked(spawnSync);
  const realSpawnSync = spawnSyncMock.getMockImplementation()!;

  afterEach(() => {
    spawnSyncMock.mockReset();
    spawnSyncMock.mockImplementation(realSpawnSync);
  });

  it("asks for approval before any GitHub write while passive tools pass through", async () => {
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
    await expect(gate!({ name: "pimp_github_write" }, next)).resolves.toMatchObject({
      kind: "ask",
      reason: expect.stringContaining("writes to GitHub"),
    });
    expect(next).toHaveBeenCalled();
    await expect(gate!({ name: "pimp_git_read" }, next)).resolves.toEqual({ kind: "allow" });
    await expect(gate!({ name: "pimp_github_read" }, next)).resolves.toEqual({ kind: "allow" });
    await expect(gate!({ name: "pimp_memory" }, next)).resolves.toEqual({ kind: "allow" });
  });

  it("rejects invalid repository, oversized title, and unsafe branch names before spawning", async () => {
    const write = registerTools().find((tool) => tool.name === "pimp_github_write");
    expect(write).toBeDefined();

    await expect(
      write!.execute({ operation: "pr", repository: "../private", title: "T", body: "B" }, {}),
    ).rejects.toThrow("exact owner/name");

    await expect(
      write!.execute({ operation: "issue", repository: "owner/name", title: "x".repeat(257), body: "B" }, {}),
    ).rejects.toThrow("title must be");

    for (const head of ["feat..x", "/lead", "-bad"]) {
      await expect(
        write!.execute({ operation: "pr", repository: "owner/name", title: "T", body: "B", head }, {}),
      ).rejects.toThrow("branch must");
    }

    expect(spawnSyncMock).not.toHaveBeenCalled();
  });

  it("builds fixed gh argv for pr, issue, and comment without a shell", async () => {
    const write = registerTools().find((tool) => tool.name === "pimp_github_write");
    expect(write).toBeDefined();

    spawnSyncMock.mockImplementation(((
      _command: string,
      args?: readonly string[],
    ) => {
      if (Array.isArray(args) && args.includes("--show-current")) {
        return { status: 0, stdout: "feature-x\n", stderr: "", signal: null, error: undefined, pid: 1, output: ["feature-x\n", ""] };
      }
      return { status: 0, stdout: "https://github.com/owner/name/pull/1\n", stderr: "", signal: null, error: undefined, pid: 1, output: ["https://github.com/owner/name/pull/1\n", ""] };
    }) as never);

    const ghArgv = (first: string, second: string) =>
      spawnSyncMock.mock.calls
        .map((call) => call[1])
        .find((args) => Array.isArray(args) && args[0] === first && args[1] === second);

    const pr = await write!.execute(
      { operation: "pr", repository: "owner/name", title: "T", body: "B" },
      {},
    ) as GitHubWriteResult;
    expect(pr).toEqual({ operation: "pr", url: "https://github.com/owner/name/pull/1", truncated: false });
    expect(ghArgv("pr", "create")).toEqual(
      ["pr", "create", "--repo", "owner/name", "--head", "feature-x", "--title", "T", "--body", "B"],
    );

    spawnSyncMock.mockClear();

    await write!.execute({ operation: "pr", repository: "owner/name", title: "T", body: "B", base: "main" }, {});
    expect(ghArgv("pr", "create")).toEqual(
      ["pr", "create", "--repo", "owner/name", "--head", "feature-x", "--base", "main", "--title", "T", "--body", "B"],
    );

    spawnSyncMock.mockClear();

    await write!.execute({ operation: "issue", repository: "owner/name", title: "T", body: "B" }, {});
    expect(ghArgv("issue", "create")).toEqual(
      ["issue", "create", "--repo", "owner/name", "--title", "T", "--body", "B"],
    );

    spawnSyncMock.mockClear();

    await write!.execute({ operation: "comment", repository: "owner/name", number: 42, body: "B" }, {});
    expect(ghArgv("issue", "comment")).toEqual(
      ["issue", "comment", "42", "--repo", "owner/name", "--body", "B"],
    );

    spawnSyncMock.mockClear();

    await write!.execute({ operation: "comment", repository: "owner/name", number: 42, body: "B", kind: "pr" }, {});
    expect(ghArgv("pr", "comment")).toEqual(
      ["pr", "comment", "42", "--repo", "owner/name", "--body", "B"],
    );
  });

  it("bounds a non-zero gh exit without leaking the full stderr blob", async () => {
    const write = registerTools().find((tool) => tool.name === "pimp_github_write");
    expect(write).toBeDefined();

    const tail = "UNIQUE_CREDENTIAL_TAIL";
    spawnSyncMock.mockImplementation(((
      _command: string,
      _args?: readonly string[],
    ) => ({ status: 1, stdout: "", stderr: "x".repeat(100_000) + tail, signal: null, error: undefined, pid: 1, output: ["", "x".repeat(100_000) + tail] })) as never);

    let message = "";
    try {
      await write!.execute({ operation: "issue", repository: "owner/name", title: "T", body: "B" }, {});
    } catch (error) {
      message = (error as Error).message;
    }
    expect(message).not.toContain(tail);
    expect(message.length).toBeLessThanOrEqual(64_000);
  });
});
