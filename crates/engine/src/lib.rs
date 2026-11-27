//! The shell around `core`: it stores the changes, carries out the intents (jobs, actions)
//! and gives `core` what happened as events (1.12.1). One module per outside world (1.12.2).
#![forbid(unsafe_code)]

mod azure;
mod bitbucket;
pub mod board;
pub mod builds;
mod claude_accounts;
pub mod cli;
pub mod clipboard;
pub mod clock;
pub mod config;
mod config_checks;
pub mod doctor;
pub mod door;
mod encoding;
pub mod faults;
pub mod files;
mod forge;
pub mod git;
mod github;
pub mod init;
pub mod jobs;
mod lock;
pub mod login_files;
pub mod paths;
pub mod random;
pub mod resources;
pub mod runtime;
pub mod seat_homes;
pub mod secrets;
pub mod settings;
pub mod sockets;
pub mod store;
mod tickets;
pub mod web;
