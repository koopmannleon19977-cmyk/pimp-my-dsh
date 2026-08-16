import { describe, expect, it } from "vitest";
import { collectFiles, PROFILES_DIR, readText } from "./helpers";

describe("profile patches", () => {
  const files = collectFiles(PROFILES_DIR);

  it("ships at least one profile patch", () => {
    expect(files.length).toBeGreaterThan(0);
  });

  it("contains only data files (YAML/JSON), never code", () => {
    for (const f of files) {
      expect(f, `${f} must be a data file`).toMatch(/\.(ya?ml|json)$/);
    }
  });

  it("each patch is a valid non-empty patch-list representation", () => {
    for (const f of files) {
      const text = readText(f);
      expect(text.trim().length, `${f} must be non-empty`).toBeGreaterThan(0);
      expect(text, `${f} must be [] or use stable row ids`).toMatch(/^\s*\[\]\s*$|^\s*-\s+id:\s*\S/m);
    }
  });

  it("makes the safe profile read-only and approval-gated", () => {
    const path = files.find((file) => file.endsWith("safe.patch.yml"));
    expect(path).toBeDefined();
    const text = readText(path!);
    expect(text).toMatch(/defaultPreset:\s*read-only/);
    expect(text).toMatch(/sandbox:\s*read-only/);
    expect(text).toMatch(/approval:\s*ask/);
  });
});
