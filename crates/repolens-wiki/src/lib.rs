//! Wiki compiled knowledge layer for RepoLens.
//!
//! Provides structured article storage, staleness assessment, and keyword search
//! over a directory of Markdown wiki articles managed within a repository's
//! `.repolens/wiki/` store.

pub mod coverage;
pub mod index;
pub mod search;
pub mod staleness;
pub mod store;
