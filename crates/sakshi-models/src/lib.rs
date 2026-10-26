//! The bundled model manifest, Hugging Face downloads, checksum
//! verification, and license display.
//!
//! Network access happens only in [`Hub`], and only when the user asks for a
//! download or adds a custom model.

mod error;
pub mod gguf;
mod hub;
mod manifest;
mod store;

pub use error::Error;
pub use hub::{CustomModel, DownloadProgress, Hub};
pub use manifest::{Engine, Format, Manifest, ModelEntry, Role};
pub use store::ModelStore;
