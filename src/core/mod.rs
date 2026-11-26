// Aurora Browser — Rust-ядро
// Модули core не зависят от tauri (легко тестируются и переносятся).

pub mod bundled;
mod history_db;
pub mod model;
pub mod plugins;
pub mod store;
pub mod urls;
