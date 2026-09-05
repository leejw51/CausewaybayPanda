//! Everything a cafe does, with nothing it does it *on*. The same `apply`
//! runs behind an axum socket over SQLite and inside a browser tab over a
//! `MemStore`; the wire frames are identical, so a page cannot tell which.

pub mod cafe;
pub mod demo;
pub mod mem;
pub mod settlement;
pub mod shop;
pub mod store;

pub use mem::MemStore;
pub use shop::Shop;
pub use store::{SessionRow, Store, Takings};
