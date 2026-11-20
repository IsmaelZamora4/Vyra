import { Copy, RotateCcw, SlidersHorizontal, WandSparkles } from "lucide-react";
import type { Capabilities, EngineStatus, Profile, Settings } from "../../types/app";
import { DiagnosticPanel } from "./DiagnosticPanel";

type Control = { key: keyof Settings; label: string; min: number; max: number; step: number; value: number; unit?: string; description: string };

const controls = (settings: Settings): Control[] => [
  { key: "brightness", label: "Brillo", min: -100, max: 100, step: 1, value: settings.brightness, unit: "%", description: "Desplazamiento uniforme de los canales." },
  { key: "contrast", label: "Contraste", min: -100, max: 100, step: 1, value: settings.contrast, unit: "%", description: "Escala lineal alrededor del punto medio." },
  { key: "saturation", label: "Saturación", min: -100, max: 100, step: 1, value: settings.saturation, unit: "%", description: "Mezcla de luminancia y color original." },
  { key: "hue", label: "Tono", min: -180, max: 180, step: 1, value: settings.hue, unit: "°", description: "Rotación de color calculada en matriz 3×3." },
  { key: "gamma", label: "Gamma", min: 0.5, max: 2, step: 0.01, value: settings.gamma, description: "Pendiente: requiere una curva no lineal." },
  { key: "sharpness", label: "Nitidez", min: 0, max: 100, step: 1, value: settings.sharpness, unit: "%", description: "Pendiente: requiere procesamiento espacial." },
];

export function Adjustments({ profile, profiles, capabilities, status, onSetting, onToggle, onRestore, onSelect, onDuplicate }: {
  profile: Profile; profiles: Profile[]; capabilities: Capabilities; status: EngineStatus; onSetting: (key: keyof Settings, value: number) => void;
  onToggle: () => void; onRestore: () => void; onSelect: (id: string) => void; onDuplicate: (profile: Profile) => void;
}) {
  const simulatedOnly = capabilities.isSimulation || !capabilities.available || !!capabilities.activationBlockedReason;
  return <>
    <section className="engine-bar" aria-label="Control del motor">
      <div className="engine-summary"><span className={`engine-light ${status.enabled ? "on" : ""}`} /><div><strong>{status.recoveryRequired ? "Restauración pendiente" : status.enabled ? "Filtros activos" : "Filtros apagados"}</strong><small>{capabilities.engineName}</small></div></div>
      <div className="engine-actions">
        <label className={`switch ${!capabilities.available || status.recoveryRequired || capabilities.activationBlockedReason ? "disabled" : ""}`}>
          <input type="checkbox" role="switch" aria-label="Activar filtros de color" checked={status.enabled} disabled={status.recoveryRequired || (!status.enabled && (!capabilities.available || !!capabilities.activationBlockedReason))} onChange={onToggle} />
          <span className="switch-track" aria-hidden="true" />
        </label>
        <button className="button button-quiet" onClick={onRestore} disabled={!status.enabled && !status.recoveryRequired}><RotateCcw size={15} />Restaurar</button>
      </div>
    </section>
    {capabilities.activationBlockedReason && <p className="notice" role="status">{capabilities.activationBlockedReason}</p>}
    <DiagnosticPanel status={status} onRestore={onRestore} />
    <section className="profile-strip">
      <div><span className="eyebrow">PERFIL ACTIVO</span><strong>{profile.name}</strong></div>
      <select aria-label="Seleccionar perfil" value={profile.id} onChange={(event) => onSelect(event.target.value)}>
        {profiles.map((item) => <option value={item.id} key={item.id}>{item.name}</option>)}
      </select>
      <span className="profile-hint"><WandSparkles size={14} />{profile.builtin ? "Preset protegido" : "Guardado automático"}</span>
    </section>
    <section className="adjustment-section">
      <header className="section-heading"><div><span className="eyebrow">IMAGEN</span><h2>Ajustes de color</h2><p>{simulatedOnly ? "Los ajustes solo se guardan en el perfil; no modifican el escritorio." : "Estado neutro conserva la transformación identidad."}</p></div>{profile.builtin ? <button className="button button-quiet" onClick={() => onDuplicate(profile)}><Copy size={14} />Duplicar para editar</button> : <SlidersHorizontal size={18} />}</header>
      <div className="control-list">
        {controls(profile.settings).map((control) => {
          const supported = capabilities.supportedControls.includes(control.key);
          const editable = supported && !profile.builtin;
          const disabledReason = !supported
            ? "No compatible con el motor activo"
            : profile.builtin
              ? "Duplica este preset para editar"
              : control.description;
          return <div className={`control-row ${!editable ? "unsupported" : ""}`} key={control.key}>
            <div className="control-label"><label htmlFor={`control-${control.key}`}>{control.label}</label><small>{disabledReason}</small></div>
            <input id={`control-${control.key}`} type="range" min={control.min} max={control.max} step={control.step} value={control.value} disabled={!editable} onChange={(event) => onSetting(control.key, Number(event.target.value))} aria-valuetext={`${control.value}${control.unit ?? ""}`} />
            <output htmlFor={`control-${control.key}`} className="control-value">{control.value}{control.unit}</output>
          </div>;
        })}
      </div>
    </section>
    <p className="notice"><span aria-hidden="true">ⓘ</span> {simulatedOnly ? "Los ajustes se guardan en este dispositivo como simulación; no se cambia el color de Windows." : "Los ajustes se guardan en este dispositivo. El motor solo actúa al activar filtros."}</p>
  </>;
}
