import { createRequire } from "node:module";
import { dirname, join } from "node:path";
import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { CORDIS_PATCH, readText } from "./helpers";

/**
 * Split a cordis patch list into its top-level rows. A row begins with a
 * column-0 `- `; nested rows inside a group are indented and stay attached to
 * their parent row.
 */
function topLevelRows(text: string): string[] {
  return text.split(/\n(?=- )/);
}

function findRow(text: string, idPattern: RegExp): string | null {
  return topLevelRows(text).find((r) => idPattern.test(r)) ?? null;
}

const require = createRequire(import.meta.url);
const dshRequire = createRequire(require.resolve("@deepseek-ai/dsh/package.json"));
const basePatch = join(dirname(dshRequire.resolve("@deepseek-ai/dsh-base/package.json")), "cordis.patch.yml");

describe("cordis.patch.yml contract", () => {
  const text = existsSync(CORDIS_PATCH) ? readText(CORDIS_PATCH) : "";

  it("is a non-empty DSH patch list", () => {
    expect(text.trim().length).toBeGreaterThan(0);
    expect(text).toMatch(/^\s*-\s+(id|insert):/m);
    expect(text).toMatch(/^\s*-\s+id:\s*\S/m);
  });

  it("targets rows that still exist in the pinned upstream base bundle", () => {
    const upstreamIds = new Set(
      [...readText(basePatch).matchAll(/^\s+- id:\s*([^\s]+)/gm)]
        .map((match) => match[1])
        .filter((id): id is string => id !== undefined),
    );
    const targetIds = topLevelRows(text)
      .map((row) => row.match(/^- id:\s*([^\s]+)/)?.[1])
      .filter((id): id is string => id !== undefined);
    expect(targetIds.length).toBeGreaterThan(0);
    for (const id of targetIds) {
      expect(upstreamIds, `upstream removed or renamed patch target ${id}`).toContain(id);
    }
  });

  it("disables telemetry unconditionally", () => {
    expect(text).toMatch(/-\s+id:\s*session-telemetry-otel[\s\S]*?disabled\s*:\s*true/);
  });

  it("disables web fetch and search", () => {
    const row = findRow(text, /-\s+id:\s*tool-web\b/i);
    expect(row, "patch must address web fetch/search").toBeTruthy();
    expect(row).toMatch(/fetch\s*:\s*false/i);
    expect(row).toMatch(/search\s*:\s*false/i);
  });

  it("keeps the sandbox default at workspace-write with partial enforcement", () => {
    expect(text).toMatch(/workspace-write/);
    expect(text).toMatch(/partial/);
  });

  it("requires approval for danger-full-access", () => {
    expect(text).toMatch(/danger-full-access:[\s\S]*?sandbox\s*:\s*danger-full-access[\s\S]*?approval\s*:\s*ask/);
    expect(text).toMatch(/defaultPreset\s*:\s*workspace-write/);
  });

  it("pins native parallel subagents to a bounded fresh-child provider", () => {
    const loop = findRow(text, /-\s+id:\s*agent-loop\b/i);
    const provider = findRow(text, /-\s+id:\s*subagent-spawn-in-process\b/i);
    const tool = findRow(text, /-\s+id:\s*tool-subagent\b/i);
    expect(loop).toMatch(/agents\s*:\s*\[\][\s\S]*?maxParallelToolCalls\s*:\s*4/);
    expect(provider).toMatch(/providerName\s*:\s*spawn/);
    expect(tool).toMatch(/provider\s*:\s*spawn[\s\S]*?backgroundMode\s*:\s*continuable[\s\S]*?maxDepth\s*:\s*3/);
  });

  it("exposes approval-gated one-shot worktree delegation", () => {
    const row = findRow(text, /-\s+insert:/i);
    expect(row).toMatch(/id:\s*tool-subagent-worktree[\s\S]*?provider\s*:\s*worktree[\s\S]*?toolName\s*:\s*subagent_worktree[\s\S]*?backgroundMode\s*:\s*one-shot/);
  });

  it("keeps LSP opt-in (never force-enabled)", () => {
    expect(text).not.toMatch(/lsp\s*:\s*true|enableLsp\s*:\s*true/i);
  });

  it("inserts the distribution-owned plugin", () => {
    expect(text).toMatch(/pimp-my-dsh/i);
  });

  it("selects pwsh over bash on Windows", () => {
    expect(text).toMatch(/-\s+id:\s*(tool-bash|bash-sandbox)[\s\S]*?disabled:\s*!!js process\.platform === 'win32'/);
    expect(text).toMatch(/-\s+id:\s*(tool-pwsh|pwsh-sandbox)[\s\S]*?disabled:\s*!!js process\.platform !== 'win32'/);
  });
});
