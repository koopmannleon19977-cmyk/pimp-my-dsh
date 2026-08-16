import { describe, expect, it } from "vitest";
import { CORDIS_PATCH, collectFiles, PROFILES_DIR, readText, SRC_DIR } from "./helpers";

function readSourceText(): string {
  const files = [CORDIS_PATCH, ...collectFiles(SRC_DIR), ...collectFiles(PROFILES_DIR)];
  return files.map((f) => readText(f)).join("\n");
}

describe("OpenAI-compatible env indirection", () => {
  const src = readSourceText();

  it("reads API key, base URL, and model from the environment", () => {
    expect(src).toMatch(/PIMP_DSH_API_KEY/);
    expect(src).toMatch(/PIMP_DSH_BASE_URL/);
    expect(src).toMatch(/PIMP_DSH_MODEL/);
  });

  it("reads credentials via process.env, never hardcoded", () => {
    // The distribution reads the environment; the exact access style
    // (dot, bracket, or destructuring) is an implementation detail.
    expect(src).toMatch(/process\.env/);
    expect(src).toMatch(/PIMP_DSH_API_KEY/);
    expect(src).toMatch(/PIMP_DSH_BASE_URL/);
    expect(src).toMatch(/PIMP_DSH_MODEL/);
  });

  it("gates LSP behind PIMP_DSH_ENABLE_LSP", () => {
    expect(src).toMatch(/PIMP_DSH_ENABLE_LSP/);
  });

  it("ignores DSH_TELEMETRY_MODE (telemetry disabled unconditionally)", () => {
    // The distribution must never read the upstream telemetry switch.
    expect(src).not.toMatch(/process\.env\.DSH_TELEMETRY_MODE/);
  });

  it("promotes public settings into DSH-protected variables before launch", () => {
    expect(src).toMatch(/PIMP_DSH_API_KEY['"], ['"]DSH_PIMP_API_KEY/);
    expect(src).toMatch(/PIMP_DSH_BASE_URL['"], ['"]DSH_PIMP_BASE_URL/);
    expect(src).toMatch(/PIMP_DSH_MODEL['"], ['"]DSH_PIMP_MODEL/);
    expect(src).toMatch(/PIMP_DSH_ENABLE_LSP['"], ['"]DSH_PIMP_ENABLE_LSP/);
  });

  it("forces the upstream hard telemetry kill switch", () => {
    expect(src).toMatch(/DSH_TELEMETRY_DISABLED\s*=\s*['"]1['"]/);
  });

  it("keeps project-loadable public variables out of bundle authority checks", () => {
    const patch = readText(CORDIS_PATCH);
    expect(patch).not.toMatch(/process\.env\.PIMP_DSH_/);
    expect(patch).toMatch(/process\.env\.DSH_PIMP_/);
    expect(patch).toMatch(/apiKeyEnv:\s*DSH_PIMP_API_KEY/);
  });

  it("disables package lifecycle scripts during profile installation", () => {
    expect(src).toMatch(/npm_config_ignore_scripts\s*[:=]\s*['"]true['"]/);
  });

  it("uses the bundled pnpm without hooks, scripts, or ambient secret variables", () => {
    const cliPath = collectFiles(SRC_DIR).find((file) => file.endsWith("cli.ts"));
    expect(cliPath).toBeDefined();
    const cli = readText(cliPath!);
    expect(cli).toMatch(/require\.resolve\(['"]pnpm['"]\)/);
    expect(cli).toMatch(/--ignore-pnpmfile/);
    const environmentBuilder = /function packageManagerEnvironment[\s\S]+?function harnessEnvironment/.exec(cli)?.[0];
    expect(environmentBuilder).toBeDefined();
    expect(environmentBuilder).not.toMatch(/\.\.\.process\.env/);
    expect(environmentBuilder).not.toMatch(/PIMP_DSH_API_KEY/);
  });
});

describe("no secret literals", () => {
  const src = readSourceText();

  it("contains no well-known secret key prefixes", () => {
    const patterns = [
      /sk-[A-Za-z0-9_-]{16,}/,
      /ghp_[A-Za-z0-9]{20,}/,
      /github_pat_[A-Za-z0-9_]{20,}/,
      /xox[baprs]-[A-Za-z0-9-]{10,}/,
      /AKIA[0-9A-Z]{16}/,
      /-----BEGIN [A-Z ]*PRIVATE KEY-----/,
    ];
    for (const re of patterns) {
      expect(src, `secret pattern ${re} found in source`).not.toMatch(re);
    }
  });

  it("never assigns a literal value to the credential env vars", () => {
    expect(src).not.toMatch(/PIMP_DSH_API_KEY\s*[:=]\s*["'][^"']+["']/);
    expect(src).not.toMatch(/PIMP_DSH_BASE_URL\s*[:=]\s*["'][^"']+["']/);
  });
});
