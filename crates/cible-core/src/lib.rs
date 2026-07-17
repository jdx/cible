//! cible-core — domain model and the local warehouse.
//!
//! The warehouse is a disposable SQLite materialization of CI history whose
//! source of truth is GitHub itself; it can be rebuilt from scratch at any
//! time with a read-only token.

pub mod warehouse;

pub use warehouse::Warehouse;
