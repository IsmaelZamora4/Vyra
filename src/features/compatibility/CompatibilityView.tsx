import { Check, CircleHelp, Monitor, X } from "lucide-react";
import type { Capabilities, EngineStatus } from "../../types/app";

const names: Record<string, string> = { brightness: "Brillo", contrast: "Contraste", saturation: "Saturación", hue: "Tono", gamma: "Gamma", sharpness: "Nitidez" };

export function CompatibilityView({ capabilities, status }: { capabilities: Capabilities; status: EngineStatus }) {
  return <>
    <div className="page-intro"><span className="eyebrow">DIAGNÓSTICO</span><h2>Compatibilidad</h2><p>Capacidades informadas por el motor. La compatibilidad de juegos no se verifica.</p></div>
    <section className="compat-engine"><div className="compat-icon"><Monitor size={20} /></div><div className="compat-details"><strong>{capabilities.engineName}</strong><small>{capabilities.available ? "Disponible en este sistema" : "No disponible en esta plataforma"}</small></div><span className={`tag ${status.enabled ? "tag-active" : ""}`}>{status.description}</span></section>
    <section className="capability-panel"><h3>Controles</h3><div className="capability-list">
      {[...capabilities.supportedControls, ...capabilities.unsupportedControls].map((control) => {
        const supported = capabilities.supportedControls.includes(control); const Icon = supported ? Check : X;
        return <div className="capability-row" key={control}><span className={`capability-icon ${supported ? "yes" : "no"}`}><Icon size={14} /></span><span>{names[control] ?? control}</span><small>{supported ? "Matriz de color" : "No implementado"}</small></div>;
      })}
    </div></section>
    <section className="limitation"><CircleHelp size={17} /><div><strong>Limitaciones conocidas</strong><p>{capabilities.limitation}</p><p>La API aplica una transformación global del escritorio. Pantalla completa exclusiva, HDR y varias pantallas requieren verificación en cada equipo.</p><p>Sin garantías sobre anticheats. VYRA no accede a procesos de juegos ni altera archivos.</p></div></section>
  </>;
}
