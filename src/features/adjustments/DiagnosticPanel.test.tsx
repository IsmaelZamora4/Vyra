import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { DiagnosticPanel } from "./DiagnosticPanel";
import type { DiagnosticReport } from "../../types/app";

const report: DiagnosticReport = {
  attemptId: null,
  stage: "blocked", phase: "recoveryPending", blockerCode: "recoveryPending", maxDurationSeconds: 10, maxPreflightWaitSeconds: 60, saturation: 5,
  independentRecoveryReady: false, currentStateReadAccepted: true,
  confirmationAccepted: false, applicationAccepted: null, applicationVerified: null, restorationAccepted: null, canRequestConfirmation: false,
  reason: "Recuperación independiente pendiente.", timestampMs: 1,
  watchdogState: "armed", helperVerified: true, helperPid: 123,
  helperInitializationAccepted: true, helperStateReadAccepted: true,
  helperJournalDurable: true, helperSameWindowsSession: true, restorationVerified: null,
};
vi.mock("../../lib/vyra", () => ({
  prepareDiagnostic: vi.fn(async () => report),
  cancelDiagnostic: vi.fn(async () => ({ ...report, stage: "cancelled", phase: "cancelled" })),
  handoffDiagnostic: vi.fn(async () => ({ ...report, stage: "cancelled", phase: "cancelled", reason: "Cierra automatización; VYRA no verifica el traspaso." })),
  startDiagnostic: vi.fn(async () => false),
  getDiagnostic: vi.fn(async () => report),
}));
import { prepareDiagnostic, cancelDiagnostic, handoffDiagnostic, getDiagnostic, startDiagnostic } from "../../lib/vyra";

describe("DiagnosticPanel safety", () => {
  it("only prepares on manual click, blocks start, and cancels without activating", async () => {
    const reactEnvironment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
    reactEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.append(container);
    // jsdom does not implement the native dialog methods.
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
    const root = createRoot(container);
    const onRestore = vi.fn();
    await act(async () => root.render(<DiagnosticPanel status={{ enabled: false, recoveryRequired: false, description: "Apagado" }} onRestore={onRestore} />));
    expect(prepareDiagnostic).not.toHaveBeenCalled();
    const prepare = [...container.querySelectorAll("button")].find((button) => button.textContent === "Preparar diagnóstico")!;
    await act(async () => prepare.click());
    expect(prepareDiagnostic).toHaveBeenCalledTimes(1);
    expect(container.querySelector("dialog")?.open).toBe(true);
    const start = [...container.querySelectorAll("button")].find((button) => button.textContent === "Iniciar prueba")!;
    expect(start.disabled).toBe(true);
    expect(container.textContent).toContain("Recuperación pendiente");
    expect(container.textContent).toContain("Resuelve la recuperación pendiente.");
    await act(async () => start.click());
    expect(startDiagnostic).not.toHaveBeenCalled();
    const cancel = [...container.querySelectorAll("button")].find((button) => button.textContent === "Cancelar sin aplicar")!;
    await act(async () => cancel.click());
    expect(cancelDiagnostic).toHaveBeenCalledTimes(1);
    expect(onRestore).not.toHaveBeenCalled();
    expect(container.querySelector("dialog")?.open).toBe(false);
    await act(async () => prepare.click());
    vi.mocked(cancelDiagnostic).mockRejectedValueOnce(new Error("No se pudo escribir el registro"));
    await act(async () => cancel.click());
    expect(container.querySelector("dialog")?.open).toBe(false);
    expect(container.textContent).toContain("No se pudo escribir el registro");
    expect(startDiagnostic).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it("keeps Apply disabled after a successful preflight during incident lockdown", async () => {
    const reactEnvironment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
    reactEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.append(container);
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
    const armed = { ...report, stage: "awaitingConfirmation" as const, phase: "consentIncidentLockdown" as const, blockerCode: "consentIncidentLockdown" as const, canRequestConfirmation: false, reason: "Ensayo visual deshabilitado por incidente." };
    vi.mocked(prepareDiagnostic).mockResolvedValueOnce(armed);
    const root = createRoot(container);
    await act(async () => root.render(<DiagnosticPanel status={{ enabled: false, recoveryRequired: false, description: "Apagado" }} onRestore={vi.fn()} />));
    const prepare = [...container.querySelectorAll("button")].find((button) => button.textContent === "Preparar diagnóstico")!;
    await act(async () => prepare.click());
    const start = [...container.querySelectorAll("button")].find((button) => button.textContent === "Iniciar prueba")!;
    expect(start.disabled).toBe(true);
    expect(container.textContent).toContain("Ensayo visual deshabilitado por incidente.");
    await act(async () => start.click());
    expect(startDiagnostic).not.toHaveBeenCalled();
    expect(container.querySelector("dialog")?.open).toBe(true);
    const cancel = [...container.querySelectorAll("button")].find((button) => button.textContent === "Cancelar sin aplicar")!;
    await act(async () => cancel.click());
    expect(cancelDiagnostic).toHaveBeenCalledTimes(1);
    expect(container.querySelector("dialog")?.open).toBe(false);
    await act(async () => root.unmount());
    container.remove();
  });

  it("cancels and closes the diagnostic before manual handoff", async () => {
    const reactEnvironment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
    reactEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.append(container);
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
    const root = createRoot(container);
    await act(async () => root.render(<DiagnosticPanel status={{ enabled: false, recoveryRequired: false, description: "Apagado" }} onRestore={vi.fn()} />));
    const prepare = [...container.querySelectorAll("button")].find((button) => button.textContent === "Preparar diagnóstico")!;
    await act(async () => prepare.click());
    const handoff = [...container.querySelectorAll("button")].find((button) => button.textContent === "Cancelar y entregar control manual")!;
    await act(async () => handoff.click());
    expect(handoffDiagnostic).toHaveBeenCalledTimes(1);
    expect(container.querySelector("dialog")?.open).toBe(false);
    expect(container.textContent).toContain("VYRA no verifica el traspaso.");
    expect(startDiagnostic).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
  });

  it("polling only queries diagnostic state and never requests start", async () => {
    vi.useFakeTimers();
    vi.clearAllMocks();
    const reactEnvironment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
    reactEnvironment.IS_REACT_ACT_ENVIRONMENT = true;
    const container = document.createElement("div");
    document.body.append(container);
    HTMLDialogElement.prototype.showModal = function () { this.setAttribute("open", ""); };
    HTMLDialogElement.prototype.close = function () { this.removeAttribute("open"); };
    const root = createRoot(container);
    await act(async () => root.render(<DiagnosticPanel status={{ enabled: false, recoveryRequired: false, description: "Apagado" }} onRestore={vi.fn()} />));
    const prepare = [...container.querySelectorAll("button")].find((button) => button.textContent === "Preparar diagnóstico")!;
    await act(async () => prepare.click());
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(getDiagnostic).toHaveBeenCalledTimes(2);
    expect(startDiagnostic).not.toHaveBeenCalled();
    await act(async () => root.unmount());
    container.remove();
    vi.useRealTimers();
  });
});
