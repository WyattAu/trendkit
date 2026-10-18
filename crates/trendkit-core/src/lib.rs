//! Core types, SQLite snapshot store, source traits, and velocity scoring
//! for trendkit.
//!
//! The crate is deliberately small: sources implement [`source::TrendSource`],
//! observations land in [`store::Store`], and [`score`] turns snapshot pairs
//! into per-day velocities. Time is always injected as a
//! [`clock_kit::Clock`] so collectors and scoring are deterministic under
//! test.

pub mod clock;
pub mod cratesio;
pub mod error;
pub mod github;
pub mod hn;
pub mod model;
pub mod score;
pub mod source;
pub mod store;

pub use error::{Result, TrendError};
pub use model::{Observation, RepoListed, SourceKind, SubjectRecord, VelocityRow};
pub use source::{CollectCtx, CollectReport, TrendSource};
pub use store::Store;
