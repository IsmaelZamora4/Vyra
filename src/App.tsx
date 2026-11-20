import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertCircle, ChevronRight } from "lucide-react";
import { Sidebar, type Page } from "./components/Sidebar";
import { Adjustments } from "./features/adjustments/Adjustments";
import { CompatibilityView } from "./features/compatibility/CompatibilityView";
import { ProfilesView } from "./features/profiles/ProfilesView";
import { ProfileNameDialog, type ProfileDialogMode } from "./features/profiles/ProfileNameDialog";
import { SettingsView } from "./features/settings/SettingsView";
import { createProfile, deleteProfile, duplicateProfile, exportProfiles, getCapabilities, getEngineStatus, importProfiles, loadConfig, renameProfile, restoreColors, saveProfileSettings, selectProfile, setBackendPreference, setFiltersEnabled } from "./lib/vyra";
import type { BackendPreference, Capabilities, EngineStatus, Profile, ProfileConfig, Settings } from "./types/app";
import "./App.css";

const pageTitles: Record<Page, string> = { adjustments: "Ajustes", profiles: "Perfiles", compatibility: "Compatibilidad", settings: "Configuración" };

function App() {
  const [page, setPage] = useState<Page>("adjustments");
  const [config, setConfig] = useState<ProfileConfig | null>(null);
  const [capabilities, setCapabilities] = useState<Capabilities | null>(null);
  const [status, setStatus] = useState<EngineStatus>({ enabled: false, recoveryRequired: false, description: "Cargando" });
  const [error, setError] = useState("");
  const [loading, setLoading] = useState(true);
  const [profileDialog, setProfileDialog] = useState<{ mode: ProfileDialogMode; profile?: Profile } | null>(null);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const pendingSave = useRef<{ profileId: string; settings: Settings } | null>(null);
  const saveQueue = useRef<Promise<void>>(Promise.resolve());

  const reportError = useCallback(async (reason: unknown) => {
    setError(message(reason));
    try { setStatus(await getEngineStatus()); } catch { /* Keep last known status when engine is unavailable. */ }
  }, []);

  const flushSave = useCallback(() => {
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = undefined;
    const pending = pendingSave.current;
    pendingSave.current = null;
    if (!pending) return;
    saveQueue.current = saveQueue.current.catch(() => undefined)
      .then(() => saveProfileSettings(pending.profileId, pending.settings))
      .then(setConfig)
      .catch((reason: unknown) => { void reportError(reason); });
  }, [reportError]);

  const initialize = useCallback(async () => {
    try {
      const [stored, available, engine] = await Promise.all([loadConfig(), getCapabilities(), getEngineStatus()]);
      setConfig(stored); setCapabilities(available); setStatus(engine);
      setError("");
    } catch (reason) { await reportError(reason); }
    finally { setLoading(false); }
  }, [reportError]);

  useEffect(() => { void Promise.resolve().then(initialize); }, [initialize]);

  useEffect(() => {
    const timer = setInterval(() => {
      void getEngineStatus().then(setStatus).catch(() => { /* Preserve last state on IPC failure. */ });
    }, 1000);
    return () => clearInterval(timer);
  }, []);

  useEffect(() => () => flushSave(), [flushSave]);

  const profile = useMemo(() => config?.profiles.find((item) => item.id === config.selectedProfileId) ?? config?.profiles[0], [config]);

  async function updateConfig(operation: () => Promise<ProfileConfig>) {
    setError("");
    flushSave();
    await saveQueue.current;
    try { setConfig(await operation()); } catch (reason) { await reportError(reason); }
  }

  function updateSetting(key: keyof Settings, value: number) {
    if (!config || !profile) return;
    const settings = { ...profile.settings, [key]: value };
    const next = { ...config, profiles: config.profiles.map((item) => item.id === profile.id ? { ...item, settings } : item) };
    setConfig(next);
    pendingSave.current = { profileId: profile.id, settings };
    if (saveTimer.current) clearTimeout(saveTimer.current);
    saveTimer.current = setTimeout(flushSave, 180);
  }

  async function toggleFilters() {
    setError("");
    try { setStatus(await setFiltersEnabled(!status.enabled)); } catch (reason) { await reportError(reason); }
  }

  async function restore() {
    setError("");
    try { setStatus(await restoreColors()); } catch (reason) { await reportError(reason); }
  }

  async function changeBackend(preference: BackendPreference) {
    await updateConfig(() => setBackendPreference(preference));
    try {
      setCapabilities(await getCapabilities());
      setStatus(await getEngineStatus());
    } catch (reason) { await reportError(reason); }
  }

  const activeProfiles = config?.profiles ?? [];

  async function submitProfileName(name: string) {
    if (!profileDialog) return;
    const dialog = profileDialog;
    const selected = dialog.profile;
    if (dialog.mode === "create") await updateConfig(() => createProfile(name));
    if (dialog.mode === "rename" && selected) await updateConfig(() => renameProfile(selected.id, name));
    if (dialog.mode === "duplicate" && selected) await updateConfig(() => duplicateProfile(selected.id, name));
    setProfileDialog(null);
  }

  return <div className="app-shell">
    <Sidebar page={page} onChange={setPage} />
    <main className="workspace">
      <header className="topbar"><div className="breadcrumb">VYRA <ChevronRight size={13} /> <span>{pageTitles[page]}</span></div><div className="topbar-status"><span className={`status-pip ${status.enabled ? "live" : ""}`} />{status.description}</div></header>
      <div className="page-content">
        <div className="page-heading"><div><span className="eyebrow">VYRA / ESCRITORIO</span><h1>{pageTitles[page]}</h1></div></div>
        {error && config && capabilities && <div className="error-banner" role="alert"><AlertCircle size={16} /><span>{error}</span><button onClick={() => setError("")} aria-label="Cerrar error">×</button></div>}
        {loading ? <div className="loading-state" role="status">Cargando configuración local…</div> : !config || !capabilities || !profile ? <div className="load-failure" role="alert"><p>No se pudo cargar VYRA. {error || "Estado local no disponible."}</p><button className="button button-quiet" onClick={() => void initialize()}>Reintentar</button></div> : <>
          {page === "adjustments" && <Adjustments profile={profile} profiles={activeProfiles} capabilities={capabilities} status={status} onSetting={updateSetting} onToggle={() => void toggleFilters()} onRestore={() => void restore()} onSelect={(id) => void updateConfig(() => selectProfile(id))}
            onDuplicate={(item) => setProfileDialog({ mode: "duplicate", profile: item })} />}
          {page === "profiles" && <ProfilesView profiles={activeProfiles} selected={profile.id} onSelect={(id) => void updateConfig(() => selectProfile(id))}
            onCreate={() => setProfileDialog({ mode: "create" })}
            onRename={(item) => setProfileDialog({ mode: "rename", profile: item })}
            onDuplicate={(item) => setProfileDialog({ mode: "duplicate", profile: item })}
            onDelete={async (item) => { if (window.confirm(`¿Eliminar el perfil “${item.name}”?`)) await updateConfig(() => deleteProfile(item.id)); }}
            onExport={async () => { flushSave(); await saveQueue.current; try { const json = await exportProfiles(); download(json); } catch (reason) { await reportError(reason); } }}
            onImport={async (file) => { try { const json = await file.text(); await updateConfig(() => importProfiles(json)); } catch (reason) { setError(message(reason)); } }} />}
          {page === "compatibility" && <CompatibilityView capabilities={capabilities} status={status} />}
          {page === "settings" && <SettingsView config={config} capabilities={capabilities} status={status} onBackendChange={changeBackend} />}
        </>}
        {profileDialog && <ProfileNameDialog key={`${profileDialog.mode}-${profileDialog.profile?.id ?? "new"}`} mode={profileDialog.mode} profile={profileDialog.profile} onClose={() => setProfileDialog(null)} onSubmit={(name) => void submitProfileName(name)} />}
      </div>
    </main>
  </div>;
}

function message(error: unknown) { return error instanceof Error ? error.message : String(error); }

function download(json: string) {
  const url = URL.createObjectURL(new Blob([json], { type: "application/json" }));
  const anchor = document.createElement("a"); anchor.href = url; anchor.download = "vyra-profiles.json"; anchor.click(); URL.revokeObjectURL(url);
}

export default App;
