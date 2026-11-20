import { Activity, CircleUserRound, SlidersHorizontal, Settings2, ShieldCheck, type LucideIcon } from "lucide-react";

export type Page = "adjustments" | "profiles" | "compatibility" | "settings";

const navigation: { id: Page; label: string; icon: LucideIcon }[] = [
  { id: "adjustments", label: "Ajustes", icon: SlidersHorizontal },
  { id: "profiles", label: "Perfiles", icon: CircleUserRound },
  { id: "compatibility", label: "Compatibilidad", icon: ShieldCheck },
  { id: "settings", label: "Configuración", icon: Settings2 },
];

export function Sidebar({ page, onChange }: { page: Page; onChange: (page: Page) => void }) {
  return <aside className="sidebar">
    <div className="brand"><div className="brand-mark" aria-hidden="true"><img src="/vyra.png" alt="" /></div><div><strong>VYRA</strong><span>COLOR CONTROL</span></div></div>
    <nav aria-label="Navegación principal">
      {navigation.map(({ id, label, icon: Icon }) => <button key={id} className={`nav-item ${page === id ? "active" : ""}`} onClick={() => onChange(id)} aria-label={label} title={label} aria-current={page === id ? "page" : undefined}>
        <Icon size={17} strokeWidth={1.8} /><span>{label}</span>{id === "adjustments" && <Activity className="nav-dot" size={13} />}
      </button>)}
    </nav>
    <div className="sidebar-bottom"><div className="sidebar-status"><span className="status-pip" />Versión 0.1.0-alpha</div><span className="repo-placeholder">Repositorio por configurar</span></div>
  </aside>;
}
