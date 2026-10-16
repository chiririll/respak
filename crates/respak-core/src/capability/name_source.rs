//! The [`NameSource`] capability.

/// Strings in an asset's own data that may be the names (paths) of other assets: a table's
/// asset-path cells, the settings that point at assets. Guesses, not facts: a consumer keeps the
/// ones that hash to a guid it is looking for.
pub trait NameSource: Send + Sync {
    /// The candidate names, in the order they appear.
    fn name_candidates(&self) -> Vec<String>;
}
