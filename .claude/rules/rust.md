---
paths:
  - "src-tauri/**/*.rs"
---

# Rust / Tauri backend rules

- **This crate owns all filesystem I/O.** The frontend must never touch the FS;
  it calls typed Tauri commands exposed here.
- **All note writes go through a single atomic-write helper** (temp file in the
  same dir → rename over target). No other code path writes note files. Preserve
  line endings + encoding (§7.1). Add a unit test that round-trips CRLF and a
  UTF-8 BOM file unchanged.
- **Never panic on user input.** Return `Result<_, AppError>`; surface errors to
  the frontend as structured values — never `unwrap()`/`expect()` on I/O or parse
  paths, and never swallow an error silently.
- Keep the **Tauri command surface small and typed**; every command validates
  that its path is inside the active vault before doing anything.
- Keep the **capability allowlist minimal and default-deny**; no broad FS or
  network capabilities.
- Must be **`cargo clippy --all-targets -- -D warnings` clean** and `cargo fmt`
  formatted before commit.
- No network clients, no telemetry crates.
