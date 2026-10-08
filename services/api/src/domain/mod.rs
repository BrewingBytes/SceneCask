//! Pure domain policy (architecture.md): no HTTP, database or clock access. Callers pass the
//! server clock and catalog/progress rows in.

pub mod access;
pub mod progress;
pub mod release;
pub mod spoilers;
