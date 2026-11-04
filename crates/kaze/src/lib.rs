//! The kaze application: storage, downloads, jobs and the web server.
//! `main.rs` is a thin CLI on top of this library.

pub mod api;
pub mod app;
pub mod backup;
pub mod config;
pub mod db;
pub mod download;
pub mod events;
pub mod import;
pub mod integrations;
pub mod jobs;
pub mod library;
pub mod logs;
pub mod metadata;
pub mod settings;
#[cfg(test)]
mod testing;
pub mod upcoming;
pub mod updates;
