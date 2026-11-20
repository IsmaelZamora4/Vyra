export type Settings = {
  brightness: number;
  contrast: number;
  saturation: number;
  gamma: number;
  hue: number;
  sharpness: number;
};

export type Profile = {
  id: string;
  name: string;
  builtin: boolean;
  settings: Settings;
};

export type ProfileConfig = {
  schemaVersion: number;
  selectedProfileId: string;
  backendPreference: BackendPreference;
  profiles: Profile[];
};

export type BackendPreference = "automatic" | "nvidia" | "amd" | "windows";

export type DisplayAdapter = {
  vendor: string;
  name: string;
  deviceName: string;
  active: boolean;
  monitors: { name: string; deviceName: string; active: boolean }[];
};

export type ProfileExport = { schemaVersion: number; profiles: Profile[] };

export function parseProfileExport(json: string): ProfileExport {
  let value: unknown;
  try { value = JSON.parse(json); } catch { throw new Error("JSON inválido."); }
  if (!value || typeof value !== "object") throw new Error("Archivo JSON VYRA no compatible.");
  const data = value as Partial<ProfileExport>;
  if (data.schemaVersion !== 1 || !Array.isArray(data.profiles) || data.profiles.length > 128) throw new Error("Versión o cantidad de perfiles no compatible.");
  for (const profile of data.profiles) {
    if (!profile || typeof profile !== "object" || typeof profile.name !== "string" || !profile.name.trim() || profile.name.trim().length > 40) throw new Error("Nombre de perfil inválido.");
    const values = profile.settings;
    if (!values || !Number.isFinite(values.brightness) || values.brightness < -100 || values.brightness > 100
      || !Number.isFinite(values.contrast) || values.contrast < -100 || values.contrast > 100
      || !Number.isFinite(values.saturation) || values.saturation < -100 || values.saturation > 100
      || !Number.isFinite(values.hue) || values.hue < -180 || values.hue > 180
      || !Number.isFinite(values.gamma) || values.gamma < 0.5 || values.gamma > 2
      || !Number.isInteger(values.sharpness) || values.sharpness < 0 || values.sharpness > 100) throw new Error("Ajustes de perfil fuera de rango.");
  }
  return data as ProfileExport;
}

export type Capabilities = {
  engineName: string;
  selectedBackend: string;
  backendPreference: BackendPreference;
  isSimulation: boolean;
  available: boolean;
  supportedControls: string[];
  unsupportedControls: string[];
  perMonitor: boolean;
  canRestore: boolean;
  detectedAdapters: DisplayAdapter[];
  limitation: string;
  activationBlockedReason: string | null;
};

export type DiagnosticReport = {
  attemptId: string | null;
  stage: "notStarted" | "blocked" | "preflightPending" | "awaitingConfirmation" | "armed" | "applying" | "applied" | "cancelled" | "restored" | "failed" | "expired" | "outcomeUncertain" | "auditIncomplete";
  phase: "notStarted" | "preflightPending" | "preflightAccepted" | "confirmationAvailable" | "confirming" | "consentIncidentLockdown" | "sessionExpired" | "recoveryPending" | "externalConflict" | "watchdogError" | "applying" | "applied" | "outcomeUncertain" | "auditPersistenceFailed" | "cancelled" | "restored" | "failed";
  blockerCode: "backendMismatch" | "filtersActive" | "recoveryPending" | "windowsReadFailed" | "helperUnavailable" | "preflightExpired" | "externalConflict" | "watchdogError" | "confirmationDeclined" | "nativeConfirmationFailed" | "sessionExpired" | "consentIncidentLockdown" | "applyOutcomeUncertain" | "auditPersistenceFailed" | null;
  maxDurationSeconds: number;
  maxPreflightWaitSeconds: number;
  saturation: number;
  independentRecoveryReady: boolean;
  currentStateReadAccepted: boolean | null;
  confirmationAccepted: boolean;
  applicationAccepted: boolean | null;
  applicationVerified: boolean | null;
  restorationAccepted: boolean | null;
  canRequestConfirmation: boolean;
  reason: string;
  timestampMs: number;
  watchdogState: string;
  helperVerified: boolean;
  helperPid: number | null;
  helperInitializationAccepted: boolean | null;
  helperStateReadAccepted: boolean | null;
  helperJournalDurable: boolean | null;
  helperSameWindowsSession: boolean | null;
  restorationVerified: boolean | null;
};

export type EngineStatus = { enabled: boolean; recoveryRequired: boolean; description: string };

export const defaultSettings: Settings = {
  brightness: 0, contrast: 0, saturation: 0, gamma: 1, hue: 0, sharpness: 0,
};

export const defaults: ProfileConfig = {
  schemaVersion: 1,
  selectedProfileId: "builtin-competitive",
  backendPreference: "automatic",
  profiles: [
    { id: "builtin-competitive", name: "Competitivo", builtin: true, settings: { ...defaultSettings } },
    { id: "builtin-cinematic", name: "Cinematográfico", builtin: true, settings: { ...defaultSettings, contrast: 8, saturation: 6 } },
    { id: "builtin-vibrant", name: "Vibrante", builtin: true, settings: { ...defaultSettings, saturation: 24 } },
    { id: "builtin-night", name: "Nocturno", builtin: true, settings: { ...defaultSettings, brightness: -10, contrast: 12 } },
  ],
};
