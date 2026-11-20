import { useEffect, useRef, useState, type SyntheticEvent } from "react";
import { cancelDiagnostic, getDiagnostic, handoffDiagnostic, prepareDiagnostic, startDiagnostic } from "../../lib/vyra";
import type { DiagnosticReport, EngineStatus } from "../../types/app";

const accepted = (value: boolean | null) => value === null ? "No ejecutada" : value ? "Aceptada" : "Rechazada";
const phaseLabels: Record<DiagnosticReport["phase"], string> = {
  notStarted: "Sin iniciar", preflightPending: "Preflight en curso", preflightAccepted: "Preflight aceptado",
  confirmationAvailable: "Confirmación disponible", confirming: "Esperando confirmación nativa",
  consentIncidentLockdown: "Bloqueado por incidente de consentimiento",
  sessionExpired: "Sesión caducada", recoveryPending: "Recuperación pendiente",
  externalConflict: "Conflicto externo", watchdogError: "Error del watchdog", applying: "Aplicando",
  applied: "Aplicado y verificado", outcomeUncertain: "Resultado incierto; recuperación prioritaria", auditPersistenceFailed: "Auditoría incompleta", cancelled: "Cancelado", restored: "Restaurado", failed: "Fallido",
};
const blockerLabels: Record<NonNullable<DiagnosticReport["blockerCode"]>, string> = {
  backendMismatch: "Selecciona Windows experimental.", filtersActive: "Desactiva filtros activos.",
  recoveryPending: "Resuelve la recuperación pendiente.", windowsReadFailed: "Windows no aceptó la lectura.",
  helperUnavailable: "El watchdog no está disponible o verificado.", preflightExpired: "El preflight caducó; prepara uno nuevo.",
  externalConflict: "Otra aplicación cambió la matriz; no se sobrescribió.", watchdogError: "El watchdog no confirmó el estado esperado.",
  confirmationDeclined: "Confirmación rechazada; no se aplicó color.", nativeConfirmationFailed: "Windows no pudo mostrar la confirmación; no se aplicó color.", sessionExpired: "La sesión caducó; no se aplicó color.",
  consentIncidentLockdown: "Ensayo visual deshabilitado; preflight de lectura y recuperación manual siguen disponibles.",
  applyOutcomeUncertain: "No se pudo confirmar el resultado de Apply; restaura y conserva el diario.",
  auditPersistenceFailed: "El watchdog informa el resultado, pero falta persistir un evento de auditoría; revisa watchdog-result.json.",
};

export function DiagnosticPanel({ status, onRestore }: { status: EngineStatus; onRestore: () => void }) {
  const [report, setReport] = useState<DiagnosticReport | null>(null);
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  const [monitoring, setMonitoring] = useState(false);
  const dialog = useRef<HTMLDialogElement>(null);
  useEffect(() => {
    if (!monitoring) return;
    const timer = setInterval(() => { void getDiagnostic().then(setReport).catch((reason: unknown) => setError(String(reason))); }, 500);
    return () => clearInterval(timer);
  }, [monitoring]);

  async function prepare() {
    setBusy(true); setError("");
    try { setReport(await prepareDiagnostic()); setMonitoring(true); dialog.current?.showModal(); }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  async function cancel() {
    setBusy(true); setError("");
    try { setReport(await cancelDiagnostic()); }
    catch (reason) { setError(String(reason)); }
    finally { setMonitoring(false); dialog.current?.close(); setBusy(false); }
  }
  async function handoff() {
    setBusy(true); setError("");
    try { setReport(await handoffDiagnostic()); }
    catch (reason) { setError(String(reason)); }
    finally { setMonitoring(false); dialog.current?.close(); setBusy(false); }
  }
  async function start() {
    setBusy(true); setError("");
    try {
      const started = await startDiagnostic(report?.attemptId ?? "");
      const next = await getDiagnostic();
      setReport(next);
      if (!started) { setMonitoring(false); dialog.current?.close(); }
    }
    catch (reason) { setError(String(reason)); }
    finally { setBusy(false); }
  }
  function handleDialogCancel(event: SyntheticEvent<HTMLDialogElement>) {
    event.preventDefault();
    if (report?.applicationAccepted == null) void cancel();
    else onRestore();
  }
  return <section className="diagnostic-panel" aria-label="Diagnóstico experimental">
    <strong>Primera prueba de color global</strong>
    <p>Preflight de solo lectura disponible. El ensayo visual está deshabilitado por incidente de consentimiento; no se aplicará color.</p>
    <button className="button button-quiet" disabled={busy || status.enabled || status.recoveryRequired} onClick={() => void prepare()}>Preparar diagnóstico</button>
    {report?.stage === "cancelled" && <p role="status">Cancelado; sin cambios de color.</p>}
    {error && <p role="alert">{error}</p>}
    <dialog ref={dialog} className="profile-dialog" aria-labelledby="diagnostic-title" onCancel={handleDialogCancel}>
      <h2 id="diagnostic-title">Diagnóstico Windows experimental</h2>
      {report && <>
        <p>Saturación +{report.saturation}; duración máxima {report.maxDurationSeconds} segundos.</p>
        <p>Estado: {phaseLabels[report.phase]}; confirmación disponible como máximo durante {report.maxPreflightWaitSeconds} segundos.</p>
        {report.blockerCode && <p role="alert">{blockerLabels[report.blockerCode]}</p>}
        <p role="status">{report.reason}</p>
        <p>Watchdog: {report.watchdogState} · {report.helperVerified ? "Ejecutable verificado" : "Verificación pendiente"}{report.helperPid ? ` · PID ${report.helperPid}` : ""}.</p>
        <p>Lectura de Windows: {accepted(report.currentStateReadAccepted)}.</p>
        <p>Confirmación nativa: {report.confirmationAccepted ? "Aceptada" : "Pendiente"}.</p>
        <p>Watchdog: inicialización {accepted(report.helperInitializationAccepted)}, lectura {accepted(report.helperStateReadAccepted)}, diario durable {accepted(report.helperJournalDurable)}, misma sesión Windows {accepted(report.helperSameWindowsSession)}.</p>
        <p>Aplicación: {accepted(report.applicationAccepted)}; lectura de retorno {accepted(report.applicationVerified)}. Restauración: {accepted(report.restorationAccepted)}; lectura de retorno {accepted(report.restorationVerified)}.</p>
        <div className="dialog-actions">
          <button className="button button-quiet" disabled={busy || report.applicationAccepted != null || report.stage === "applying"} onClick={() => void cancel()}>Cancelar sin aplicar</button>
          <button className="button button-quiet" disabled={busy || report.applicationAccepted != null || report.stage === "applying"} onClick={() => void handoff()}>Cancelar y entregar control manual</button>
          <button className="button button-quiet" disabled={!status.enabled && !status.recoveryRequired && !["armed", "applying", "applied", "pendingRecovery", "failed"].includes(report.watchdogState)} onClick={onRestore}>Restaurar ahora</button>
          <button className="button" disabled={busy || !report.canRequestConfirmation} onClick={() => void start()}>Iniciar prueba</button>
        </div>
      </>}
    </dialog>
  </section>;
}
