//! Caraxyn core: everything except the GUI.

pub mod error;
pub mod ids;
pub mod node;

/// Name of this crate, as Cargo knows it.
pub fn crate_name() -> &'static str {
    tracing::trace!("crate_name called");
    env!("CARGO_PKG_NAME")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_caraxyn_core() {
        // Shows logs with: RUST_LOG=caraxyn_core=trace cargo test -- --nocapture
        let _ = tracing_subscriber::fmt()
            .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
            .with_test_writer()
            .try_init();
        assert_eq!(crate_name(), "caraxyn-core");
    }
}
