import { existsSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { CORDIS_PATCH, PROFILES_DIR, readPackageJson, readText } from "./helpers";

const PIN = "0.1.0-rc.6";

describe("package.json contract", () => {
  const pkg = readPackageJson();

  it("is pimp-my-dsh 0.1.0 under MIT", () => {
    expect(pkg.name).toBe("pimp-my-dsh");
    expect(pkg.version).toBe("0.1.0");
    expect(pkg.license).toBe("MIT");
  });

  it("is ESM with the documented entry points", () => {
    expect(pkg.type).toBe("module");
    expect(pkg.main).toBe("dist/plugin.js");
    const bin = pkg.bin as Record<string, string>;
    expect(bin["pimp-dsh"]).toBe("dist/cli.js");
  });

  it("declares the bundle patch manifest", () => {
    const dsh = pkg.dsh as { bundle?: { patch?: string } };
    expect(dsh?.bundle?.patch).toBe("./cordis.patch.yml");
  });

  it("ships runtime, profile, documentation, and legal files", () => {
    const files = pkg.files as string[];
    expect(Array.isArray(files)).toBe(true);
    for (const required of [
      "dist",
      "cordis.patch.yml",
      "profiles",
      "schema",
      "docs",
      "SECURITY.md",
      "LICENSE",
    ]) {
      expect(files, `files must include ${required}`).toContain(required);
    }
    expect(files.some((f) => /^README(\.md)?$/i.test(f)), "files must include README").toBe(true);
    expect(
      files.some((f) => /^THIRD_PARTY_NOTICES(\.md)?$/i.test(f)),
      "files must include THIRD_PARTY_NOTICES",
    ).toBe(true);
  });

  it("declares the supported Node range", () => {
    const engines = pkg.engines as { node?: string };
    expect(engines?.node).toBe("^22.19.0 || >=24.0.0");
  });

  it("pins pnpm to 11.7.0", () => {
    expect(pkg.packageManager).toBe("pnpm@11.7.0");
  });

  it("bundles exact pnpm and builds before CLI contract tests", () => {
    const deps = pkg.dependencies as Record<string, string>;
    const scripts = pkg.scripts as Record<string, string>;
    expect(deps.pnpm).toBe("11.7.0");
    expect(scripts.pretest).toBe("pnpm run build");
  });

  it("pins @deepseek-ai/dsh to the exact rc.6", () => {
    const deps = pkg.dependencies as Record<string, string>;
    expect(deps["@deepseek-ai/dsh"]).toBe(PIN);
  });

  it("pins every direct @deepseek-ai/dsh-* package to the exact rc.6", () => {
    const deps = {
      ...((pkg.dependencies as Record<string, string>) ?? {}),
      ...((pkg.devDependencies as Record<string, string>) ?? {}),
      ...((pkg.peerDependencies as Record<string, string>) ?? {}),
    };
    const dshDeps = Object.entries(deps).filter(([k]) => k.startsWith("@deepseek-ai/dsh-"));
    expect(dshDeps.length).toBeGreaterThan(0);
    for (const [name, version] of dshDeps) {
      expect(version, `${name} must be exact ${PIN}`).toBe(PIN);
    }
  });

  it("uses no unsafe ranges or dist-tags for upstream packages", () => {
    const deps = {
      ...((pkg.dependencies as Record<string, string>) ?? {}),
      ...((pkg.devDependencies as Record<string, string>) ?? {}),
      ...((pkg.peerDependencies as Record<string, string>) ?? {}),
    };
    const upstream = Object.entries(deps).filter(
      ([k]) => k === "@deepseek-ai/dsh" || k.startsWith("@deepseek-ai/dsh-"),
    );
    const unsafe = /[\^~><*x|]|latest|next|workspace:|file:|link:|npm:/;
    for (const [name, version] of upstream) {
      expect(String(version), `${name} has an unsafe range`).not.toMatch(unsafe);
    }
  });
});

describe("bundle manifest and patch presence", () => {
  it("cordis.patch.yml exists and is non-empty", () => {
    expect(existsSync(CORDIS_PATCH)).toBe(true);
    expect(readText(CORDIS_PATCH).trim().length).toBeGreaterThan(0);
  });

  it("profiles directory is declared and present", () => {
    expect(existsSync(PROFILES_DIR)).toBe(true);
  });
});
