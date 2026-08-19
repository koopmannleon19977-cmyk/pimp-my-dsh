#!/usr/bin/env node

import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const tauriConfig = JSON.parse(readFileSync(join(ROOT, "apps", "desktop", "src-tauri", "tauri.conf.json"), "utf8"));
const policy = JSON.parse(readFileSync(join(ROOT, "schema", "upstream-release-policy-v1.json"), "utf8"));
const workflow = readFileSync(join(ROOT, ".github", "workflows", "release.yml"), "utf8");
const tag = process.argv.slice(2).find((value) => value !== "--json") ?? process.env.GITHUB_REF_NAME;
const requestedVersion = tag?.replace(/^v/, "");
const dependencies = packageJson.dependencies ?? {};
const upstream = Object.entries(dependencies).filter(([name]) => name.startsWith("@deepseek-ai/dsh"));
const errors = [];

if (requestedVersion !== undefined && requestedVersion !== packageJson.version) {
  errors.push(`tag ${tag} does not match package.json ${packageJson.version}`);
}
if (tauriConfig.version !== packageJson.version) {
  errors.push(`tauri.conf.json ${tauriConfig.version} does not match package.json ${packageJson.version}`);
}
if (policy.currentPin !== new Set(upstream.map(([, version]) => version)).values().next().value) {
  errors.push("upstream release policy does not match the direct dependency pin");
}
if (new Set(upstream.map(([, version]) => version)).size !== 1) {
  errors.push("direct @deepseek-ai/dsh packages do not share one exact pin");
}
for (const required of [
  "id-token: write",
  "attestations: write",
  "actions/attest-build-provenance@",
  "subject-checksums:",
  "attestation-subjects.sha256",
  "$env:SIG_PATH",
  "$env:CHECKSUMS_PATH",
]) {
  if (!workflow.includes(required)) errors.push(`release workflow is missing ${required}`);
}
for (const requiredPath of [
  "schema/upstream-release-policy-v1.json",
  "schema/upstream-release-policy-v1.schema.json",
  "scripts/check-upstream-pin.mjs",
]) {
  if (!existsSync(join(ROOT, requiredPath))) errors.push(`missing ${requiredPath}`);
}

const report = {
  version: packageJson.version,
  tag: tag ?? null,
  upstreamPin: policy.currentPin,
  upstreamPackages: upstream.length,
  signing: "CI-only; this preflight does not inspect or print signing secrets",
  errors,
};
if (process.argv.includes("--json")) process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
else {
  process.stdout.write(`Release preflight: ${report.version}\n`);
  process.stdout.write(`Upstream packages: ${report.upstreamPackages} @ ${report.upstreamPin}\n`);
  process.stdout.write("Signing: CI-only; secrets are not inspected locally\n");
  for (const error of errors) process.stdout.write(`ERROR: ${error}\n`);
}
process.exitCode = errors.length === 0 ? 0 : 1;
