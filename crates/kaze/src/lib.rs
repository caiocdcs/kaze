//! The kaze application: storage, downloads, jobs and the web server.
//! `main.rs` is a thin CLI on top of this library.

pub mod api;
pub mod app;
pub mod config;
pub mod db;
pub mod download;
pub mod events;
pub mod jobs;
pub mod library;
#[cfg(test)]
mod testing;
