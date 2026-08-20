import { describe, expect, it, vi } from "vitest";
import { verifyCommunityPluginAllowlist } from "../scripts/verify-community-plugin-allowlist.mjs";

const plugin = {
  name: "@example/reviewed-plugin",
  version: "1.2.3",
  integrity: "sha512-YWJjZA==",
  source: "https://example.invalid/reviewed-plugin",
  license: "MIT",
  permissions: { filesystem: "workspace", network: "public", process: "none" },
  windows: { reviewed: true, notes: "Reviewed on Windows." },
  reviewedBy: "Reviewer",
  reviewedAt: "2026-08-20T12:00:00Z",
};
const allowlist = { schemaVersion: 1, plugins: [plugin] };
const metadata = {
  name: plugin.name,
  version: plugin.version,
  dist: { integrity: plugin.integrity },
  license: plugin.license,
};

function response(body: unknown, ok = true, status = 200) {
  return { ok, status, json: async () => body };
}

describe("community plugin allowlist verifier", () => {
  it("passes an empty allowlist without a registry request", async () => {
    const fetchImpl = vi.fn();

    await expect(verifyCommunityPluginAllowlist({ schemaVersion: 1, plugins: [] }, fetchImpl)).resolves.toEqual([]);
    expect(fetchImpl).not.toHaveBeenCalled();
  });

  it("passes when exact registry metadata matches the reviewed pin", async () => {
    const fetchImpl = vi.fn(async () => response(metadata));

    await expect(verifyCommunityPluginAllowlist(allowlist, fetchImpl)).resolves.toEqual([
      "@example/reviewed-plugin@1.2.3",
    ]);
    expect(fetchImpl).toHaveBeenCalledWith(
      "https://registry.npmjs.org/%40example%2Freviewed-plugin/1.2.3",
      expect.objectContaining({ signal: expect.any(AbortSignal) }),
    );
  });

  it("fails closed when the exact version is unavailable", async () => {
    const fetchImpl = vi.fn(async () => response({}, false, 404));

    await expect(verifyCommunityPluginAllowlist(allowlist, fetchImpl)).rejects.toThrow(
      "exact published version could not be verified: registry returned HTTP 404",
    );
  });

  it.each([
    ["exact version", { ...metadata, version: "1.2.4" }, "exact published version does not match"],
    ["integrity", { ...metadata, dist: { integrity: "sha512-ZGlmZmVyZW50" } }, "dist.integrity does not match"],
    ["license", { ...metadata, license: "Apache-2.0" }, "license does not match"],
  ])("fails closed on a %s mismatch", async (_field, registryMetadata, message) => {
    const fetchImpl = vi.fn(async () => response(registryMetadata));

    await expect(verifyCommunityPluginAllowlist(allowlist, fetchImpl)).rejects.toThrow(message);
  });
});
