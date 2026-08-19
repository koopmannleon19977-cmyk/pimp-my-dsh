import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { ROOT, readPackageJson, readText } from "./helpers";

const workflow = readText(join(ROOT, ".github", "workflows", "release.yml"));
const monitorWorkflow = readText(join(ROOT, ".github", "workflows", "upstream-monitor.yml"));
const pinPolicy = JSON.parse(
  readFileSync(join(ROOT, "schema", "upstream-release-policy-v1.json"), "utf8"),
) as {
  schemaVersion: number;
  currentPin: string;
  monitoring: { cadence: string; intervalDays: number; scope: string };
  plannedRepin: { cadence: string; intervalDays: number; requiresSinglePin: boolean; requiredChecks: string[] };
  securityFixes: { response: string; requiresSinglePin: boolean };
  lastReviewed: string;
};

const packageJson = readPackageJson();
const dependencies = packageJson.dependencies as Record<string, string>;


describe("release metadata contract", () => {
  it("keeps all direct upstream packages on one exact policy pin", () => {
    const upstream = Object.entries(dependencies).filter(([name]) => name.startsWith("@deepseek-ai/dsh"));
    expect(upstream.length).toBeGreaterThan(0);
    expect(pinPolicy.schemaVersion).toBe(1);
    expect(new Set(upstream.map(([, version]) => version))).toEqual(new Set([pinPolicy.currentPin]));
    expect(pinPolicy.monitoring).toMatchObject({ cadence: "weekly", intervalDays: 7 });
    expect(pinPolicy.plannedRepin).toMatchObject({ cadence: "monthly", intervalDays: 30, requiresSinglePin: true });
    expect(pinPolicy.securityFixes).toEqual({ response: "immediate", requiresSinglePin: true });
    expect(pinPolicy.plannedRepin.requiredChecks.length).toBeGreaterThan(0);
    expect(pinPolicy.lastReviewed).toMatch(/^\d{4}-\d{2}-\d{2}$/);
  });

  it("requires OIDC-backed GitHub artifact attestations in the release job", () => {
    expect(workflow).toContain("id-token: write");
    expect(workflow).toContain("attestations: write");
    expect(workflow).toMatch(/actions\/attest-build-provenance@[0-9a-f]{40}\s+# v2\.4\.0/);
    expect(workflow).toContain("subject-checksums:");
    expect(workflow).toContain("attestation-subjects.sha256");
    expect(workflow).toContain("$env:SIG_PATH");
    expect(workflow).toContain("$env:CHECKSUMS_PATH");
  });

  it("runs the upstream monitor weekly and invokes the local preflight", () => {
    expect(monitorWorkflow).toContain("cron: '30 9 * * 1'");
    expect(monitorWorkflow).toContain("node scripts/check-upstream-pin.mjs");
    expect(workflow).toContain("node scripts/release-preflight.mjs $env:GITHUB_REF_NAME");
    expect(readText(join(ROOT, "scripts", "release-preflight.mjs"))).toContain("signing");
  });

  it("documents the same cadence and verification path", () => {
    const doc = readText(join(ROOT, "docs", "upstream-pin.md"));
    expect(doc).toContain("schema/upstream-release-policy-v1.json");
    expect(doc).toContain("Weekly monitoring");
    expect(doc).toContain("Monthly planned re-pin");
    expect(doc).toContain("Immediate security response");
    expect(doc).toContain("gh attestation verify");
  });
});
