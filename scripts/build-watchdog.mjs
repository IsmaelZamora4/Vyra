import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
if (process.platform !== "win32") throw new Error("El watchdog de esta fase requiere Windows MSVC.");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1].trim();
const target = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (target !== "x86_64-pc-windows-msvc") throw new Error(`Target no validado: ${target}`);
if (!existsSync(resolve(root, "dist/index.html"))) throw new Error("Ejecuta npm run build antes de compilar el helper.");

// Bootstrap only: tauri-build cannot copy an externalBin that has not been built yet.
// The parent environment is unchanged; the real Tauri build uses the full externalBin config.
const config = process.env.TAURI_CONFIG ? JSON.parse(process.env.TAURI_CONFIG) : {};
config.bundle = { ...config.bundle, externalBin: [] };
execFileSync("cargo", ["build", "--release", "--locked", "--manifest-path", "src-tauri/Cargo.toml", "--features", "watchdog-bin", "--bin", "vyra-watchdog"], {
  cwd: root, stdio: "inherit", env: { ...process.env, TAURI_CONFIG: JSON.stringify(config) },
});
const cargoTarget = resolve(root, process.env.CARGO_TARGET_DIR || "src-tauri/target");
const output = resolve(root, "src-tauri/binaries");
mkdirSync(output, { recursive: true });
copyFileSync(resolve(cargoTarget, "release/vyra-watchdog.exe"), resolve(output, `vyra-watchdog-${target}.exe`));
console.log(`Watchdog MSVC preparado: binaries/vyra-watchdog-${target}.exe`);
