import { describe, expect, it } from "vitest";
import { defaultSettings, parseProfileExport } from "./app";

const exportWith = (settings: typeof defaultSettings) => JSON.stringify({
  schemaVersion: 1,
  profiles: [{ id: "test", name: "Test", builtin: true, settings }],
});

describe("parseProfileExport", () => {
  it("accepts versioned profiles with valid settings", () => {
    expect(parseProfileExport(exportWith({ ...defaultSettings, saturation: 24 })).profiles).toHaveLength(1);
  });

  it("rejects unsupported versions and out-of-range values", () => {
    expect(() => parseProfileExport('{"schemaVersion":2,"profiles":[]}')).toThrow("Versión o cantidad");
    expect(() => parseProfileExport(exportWith({ ...defaultSettings, hue: 181 }))).toThrow("fuera de rango");
  });
});
