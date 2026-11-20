# Contributing

VYRA is free and open source. Keep all processing local and preserve these boundaries:

- Never access game processes, modify game files, inject code, install drivers, or claim anti-cheat compatibility without verifiable evidence.
- Keep unsupported controls visibly disabled. Do not report a simulated result as a display change.
- Validate imported and command inputs. Preserve versioned profile data and original display state.
- Do not test global display effects without explicit authorization.

Before opening a change, run `npm run build`, `npm test`, and `npm run lint`. Rust changes also require `cargo fmt --check`, `cargo test`, and `cargo check` on Windows. Include exact commands and results in the change description.
