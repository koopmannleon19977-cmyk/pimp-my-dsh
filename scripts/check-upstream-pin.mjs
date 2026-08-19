#!/usr/bin/env node

import { appendFileSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const packageJson = JSON.parse(readFileSync(join(ROOT, "package.json"), "utf8"));
const policy = JSON.parse(readFileSync(join(ROOT, "schema", "upstream-release-policy-v1.json"), "utf8"));
const currentPin = policy.currentPin;
const upstreamPackages = Object.entries(packageJson.dependencies ?? {})
  .filter(([name]) => name.startsWith("@deepseek-ai/dsh"))
  .map(([name, version]) => ({ name, version }));
const VERSION = /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?$/;

function parseVersion(value) {
  const match = VERSION.exec(value);
  if (match === null) return null;
  return {
    major: Number(match[1]),
    minor: Number(match[2]),
    patch: Number(match[3]),
    prerelease: match[4]?.split(".") ?? [],
  };
}

function compareVersions(left, right) {
  const a = parseVersion(left);
  const b = parseVersion(right);
  if (a === null || b === null) return 0;
  for (const key of ["major", "minor", "patch"]) {
    if (a[key] !== b[key]) return a[key] - b[key];
  }
  if (a.prerelease.length === 0 && b.prerelease.length > 0) return 1;
  if (a.prerelease.length > 0 && b.prerelease.length === 0) return -1;
  for (let index = 0; index < Math.max(a.prerelease.length, b.prerelease.length); index += 1) {
    const leftPart = a.prerelease[index];
    const rightPart = b.prerelease[index];
    if (leftPart === undefined) return -1;
    if (rightPart === undefined) return 1;
    if (leftPart === rightPart) continue;
    const leftNumber = /^\d+$/.test(leftPart) ? Number(leftPart) : null;
    const rightNumber = /^\d+$/.test(rightPart) ? Number(rightPart) : null;
    if (leftNumber !== null && rightNumber !== null) return leftNumber - rightNumber;
    if (leftNumber !== null) return -1;
    if (rightNumber !== null) return 1;
    return leftPart < rightPart ? -1 : 1;
  }
  return 0;
}

async function inspectPackage({ name, version }) {
  const url = `https://registry.npmjs.org/${encodeURIComponent(name)}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(15_000) });
  if (!response.ok) throw new Error(`${name}: registry returned HTTP ${response.status}`);
  const metadata = await response.json();
  const versions = Object.keys(metadata.versions ?? {}).filter((candidate) => parseVersion(candidate) !== null);
  const newest = versions.sort(compareVersions).at(-1);
  if (newest === undefined) throw new Error(`${name}: registry returned no SemVer versions`);
  return {
    name,
    pinned: version,
    newest,
    latestTag: metadata["dist-tags"]?.latest ?? null,
    currentPublished: metadata.versions?.[version] !== undefined,
    updateAvailable: compareVersions(newest, version) > 0,
  };
}

const results = [];
const errors = [];
for (const dependency of upstreamPackages) {
  try {
    results.push(await inspectPackage(dependency));
  } catch (error) {
    errors.push(error instanceof Error ? error.message : String(error));
  }
}

const updates = results.filter((result) => result.updateAvailable);
const missingPins = results.filter((result) => !result.currentPublished);
const report = { currentPin, results, updates, missingPins, errors };
if (process.argv.includes("--json")) {
  process.stdout.write(`${JSON.stringify(report, null, 2)}\n`);
} else {
  process.stdout.write(`Upstream pin: ${currentPin}\n`);
  for (const result of results) {
    process.stdout.write(`${result.name}: pinned=${result.pinned} newest=${result.newest} latest=${result.latestTag ?? "unknown"}\n`);
  }
  for (const message of errors) process.stdout.write(`ERROR: ${message}\n`);
  for (const result of updates) process.stdout.write(`UPDATE AVAILABLE: ${result.name} -> ${result.newest}\n`);
  for (const result of missingPins) process.stdout.write(`PIN MISSING FROM REGISTRY: ${result.name}@${result.pinned}\n`);
}

if (process.env.GITHUB_STEP_SUMMARY) {
  const lines = [
    "## Upstream pin monitor",
    `- Current pin: \`${currentPin}\``,
    `- Packages checked: ${results.length}/${upstreamPackages.length}`,
    `- Updates available: ${updates.length}`,
    `- Registry errors: ${errors.length}`,
    "",
  ];
  appendFileSync(process.env.GITHUB_STEP_SUMMARY, `${lines.join("\n")}\n`);
}

process.exitCode = errors.length > 0 || updates.length > 0 || missingPins.length > 0 ? 1 : 0;
