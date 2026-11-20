import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it, vi } from "vitest";
import { Adjustments } from "./Adjustments";
import type { Capabilities, Profile } from "../../types/app";

const profile: Profile = {
  id: "custom",
  name: "Personalizado",
  builtin: false,
  settings: { brightness: 0, contrast: 0, saturation: 0, gamma: 1, hue: 0, sharpness: 0 },
};

function capabilities(activationBlockedReason: string | null): Capabilities {
  return {
    engineName: "Windows experimental",
    selectedBackend: "windows",
    backendPreference: "windows",
    isSimulation: false,
    available: true,
    supportedControls: ["brightness", "contrast", "saturation", "hue"],
    unsupportedControls: ["gamma", "sharpness"],
    perMonitor: false,
    canRestore: true,
    detectedAdapters: [],
    limitation: "Experimental",
    activationBlockedReason,
  };
}

function renderAdjustments(value: Capabilities) {
  const environment = globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean };
  environment.IS_REACT_ACT_ENVIRONMENT = true;
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  const onToggle = vi.fn();
  const onSetting = vi.fn();
  act(() => root.render(
    <Adjustments
      profile={profile}
      profiles={[profile]}
      capabilities={value}
      status={{ enabled: false, recoveryRequired: false, description: "Filtros apagados" }}
      onSetting={onSetting}
      onToggle={onToggle}
      onRestore={vi.fn()}
      onSelect={vi.fn()}
      onDuplicate={vi.fn()}
    />,
  ));
  return { container, onToggle, onSetting, dispose: () => { act(() => root.unmount()); container.remove(); } };
}

describe("Simulated profile adjustments", () => {
  it("allows editing custom profile values while real activation stays blocked", () => {
    const view = renderAdjustments(capabilities("Recuperación independiente no validada."));
    for (const name of ["brightness", "contrast", "saturation", "hue"]) {
      expect(view.container.querySelector(`#control-${name}`)).toHaveProperty("disabled", false);
    }
    expect(view.container.querySelector('[aria-label="Activar filtros de color"]')).toHaveProperty("disabled", true);
    expect(view.container.textContent).toContain("solo se guardan en el perfil");
    view.container.querySelector<HTMLElement>('[aria-label="Activar filtros de color"]')?.click();
    expect(view.onToggle).not.toHaveBeenCalled();
    expect(view.container.querySelector("#control-gamma")).toHaveProperty("disabled", true);
    view.dispose();
  });

  it("keeps supported profile controls editable when activation is allowed", () => {
    const view = renderAdjustments(capabilities(null));
    for (const name of ["brightness", "contrast", "saturation", "hue"]) {
      expect(view.container.querySelector(`#control-${name}`)).toHaveProperty("disabled", false);
    }
    expect(view.container.querySelector("#control-gamma")).toHaveProperty("disabled", true);
    view.dispose();
  });
});
