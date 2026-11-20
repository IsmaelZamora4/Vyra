import { useEffect, useRef, useState } from "react";
import type { Profile } from "../../types/app";

export type ProfileDialogMode = "create" | "rename" | "duplicate";

export function ProfileNameDialog({ mode, profile, onClose, onSubmit }: {
  mode: ProfileDialogMode; profile?: Profile; onClose: () => void; onSubmit: (name: string) => void;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [name, setName] = useState(profile ? (mode === "rename" ? profile.name : `${profile.name} copia`) : "");
  const title = mode === "rename" ? "Renombrar perfil" : mode === "duplicate" ? "Duplicar perfil" : "Crear perfil";

  useEffect(() => {
    const element = dialog.current;
    if (element && !element.open) element.showModal();
    return () => { if (element?.open) element.close(); };
  }, []);

  return <dialog ref={dialog} className="profile-dialog" aria-labelledby="profile-dialog-title" onCancel={(event) => { event.preventDefault(); onClose(); }}>
    <form onSubmit={(event) => { event.preventDefault(); const trimmed = name.trim(); if (trimmed) onSubmit(trimmed); }}>
      <span className="eyebrow">PERFILES LOCALES</span>
      <h2 id="profile-dialog-title">{title}</h2>
      <label htmlFor="profile-name">Nombre</label>
      <input id="profile-name" autoFocus required minLength={1} maxLength={40} value={name} onChange={(event) => setName(event.target.value)} />
      <div className="dialog-actions"><button type="button" className="button button-quiet" onClick={onClose}>Cancelar</button><button type="submit" className="button button-primary">{mode === "rename" ? "Guardar" : mode === "duplicate" ? "Crear copia" : "Crear perfil"}</button></div>
    </form>
  </dialog>;
}
