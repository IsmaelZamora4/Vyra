# VYRA

VYRA es una aplicación de escritorio para Windows x64 que permite gestionar perfiles locales de ajustes visuales y consultar información de compatibilidad y hardware.

> **Beta experimental:** esta versión permite gestionar perfiles y previsualizar ajustes simulados. Todavía no modifica la saturación, el brillo ni el contraste reales del escritorio. Los filtros reales están deshabilitados.

## Funciones

- Crear, editar, duplicar, importar y exportar perfiles locales.
- Guardar perfiles entre sesiones.
- Consultar inventario de hardware y notas de compatibilidad.
- Explorar controles simulados sin cambiar los colores de Windows.

No se ha validado compatibilidad con videojuegos ni anticheats. NVIDIA y AMD no tienen controles de escritura habilitados en esta versión.

## Requisitos

- Windows 10 u 11, x64.
- Microsoft Edge WebView2 Runtime, si no está disponible en el equipo.

El instalador no está firmado digitalmente. Windows puede mostrar una advertencia del publicador desconocido.

## Descargar e instalar

1. Descarga **VYRA Phase 10 Validation** desde [Releases](https://github.com/IsmaelZamora4/Vyra/releases).
2. En PowerShell, verifica el instalador descargado:

   ```powershell
   Get-FileHash ".\VYRA Phase 10 Validation_0.1.0-alpha_x64-setup.exe" -Algorithm SHA256
   ```

   SHA-256 esperado:

   ```text
   C5A20D616E8A1B2536DBC6CFEA802DB4DFFE60ECBABCA08A6987B01D21F2DA4B
   ```

3. Continúa solo si el hash coincide. Elige instalación por usuario si está disponible; cancela si se solicita elevación.
4. Abre VYRA. Los cambios de controles se guardan en perfiles y son simulados; no inicies diagnósticos de color.

## Desarrollo

Requisitos: Node.js 20.19+ o 22.12+, npm, Rust/Cargo estable, Microsoft C++ Build Tools y WebView2 Runtime.

```powershell
npm install
npm run tauri dev
```

Validación frontend:

```powershell
npm run build
npm test
npm run lint
```

Compilación de escritorio:

```powershell
npm run tauri build
```

## Estado y limitaciones

- `REAL_DIAGNOSTIC_ENABLED=false`; la escritura real de color permanece bloqueada.
- No se ha validado la restauración visual real mediante Windows Magnification API.
- No se ha validado comportamiento con HDR, múltiples monitores ni pantalla completa.
- El proceso de desinstalación y la conservación de perfiles requieren revisión manual.
- No se afirma compatibilidad con anticheats.

## Licencia

MIT. Consulta [LICENSE](LICENSE).
