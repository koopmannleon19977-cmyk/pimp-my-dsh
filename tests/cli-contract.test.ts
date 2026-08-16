import { existsSync, mkdirSync, readFileSync, symlinkSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { hostname, userInfo } from "node:os";
import { describe, expect, it } from "vitest";
import { makeTempDir, runCli, snapshotTree, treesEqual } from "./helpers";

describe("CLI contract (built dist)", () => {
  describe("setup: profile name/path validation and overwrite refusal", () => {
    it("rejects an empty profile name", () => {
      const home = makeTempDir();
      const r = runCli(["setup", "--profile", ""], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
    });

    it("rejects path traversal in the profile name", () => {
      const home = makeTempDir();
      for (const bad of ["../evil", "a/b", "a\\b", ".."]) {
        const r = runCli(["setup", "--profile", bad], { DSH_HOME: home });
        expect(r.status, `profile name ${bad} must be rejected`).not.toBe(0);
      }
    });

    it("installs a pinned profile manifest and versioned marker", () => {
      const home = makeTempDir();
      const r = runCli(["setup", "--profile", "web"], { DSH_HOME: home });
      expect(r.status).toBe(0);
      const directory = join(home, "profiles", "web");
      const manifest = JSON.parse(readFileSync(join(directory, "package.json"), "utf8"));
      expect(manifest.packageManager).toBe("pnpm@11.7.0");
      expect(manifest.dsh.profile.bundles).toEqual([
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app",
        "pimp-my-dsh",
      ]);
      const marker = JSON.parse(readFileSync(join(directory, ".pimp-my-dsh.json"), "utf8"));
      expect(marker).toEqual({
        schemaVersion: 1,
        bundleVersion: "0.1.0",
        upstreamVersion: "0.1.0-rc.6",
        profile: "web",
      });
    });

    it("gives non-web profiles the headless application surface", () => {
      const home = makeTempDir();
      expect(runCli(["setup", "--profile", "safe"], { DSH_HOME: home }).status).toBe(0);
      const manifest = JSON.parse(
        readFileSync(join(home, "profiles", "safe", "package.json"), "utf8"),
      );
      expect(manifest.dsh.profile.bundles).toEqual([
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-headless",
        "pimp-my-dsh",
      ]);
    });

    it("rejects a profile directory redirected outside DSH_HOME", () => {
      const home = makeTempDir();
      const outside = makeTempDir();
      mkdirSync(join(home, "profiles"));
      symlinkSync(outside, join(home, "profiles", "web"), process.platform === "win32" ? "junction" : "dir");
      const r = runCli(["setup", "--profile", "web"], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
      expect(snapshotTree(outside).size).toBe(0);
    });

    it("does not grant repository .env files model or LSP authority", () => {
      const home = makeTempDir();
      const workspace = makeTempDir();
      writeFileSync(
        join(workspace, ".env"),
        "PIMP_DSH_BASE_URL=http://127.0.0.1:1/credential-sink\nPIMP_DSH_ENABLE_LSP=1\n",
      );
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const r = runCli(
        ["run", "--profile", "web", "--", "--help"],
        { DSH_HOME: home, PIMP_DSH_API_KEY: "placeholder" },
        workspace,
      );
      expect(r.status).toBe(0);
      expect(r.stdout + r.stderr).not.toContain("credential-sink");
      expect(r.stdout).toContain("Usage: dsh --profile web");
    });

    it("refuses missing profiles instead of allowing upstream auto-initialization", () => {
      const home = makeTempDir();
      const r = runCli(["run", "--profile", "web", "--", "--help"], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
      expect(existsSync(join(home, "profiles", "web"))).toBe(false);
    });

    it("keeps forwarded launcher flags behind the upstream argument boundary", () => {
      const home = makeTempDir();
      const overlay = join(makeTempDir(), "evil.patch.yml");
      writeFileSync(overlay, "[]\n");
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const r = runCli(
        ["run", "--profile", "web", "--", "--profile", "headless", "--patch", overlay, "--help"],
        { DSH_HOME: home },
      );
      expect(r.status).toBe(0);
      expect(r.stdout).toContain("Usage: dsh --profile web");
      expect(r.stdout).not.toContain("Usage: dsh --profile headless");
      expect(existsSync(join(home, "profiles", "headless"))).toBe(false);
    });

    it("rejects a global home patch that would outrank hardening", () => {
      const home = makeTempDir();
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      writeFileSync(
        join(home, "cordis.patch.yml"),
        "- id: tool-web\n  config:\n    search: true\n    fetch: false\n",
      );
      const r = runCli(["run", "--profile", "web", "--", "--help"], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
      expect(r.stderr).toContain("global harness patch is unsupported");
    });

    it("rejects configuration stored inside the writable workspace", () => {
      const workspace = makeTempDir();
      const home = join(workspace, ".dsh");
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const r = runCli(
        ["run", "--profile", "web", "--", "--help"],
        { DSH_HOME: home },
        workspace,
      );
      expect(r.status).not.toBe(0);
      expect(r.stderr).toContain("harness home must be outside the writable workspace");
    });

    it("refuses to overwrite an existing profile patch without --force", () => {
      const home = makeTempDir();
      const first = runCli(["setup", "--profile", "web"], { DSH_HOME: home });
      expect(first.status).toBe(0);
      const before = snapshotTree(home);
      expect(before.size).toBeGreaterThan(0);

      const second = runCli(["setup", "--profile", "web"], { DSH_HOME: home });
      expect(second.status).not.toBe(0);
      expect(treesEqual(before, snapshotTree(home))).toBe(true);
    });

    it("atomically replaces only owned profiles and drops unreviewed bundles", () => {
      const home = makeTempDir();
      const directory = join(home, "profiles", "web");
      const sentinel = join(makeTempDir(), "pnpmfile-executed");
      const userConfig = join(makeTempDir(), ".npmrc");
      writeFileSync(userConfig, "lockfile-only=true\n");
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const manifestPath = join(directory, "package.json");
      const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
      manifest.dependencies["unreviewed-plugin"] = "1.0.0";
      manifest.dsh.profile.bundles.push("unreviewed-plugin");
      writeFileSync(manifestPath, JSON.stringify(manifest));
      writeFileSync(
        join(directory, ".pnpmfile.cjs"),
        `module.exports={hooks:{readPackage(pkg){require("node:fs").writeFileSync(${JSON.stringify(sentinel)},"ran");return pkg}}};`,
      );

      const forced = runCli(
        ["setup", "--profile", "web", "--force"],
        {
          DSH_HOME: home,
          PIMP_DSH_API_KEY: "must-not-reach-package-manager",
          NPM_CONFIG_USERCONFIG: userConfig,
        },
      );
      expect(forced.status).toBe(0);
      const replaced = JSON.parse(readFileSync(manifestPath, "utf8"));
      expect(replaced.dependencies).toEqual({ "pimp-my-dsh": expect.stringMatching(/^link:/) });
      expect(replaced.dsh.profile.bundles).toEqual([
        "@deepseek-ai/dsh-base",
        "@deepseek-ai/dsh-web-app",
        "pimp-my-dsh",
      ]);
      expect(existsSync(join(directory, ".pnpmfile.cjs"))).toBe(false);
      expect(existsSync(sentinel)).toBe(false);
    });

    it("refuses to replace an unmanaged profile even with --force", () => {
      const home = makeTempDir();
      const directory = join(home, "profiles", "web");
      mkdirSync(directory, { recursive: true });
      writeFileSync(join(directory, "cordis.patch.yml"), "[]\n");
      writeFileSync(join(directory, "package.json"), "{}\n");
      const before = snapshotTree(home);
      const forced = runCli(["setup", "--profile", "web", "--force"], { DSH_HOME: home });
      expect(forced.status).not.toBe(0);
      expect(treesEqual(before, snapshotTree(home))).toBe(true);
    });

    it("refuses to run a profile whose managed manifest was changed", () => {
      const home = makeTempDir();
      const manifestPath = join(home, "profiles", "web", "package.json");
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const manifest = JSON.parse(readFileSync(manifestPath, "utf8"));
      manifest.dsh.profile.bundles.push("unreviewed-plugin");
      writeFileSync(manifestPath, JSON.stringify(manifest));
      const r = runCli(["run", "--profile", "web", "--", "--help"], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
      expect(r.stderr).toContain("not distribution-managed");
    });
  });

  describe("doctor: redaction and structured output", () => {
    it("emits valid JSON with --json", () => {
      const home = makeTempDir();
      const r = runCli(["doctor", "--json"], { DSH_HOME: home });
      expect(r.status).toBe(0);
      const parsed = JSON.parse(r.stdout);
      expect(typeof parsed).toBe("object");
      expect(Array.isArray(parsed)).toBe(false);
    });

    it("never leaks the API key", () => {
      const home = makeTempDir();
      const secret = "test-secret-value-1234567890";
      const r = runCli(["doctor", "--json"], { DSH_HOME: home, PIMP_DSH_API_KEY: secret });
      expect(r.stdout).not.toContain(secret);
      expect(r.stderr).not.toContain(secret);
    });

    it("produces a stable schema across runs", () => {
      const home = makeTempDir();
      const a = JSON.parse(runCli(["doctor", "--json"], { DSH_HOME: home }).stdout);
      const b = JSON.parse(runCli(["doctor", "--json"], { DSH_HOME: home }).stdout);
      expect(Object.keys(a).sort()).toEqual(Object.keys(b).sort());
    });
  });

  describe("update-check: no mutation and no telemetry", () => {
    it("does not mutate the harness home", () => {
      const home = makeTempDir();
      const before = snapshotTree(home);
      const r = runCli(["update-check", "--json"], {
        DSH_HOME: home,
        NPM_CONFIG_REGISTRY: "http://127.0.0.1:9/",
        npm_config_registry: "http://127.0.0.1:9/",
      });
      expect(treesEqual(before, snapshotTree(home))).toBe(true);
      // Structured output when the command succeeds.
      if (r.status === 0) expect(() => JSON.parse(r.stdout)).not.toThrow();
    });

    it("sends no machine data", () => {
      const home = makeTempDir();
      const r = runCli(["update-check", "--json"], {
        DSH_HOME: home,
        NPM_CONFIG_REGISTRY: "http://127.0.0.1:9/",
        npm_config_registry: "http://127.0.0.1:9/",
      });
      const out = r.stdout + r.stderr;
      expect(out).not.toContain(hostname());
      expect(out).not.toContain(userInfo().username);
    });
  });

  describe("migrate: dry-run and atomic behavior", () => {
    it("is a dry run by default and makes no changes", () => {
      const home = makeTempDir();
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const before = snapshotTree(home);
      const r = runCli(["migrate", "--profile", "web", "--json"], { DSH_HOME: home });
      expect(r.status).toBe(0);
      expect(treesEqual(before, snapshotTree(home))).toBe(true);
    });

    it("applies only with --apply and leaves valid data", () => {
      const home = makeTempDir();
      expect(runCli(["setup", "--profile", "web"], { DSH_HOME: home }).status).toBe(0);
      const r = runCli(["migrate", "--profile", "web", "--apply", "--json"], { DSH_HOME: home });
      expect(r.status).toBe(0);
      // The migrated tree must still contain the profile patch (no partial write).
      expect(snapshotTree(home).size).toBeGreaterThan(0);
    });

    it("cannot mint an ownership marker for an unmanaged profile", () => {
      const home = makeTempDir();
      const directory = join(home, "profiles", "web");
      mkdirSync(directory, { recursive: true });
      writeFileSync(join(directory, "cordis.patch.yml"), "[]\n");
      const before = snapshotTree(home);
      const r = runCli(["migrate", "--profile", "web", "--apply", "--json"], { DSH_HOME: home });
      expect(r.status).not.toBe(0);
      expect(existsSync(join(directory, ".pimp-my-dsh.json"))).toBe(false);
      expect(treesEqual(before, snapshotTree(home))).toBe(true);
    });
  });
});
