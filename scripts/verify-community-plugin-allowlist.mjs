#!/usr/bin/env node

import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ALLOWLIST_PATH = join(ROOT, "schema", "community-plugin-allowlist-v1.json");
const PACKAGE_NAME = /^(?:@[a-z0-9][a-z0-9._-]*\/)?[a-z0-9][a-z0-9._-]*$/;
const VERSION = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$/;
const INTEGRITY = /^sha512-[A-Za-z0-9+/]+=*$/;
const REVIEWED_AT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?(?:Z|[+-]\d{2}:\d{2})$/;
const RESERVED = new Set([
  "@deepseek-ai/dsh-base",
  "@deepseek-ai/dsh-headless",
  "@deepseek-ai/dsh-web-app",
  "@deepseek-ai/dsh-lsp",
  "@deepseek-ai/dsh-lsp-stdio",
  "@deepseek-ai/dsh-tool-lsp",
  "@deepseek-ai/dsh-mcp-client",
  "@playwright/mcp",
  "pimp-my-dsh",
  "pnpm",
]);

function objectRecord(value) {
  return value !== null && typeof value === "object" && !Array.isArray(value) ? value : undefined;
}

function validatedPlugins(allowlist) {
  const root = objectRecord(allowlist);
  const plugins = root?.plugins;
  if (root?.schemaVersion !== 1 || !Array.isArray(plugins)) {
    throw new Error("community plugin allowlist must be schemaVersion 1 with a plugins array");
  }

  const names = new Set();
  for (const [index, raw] of plugins.entries()) {
    const plugin = objectRecord(raw);
    const permissions = objectRecord(plugin?.permissions);
    const windows = objectRecord(plugin?.windows);
    if (
      plugin === undefined
      || typeof plugin.name !== "string"
      || !PACKAGE_NAME.test(plugin.name)
      || RESERVED.has(plugin.name)
      || names.has(plugin.name)
      || typeof plugin.version !== "string"
      || !VERSION.test(plugin.version)
      || typeof plugin.integrity !== "string"
      || !INTEGRITY.test(plugin.integrity)
      || typeof plugin.source !== "string"
      || plugin.source.length === 0
      || typeof plugin.license !== "string"
      || plugin.license.length === 0
      || permissions === undefined
      || !["none", "workspace"].includes(permissions.filesystem)
      || !["none", "public"].includes(permissions.network)
      || !["none", "child"].includes(permissions.process)
      || windows === undefined
      || windows.reviewed !== true
      || typeof windows.notes !== "string"
      || windows.notes.length === 0
      || typeof plugin.reviewedBy !== "string"
      || plugin.reviewedBy.length === 0
      || typeof plugin.reviewedAt !== "string"
      || !REVIEWED_AT.test(plugin.reviewedAt)
      || !Number.isFinite(Date.parse(plugin.reviewedAt))
    ) {
      throw new Error(`community plugin allowlist entry ${index} is incomplete or not admissible`);
    }
    names.add(plugin.name);
  }
  return plugins;
}

function reason(error) {
  return error instanceof Error ? error.message : String(error);
}

export async function verifyCommunityPluginAllowlist(allowlist, fetchImpl = fetch) {
  const plugins = validatedPlugins(allowlist);
  const verified = [];
  for (const plugin of plugins) {
    const spec = `${plugin.name}@${plugin.version}`;
    let metadata;
    try {
      const response = await fetchImpl(
        `https://registry.npmjs.org/${encodeURIComponent(plugin.name)}/${encodeURIComponent(plugin.version)}`,
        { headers: { accept: "application/json" }, signal: AbortSignal.timeout(15_000) },
      );
      if (!response.ok) throw new Error(`registry returned HTTP ${response.status}`);
      metadata = await response.json();
    } catch (error) {
      throw new Error(`${spec}: exact published version could not be verified: ${reason(error)}`);
    }

    if (metadata?.name !== plugin.name || metadata?.version !== plugin.version) {
      throw new Error(`${spec}: exact published version does not match registry metadata`);
    }
    if (metadata?.dist?.integrity !== plugin.integrity) {
      throw new Error(`${spec}: dist.integrity does not match registry metadata`);
    }
    if (metadata?.license !== plugin.license) {
      throw new Error(`${spec}: license does not match registry metadata`);
    }
    verified.push(spec);
  }
  return verified;
}

function loadAllowlist() {
  try {
    return JSON.parse(readFileSync(ALLOWLIST_PATH, "utf8"));
  } catch (error) {
    throw new Error(`community plugin allowlist is unreadable: ${reason(error)}`);
  }
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  try {
    const verified = await verifyCommunityPluginAllowlist(loadAllowlist());
    process.stdout.write(`Community plugin allowlist verified: ${verified.length} plugin(s)\n`);
  } catch (error) {
    process.stderr.write(`ERROR: ${reason(error)}\n`);
    process.exitCode = 1;
  }
}
