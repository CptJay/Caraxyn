//! Caraxyn core: everything except the GUI.

/// Name of this crate, as Cargo knows it.
pub fn crate_name() -> &'static str {
    env!("CARGO_PKG_NAME")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_name_is_caraxyn_core() {
        assert_eq!(crate_name(), "caraxyn-core");
    }
}
