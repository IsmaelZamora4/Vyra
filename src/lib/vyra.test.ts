import { beforeEach, describe, expect, it, vi } from "vitest";

const { invokeMock } = vi.hoisted(() => ({ invokeMock: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: invokeMock, isTauri: () => true }));

import { cancelDiagnostic, getDiagnostic, prepareDiagnostic, setFiltersEnabled, startDiagnostic } from "./vyra";

describe("Tauri IPC command contract", () => {
  beforeEach(() => invokeMock.mockReset());

  it("uses registered command names and the Rust camelCase argument keys", async () => {
    invokeMock.mockResolvedValue(false);
    await prepareDiagnostic();
    await getDiagnostic();
    await cancelDiagnostic();
    await startDiagnostic("attempt-1");
    await setFiltersEnabled(true);

    expect(invokeMock.mock.calls).toEqual([
      ["prepare_diagnostic", undefined],
      ["get_diagnostic", undefined],
      ["cancel_diagnostic", undefined],
      ["start_diagnostic", { attemptId: "attempt-1" }],
      ["set_filters_enabled", { enabled: true }],
    ]);
  });

  it("preserves dispatcher failures instead of reporting a successful operation", async () => {
    invokeMock.mockRejectedValueOnce("Apply rejected by the native gate");
    await expect(setFiltersEnabled(true)).rejects.toBe("Apply rejected by the native gate");
  });
});
