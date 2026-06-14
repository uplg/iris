//! Emits the `OpenAPI` 3.1 spec — the server-free source for client codegen.
//!
//!   - `cargo run -q -p iris-api --bin gen-openapi`            → prints to stdout
//!   - `cargo run -q -p iris-api --bin gen-openapi -- --write` → writes the
//!     committed `web/openapi.json` (the contract the web build derives its
//!     TS types from; kept in sync with the Rust types by the snapshot test
//!     in `openapi.rs`). Wired into the web `bun run gen-api` script.

use iris_api::openapi::{spec_json, spec_path};

fn main() {
    let json = spec_json();
    if std::env::args().any(|a| a == "--write") {
        let path = spec_path();
        if let Err(e) = std::fs::write(&path, format!("{json}\n")) {
            eprintln!("failed to write {}: {e}", path.display());
            std::process::exit(1);
        }
        eprintln!("wrote {}", path.display());
    } else {
        println!("{json}");
    }
}
