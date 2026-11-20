import { Copy, Download, FileUp, MoreHorizontal, Pencil, Plus, Trash2 } from "lucide-react";
import type { Profile } from "../../types/app";

export function ProfilesView({ profiles, selected, onSelect, onCreate, onRename, onDuplicate, onDelete, onExport, onImport }: {
  profiles: Profile[]; selected: string; onSelect: (id: string) => void; onCreate: () => void; onRename: (profile: Profile) => void;
  onDuplicate: (profile: Profile) => void; onDelete: (profile: Profile) => void; onExport: () => void; onImport: (file: File) => void;
}) {
  return <>
    <div className="page-actions"><div><span className="eyebrow">BIBLIOTECA LOCAL</span><h2>Perfiles</h2><p>Presets originales protegidos. Duplica para personalizarlos.</p></div><div className="action-group">
      <button className="button button-quiet" onClick={onExport}><Download size={15} />Exportar JSON</button>
      <label className="button button-quiet import-button"><FileUp size={15} />Importar<input type="file" accept="application/json,.json" onChange={(event) => { const file = event.target.files?.[0]; if (file) onImport(file); event.currentTarget.value = ""; }} /></label>
      <button className="button button-primary" onClick={onCreate}><Plus size={15} />Nuevo perfil</button>
    </div></div>
    <div className="profile-list" role="list">
      {profiles.map((profile) => <article className={`profile-item ${profile.id === selected ? "selected" : ""}`} key={profile.id} role="listitem">
        <button className="profile-select" onClick={() => onSelect(profile.id)} aria-pressed={profile.id === selected}>
          <span className="profile-indicator" aria-hidden="true">{profile.id === selected ? "✓" : ""}</span>
          <span className="profile-name"><strong>{profile.name}</strong><small>{profile.builtin ? "Preset de VYRA" : "Personalizado"}</small></span>
          <span className="profile-values">B {profile.settings.brightness} <i /> C {profile.settings.contrast} <i /> S {profile.settings.saturation}</span>
        </button>
        <div className="profile-actions">
          {!profile.builtin && <button className="icon-button" aria-label={`Renombrar ${profile.name}`} onClick={() => onRename(profile)}><Pencil size={15} /></button>}
          <button className="icon-button" aria-label={`Duplicar ${profile.name}`} onClick={() => onDuplicate(profile)}><Copy size={15} /></button>
          {!profile.builtin && <button className="icon-button danger" aria-label={`Eliminar ${profile.name}`} onClick={() => onDelete(profile)}><Trash2 size={15} /></button>}
        </div>
      </article>)}
    </div>
    <div className="profile-footnote"><MoreHorizontal size={15} />Perfiles compatibles entre instalaciones mediante JSON versionado.</div>
  </>;
}
