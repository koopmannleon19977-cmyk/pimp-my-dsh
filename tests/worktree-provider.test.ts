import { spawnSync } from "node:child_process";
import { existsSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { makeTempDir } from "./helpers";

const driver = vi.hoisted(() => ({ startInProcessRun: vi.fn() }));
vi.mock("@deepseek-ai/dsh-subagent-in-process-driver", () => driver);

import { registerWorktreeSubagent } from "../src/worktree-subagent";

function git(cwd: string, args: string[]): string {
  const result = spawnSync("git", args, { cwd, encoding: "utf8" });
  expect(result.status, result.stderr).toBe(0);
  return result.stdout.trim();
}

describe("worktree subagent provider", () => {
  let previousHome: string | undefined;

  beforeEach(() => {
    previousHome = process.env.DSH_HOME;
    process.env.DSH_HOME = makeTempDir("worktree-provider-home-");
    driver.startInProcessRun.mockReset();
  });

  afterEach(() => {
    if (previousHome === undefined) delete process.env.DSH_HOME;
    else process.env.DSH_HOME = previousHome;
  });

  it("routes the child to its worktree and preserves review metadata on infrastructure failure", async () => {
    const repository = makeTempDir("worktree-provider-repo-");
    git(repository, ["init", "-b", "main"]);
    writeFileSync(join(repository, "tracked.txt"), "committed\n");
    git(repository, ["add", "tracked.txt"]);
    git(repository, [
      "-c", "user.name=Smoke",
      "-c", "user.email=smoke@example.invalid",
      "commit", "-m", "base",
    ]);

    let rejectResult!: (error: unknown) => void;
    const childResult = new Promise<never>((_resolve, reject) => {
      rejectResult = reject;
    });
    const disposalFailure = new Error("child disposal failed");
    driver.startInProcessRun.mockResolvedValueOnce({
      id: "child",
      localAgent: undefined,
      result: childResult,
      dispose: vi.fn(async () => { throw disposalFailure; }),
    });

    let provider: {
      start(request: unknown): Promise<{ result: Promise<unknown>; dispose(): Promise<void> }>;
    } | undefined;
    registerWorktreeSubagent({
      subagents: {
        registerProvider(value: typeof provider) {
          provider = value;
        },
      },
    } as never);

    const run = await provider!.start({
      parent: { session: { header: { cwd: repository } } },
      prompt: [],
      descriptor: {},
      signal: new AbortController().signal,
    });
    const delegated = driver.startInProcessRun.mock.calls[0]![0] as {
      parent: { session: { header: { cwd: string } } };
    };
    const worktreePath = delegated.parent.session.header.cwd;
    const branch = git(worktreePath, ["branch", "--show-current"]);

    try {
      expect(worktreePath).not.toBe(repository);
      expect(existsSync(join(worktreePath, "tracked.txt"))).toBe(true);

      const infrastructureFailure = new Error("child transport failed");
      rejectResult(infrastructureFailure);
      const failure = await run.result.catch((error: unknown) => error) as Error;
      expect(failure.message).toContain("child transport failed");
      expect(failure.message).toContain(worktreePath);
      expect(failure.message).toContain(branch);
      expect(failure.cause).toBe(infrastructureFailure);
      const disposal = await run.dispose().catch((error: unknown) => error) as Error;
      expect(disposal.message).toContain("child disposal failed");
      expect(disposal.message).toContain(worktreePath);
      expect(disposal.message).toContain(branch);
      expect(disposal.cause).toBe(disposalFailure);
    } finally {
      git(repository, ["worktree", "remove", "--force", worktreePath]);
      git(repository, ["branch", "-D", branch]);
    }
  });
});
