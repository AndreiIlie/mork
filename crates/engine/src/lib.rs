//! The shell around `core`: it stores the changes, carries out the intents (jobs, actions)
//! and gives `core` what happened as events (1.12.1). One module per outside world (1.12.2).
#![forbid(unsafe_code)]

pub mod azure;
pub mod bitbucket;
pub mod board;
pub mod builds;
mod claude_accounts;
pub mod cli;
pub mod clipboard;
pub mod clock;
pub mod config;
pub mod doctor;
pub mod door;
pub mod faults;
pub mod files;
pub mod git;
pub mod jobs;
pub mod paths;
pub mod random;
pub mod resources;
pub mod runtime;
pub mod secrets;
pub mod settings;
pub mod sockets;
pub mod store;
pub mod token;
pub mod web;
