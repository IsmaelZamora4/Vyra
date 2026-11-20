import { BookOpen, Database, Shield } from "lucide-react";
import type { BackendPreference, Capabilities, EngineStatus, ProfileConfig } from "../../types/app";

const controlNames: Record<string, string> = {
  brightness: "Brillo", contrast: "Contraste", saturation: "Saturación", hue: "Tono", gamma: "Gamma", sharpness: "Nitidez",
};

export function SettingsView({ config, capabilities, status, onBackendChange }: {
  config: ProfileConfig;
  capabilities: Capabilities;
  status: EngineStatus;
  onBackendChange: (preference: BackendPreference) => void;
}) {
  return <>
    <div className="page-intro"><span className="eyebrow">APLICACIÓN</span><h2>Configuración</h2><p>Motor de color, almacenamiento y rangos vigentes.</p></div>
    <section className="settings-group">
      <div className="settings-icon"><Shield size={18} /></div>
      <div>
        <strong>Backend de color</strong>
        <label className="backend-label" htmlFor="backend-preference">Seleccionar backend</label>
        <select id="backend-preference" className="backend-select" value={config.backendPreference} disabled={status.enabled || status.recoveryRequired} onChange={(event) => onBackendChange(event.target.value as BackendPreference)}>
          <option value="automatic">Automático</option>
          <option value="nvidia">NVIDIA NVAPI</option>
          <option value="amd">AMD ADLX</option>
          <option value="windows">Windows experimental</option>
        </select>
        <p>En uso: <b>{capabilities.engineName}</b>{capabilities.available ? " · disponible" : " · no disponible"}</p>
        <p>Alcance: {capabilities.perMonitor ? "por monitor" : "global"}</p>
        <p>Restauración: {capabilities.canRestore ? "disponible" : "no disponible"}</p>
        <p>Controles activos: {capabilities.supportedControls.length ? capabilities.supportedControls.map((control) => controlNames[control] ?? control).join(", ") : "ninguno"}.</p>
        {capabilities.detectedAdapters.length > 0 ? <div className="detected-adapters"><p>Adaptadores de pantalla detectados:</p><ul>{capabilities.detectedAdapters.map((adapter) => <li key={adapter.deviceName}>
          <b>{adapter.vendor}</b> — {adapter.name}{adapter.active ? " · activo" : ""}
          {adapter.monitors.map((monitor) => <span key={monitor.deviceName}>{monitor.name || monitor.deviceName}{monitor.active ? " · activo" : ""}</span>)}
        </li>)}</ul></div> : <p>No se detectaron adaptadores de pantalla en la sesión.</p>}
        <p className="backend-limitation">{capabilities.limitation}</p>
        {status.recoveryRequired && <p>Restaura el estado guardado antes de activar filtros o cambiar backend.</p>}
        {status.enabled && <p>Desactiva el motor antes de cambiar backend.</p>}
      </div>
      <span className={`tag ${capabilities.available ? "tag-active" : ""}`}>{capabilities.available ? "Disponible" : "No disponible"}</span>
    </section>
    <section className="settings-group"><div className="settings-icon"><Database size={18} /></div><div><strong>Almacenamiento local</strong><p>Perfiles JSON versionados en los datos de aplicación de Windows. Sin cuentas, telemetría ni sincronización.</p></div><span className="tag">Dispositivo</span></section>
    <section className="settings-group"><div className="settings-icon"><Shield size={18} /></div><div><strong>Privacidad</strong><p>Sin acceso a procesos de juegos. Sin cambios en registro, drivers ni archivos de juegos.</p></div><span className="tag">Local</span></section>
    <section className="settings-group range-docs"><div className="settings-icon"><BookOpen size={18} /></div><div><strong>Rangos y fórmulas</strong><p>Brillo −100…100: desplazamiento de canales, brillo / 100.</p><p>Contraste −100…100: escala 1 + contraste / 100 alrededor de 0.5.</p><p>Saturación −100…100: mezcla lineal con luminancia perceptual.</p><p>Tono −180°…180°: rotación de tono en matriz lineal.</p><p>Gamma 0.5…2 y nitidez 0…100: reservados, deshabilitados; la matriz 5×5 no representa curva gamma ni filtros espaciales.</p></div></section>
  </>;
}
