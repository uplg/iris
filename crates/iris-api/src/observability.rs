use tracing_subscriber::{EnvFilter, fmt, prelude::*};

pub fn init_tracing() {
    // html5ever WARN-spams "foster parenting not implemented" on the
    // garbage markup real trackers serve (HDT tables, TL pages) — one
    // parse can emit hundreds of lines. Real parse failures surface as
    // provider errors, so nothing of value is lost at error-level. The
    // directive is appended in code so a deployment's own RUST_LOG
    // doesn't silently reopen the firehose; a more specific env
    // directive (`html5ever::tree_builder=warn`) still wins if the
    // spam is ever wanted back.
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,iris=debug,tower_http=info"))
        .add_directive("html5ever=error".parse().expect("static filter directive"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
}
