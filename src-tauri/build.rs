fn main() {
    use sha2::{Digest, Sha256};
    let target = std::env::var("TARGET").expect("TARGET");
    println!("cargo:rerun-if-env-changed=VYRA_WATCHDOG_SIDECAR");
    let helper = std::env::var_os("VYRA_WATCHDOG_SIDECAR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(
                std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
            )
            .join(format!("binaries/vyra-watchdog-{target}.exe"))
        });
    println!("cargo:rerun-if-changed={}", helper.display());
    let bytes = std::fs::read(&helper).unwrap_or_else(|error| {
        panic!(
            "watchdog sidecar unavailable at {}: {error}",
            helper.display()
        )
    });
    let digest = Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    println!("cargo:rustc-env=VYRA_WATCHDOG_SHA256={digest}");
    tauri_build::build()
}
