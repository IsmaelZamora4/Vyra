import { invoke, isTauri } from "@tauri-apps/api/core";
import type { DiagnosticReport } from "../types/app";
import { defaults, parseProfileExport, type BackendPreference, type Capabilities, type EngineStatus, type Profile, type ProfileConfig, type Settings } from "../types/app";

const STORAGE_KEY = "vyra.dev.profiles.v1";
let devEnabled = false;

function readDevConfig(): ProfileConfig {
  try {
    const value = localStorage.getItem(STORAGE_KEY);
    if (!value) return structuredClone(defaults);
    const parsed = JSON.parse(value) as ProfileConfig;
    if (parsed.schemaVersion !== 1 || !Array.isArray(parsed.profiles) || !parsed.profiles.length) return structuredClone(defaults);
    return { ...parsed, backendPreference: parsed.backendPreference ?? "automatic" };
  } catch {
    return structuredClone(defaults);
  }
}

function writeDevConfig(config: ProfileConfig): ProfileConfig {
  localStorage.setItem(STORAGE_KEY, JSON.stringify(config));
  return structuredClone(config);
}

const call = <T>(command: string, args?: Record<string, unknown>) => invoke<T>(command, args);
const inDesktop = () => isTauri();

function browserDiagnostic(stage: DiagnosticReport["stage"]): DiagnosticReport {
  return {
    attemptId: null,
    stage, phase: stage === "cancelled" ? "cancelled" : "watchdogError", blockerCode: "helperUnavailable",
    maxDurationSeconds: 10, maxPreflightWaitSeconds: 60, saturation: 5, independentRecoveryReady: false,
    currentStateReadAccepted: null, confirmationAccepted: false, applicationAccepted: null, applicationVerified: null, restorationAccepted: null,
    canRequestConfirmation: false, reason: "El diagnóstico real solo se prepara en la aplicación nativa. Recuperación independiente pendiente de validación.",
    timestampMs: Date.now(),
    watchdogState: "notStarted", helperVerified: false, helperPid: null,
    helperInitializationAccepted: null, helperStateReadAccepted: null,
    helperJournalDurable: null, helperSameWindowsSession: null, restorationVerified: null,
  };
}
export async function prepareDiagnostic(): Promise<DiagnosticReport> {
  return inDesktop() ? call("prepare_diagnostic") : browserDiagnostic("blocked");
}
export async function getDiagnostic(): Promise<DiagnosticReport> {
  return inDesktop() ? call("get_diagnostic") : browserDiagnostic("blocked");
}
export async function cancelDiagnostic(): Promise<DiagnosticReport> {
  return inDesktop() ? call("cancel_diagnostic") : browserDiagnostic("cancelled");
}
export async function handoffDiagnostic(): Promise<DiagnosticReport> {
  return inDesktop() ? call("handoff_diagnostic") : browserDiagnostic("cancelled");
}
export async function startDiagnostic(attemptId: string): Promise<boolean> {
  if (inDesktop()) return call("start_diagnostic", { attemptId });
  throw new Error("Prueba bloqueada: recuperación independiente sin validar.");
}

export async function loadConfig() { return inDesktop() ? call<ProfileConfig>("load_config") : readDevConfig(); }

export async function getCapabilities(): Promise<Capabilities> {
  if (inDesktop()) return call("get_capabilities");
  const preference = readDevConfig().backendPreference;
  const unavailableDriver = preference === "nvidia" || preference === "amd";
  return {
    engineName: unavailableDriver ? `${preference.toUpperCase()} (simulación no disponible)` : "Simulación de desarrollo (navegador)",
    selectedBackend: preference === "automatic" ? "mock" : preference,
    backendPreference: preference,
    isSimulation: true,
    available: !unavailableDriver,
    supportedControls: unavailableDriver ? [] : ["brightness", "contrast", "saturation", "hue"],
    unsupportedControls: unavailableDriver ? ["brightness", "contrast", "saturation", "hue", "gamma", "sharpness"] : ["gamma", "sharpness"],
    perMonitor: false,
    canRestore: !unavailableDriver,
    detectedAdapters: [],
    limitation: unavailableDriver ? "El navegador no detecta ni controla drivers. Use la aplicación nativa para consultar capacidades reales." : "El navegador solo simula. No modifica la pantalla.",
    activationBlockedReason: null,
  };
}

export async function setBackendPreference(preference: BackendPreference) {
  if (inDesktop()) return call<ProfileConfig>("set_backend_preference", { preference });
  const config = readDevConfig();
  config.backendPreference = preference;
  return writeDevConfig(config);
}

export async function getEngineStatus(): Promise<EngineStatus> {
  if (inDesktop()) return call("get_engine_status");
  return { enabled: devEnabled, recoveryRequired: false, description: devEnabled ? "Simulación activa; pantalla sin cambios" : "Simulación de desarrollo apagada" };
}

export async function setFiltersEnabled(enabled: boolean): Promise<EngineStatus> {
  if (inDesktop()) return call("set_filters_enabled", { enabled });
  devEnabled = enabled;
  return getEngineStatus();
}

export async function restoreColors(): Promise<EngineStatus> {
  if (inDesktop()) return call("restore_colors");
  devEnabled = false;
  return getEngineStatus();
}

export async function selectProfile(profileId: string) {
  if (inDesktop()) return call<ProfileConfig>("select_profile", { profileId });
  const config = readDevConfig(); config.selectedProfileId = profileId; return writeDevConfig(config);
}

export async function saveProfileSettings(profileId: string, settings: Settings) {
  if (inDesktop()) return call<ProfileConfig>("save_profile_settings", { profileId, settings });
  const config = readDevConfig();
  const profile = config.profiles.find((item) => item.id === profileId);
  if (!profile) throw new Error("El perfil no existe.");
  if (profile.builtin) throw new Error("Duplica un preset para editar sus ajustes.");
  profile.settings = settings;
  return writeDevConfig(config);
}

export async function createProfile(name: string) {
  if (inDesktop()) return call<ProfileConfig>("create_profile", { name });
  const config = readDevConfig();
  const source = config.profiles.find((item) => item.id === config.selectedProfileId);
  if (!source) throw new Error("El perfil seleccionado no existe.");
  const profile: Profile = { id: crypto.randomUUID(), name: name.trim(), builtin: false, settings: { ...source.settings } };
  config.profiles.push(profile); config.selectedProfileId = profile.id; return writeDevConfig(config);
}

export async function duplicateProfile(sourceId: string, name: string) {
  if (inDesktop()) return call<ProfileConfig>("duplicate_profile", { sourceId, name });
  const config = readDevConfig();
  const source = config.profiles.find((item) => item.id === sourceId);
  if (!source) throw new Error("El perfil no existe.");
  const profile: Profile = { ...source, id: crypto.randomUUID(), name: name.trim(), builtin: false, settings: { ...source.settings } };
  config.profiles.push(profile); config.selectedProfileId = profile.id; return writeDevConfig(config);
}

export async function renameProfile(profileId: string, name: string) {
  if (inDesktop()) return call<ProfileConfig>("rename_profile", { profileId, name });
  const config = readDevConfig(); const profile = config.profiles.find((item) => item.id === profileId);
  if (!profile || profile.builtin) throw new Error("Duplica un preset para renombrarlo.");
  profile.name = name.trim(); return writeDevConfig(config);
}

export async function deleteProfile(profileId: string) {
  if (inDesktop()) return call<ProfileConfig>("delete_profile", { profileId });
  const config = readDevConfig(); const profile = config.profiles.find((item) => item.id === profileId);
  if (!profile || profile.builtin) throw new Error("Los presets originales no se pueden eliminar.");
  config.profiles = config.profiles.filter((item) => item.id !== profileId);
  if (config.selectedProfileId === profileId) config.selectedProfileId = config.profiles[0].id;
  return writeDevConfig(config);
}

export async function exportProfiles() {
  if (inDesktop()) return call<string>("export_profiles");
  const config = readDevConfig();
  return JSON.stringify({ schemaVersion: config.schemaVersion, profiles: config.profiles }, null, 2);
}

export async function importProfiles(json: string) {
  if (inDesktop()) return call<ProfileConfig>("import_profiles", { json });
  const incoming = parseProfileExport(json);
  const config = readDevConfig();
  const profiles = incoming.profiles.map((profile) => ({ ...profile, id: crypto.randomUUID(), builtin: false }));
  if (config.profiles.length + profiles.length > 128) throw new Error("Límite de perfiles alcanzado.");
  config.profiles.push(...profiles); return writeDevConfig(config);
}
