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

describe("cordis.patch.yml contract", () => {
  const text = existsSync(CORDIS_PATCH) ? readText(CORDIS_PATCH) : "";

  it("is a non-empty DSH patch list", () => {
    expect(text.trim().length).toBeGreaterThan(0);
    expect(text).toMatch(/^\s*-\s+(id|insert):/m);
    expect(text).toMatch(/^\s*-\s+id:\s*\S/m);
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
