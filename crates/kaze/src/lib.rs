//! The kaze application: storage, downloads, jobs and the web server.
//! `main.rs` is a thin CLI on top of this library.

pub mod api;
pub mod app;
pub mod config;
pub mod db;
pub mod download;
pub mod events;
pub mod integrations;
pub mod jobs;
pub mod library;
pub mod settings;
#[cfg(test)]
mod testing;
