use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_tracing() {
    // html5ever WARN-spams "foster parenting not implemented" on the
    // garbage markup real trackers serve (HDT tables, TL pages) — one
    // parse can emit hundreds of lines. Real parse failures surface as
    // provider errors, so nothing of value is lost at error-level.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,iris=debug,tower_http=info,html5ever=error"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}
