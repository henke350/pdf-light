//! pdf-light: a small PDF reader. The library holds the logic;
//! the binary in `main.rs` only starts the window.

pub mod document;
pub mod edit;
pub mod error;
pub mod export;
pub mod pages;
pub mod render;
pub mod render_pool;
pub mod search;
pub mod search_worker;
pub mod selection;
pub mod text;
pub mod view;
