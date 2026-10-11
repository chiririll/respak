//! The [`Metadata`] capability.

/// A reference to another asset, as found in an asset's own data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    /// The referenced asset's guid.
    pub guid: u64,
    /// Its name, if the referencing data carries one.
    pub name: Option<String>,
}

/// Name, guid and dependencies of an asset. All of it is optional: a format provides what it has.
///
/// Dependencies are only *reported* here; resolving them is the mediator's job (an
/// `rpak.guid → rpak.asset` edge).
pub trait Metadata: Send + Sync {
    /// The asset's name, if known.
    fn name(&self) -> Option<&str>;
    /// The asset's guid, if it has one.
    fn guid(&self) -> Option<u64>;
    /// Assets this one refers to.
    fn dependencies(&self) -> Vec<Dependency>;
}
