//! Helpers shared by the database adapters.

/// Renders bytes the way the adapters show binary values: lowercase hex with a
/// `\x` prefix, matching PostgreSQL's own bytea output.
pub(crate) fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
