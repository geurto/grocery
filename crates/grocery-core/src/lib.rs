//! Core library for the recipe / grocery app.
//!
//! Pipeline: recipe files (`.cook` or `.md` with YAML frontmatter)
//! → [`parse`] into the normalized [`model::Recipe`]
//! → [`aggregate::build`] scales, resolves names through [`food::FoodDb`],
//!   converts to grams where possible and merges into a [`aggregate::ShoppingList`]
//! → [`render`] turns that into Markdown (for the vault) or plain text (for Signal).
//!
//! The vault is the source of truth: everything here is a pure function of
//! file contents, so the web service can re-run it whenever a file changes.

pub mod aggregate;
pub mod error;
pub mod food;
pub mod model;
pub mod nutrition;
pub mod parse;
pub mod render;
pub mod units;

pub use error::{Error, Result};
