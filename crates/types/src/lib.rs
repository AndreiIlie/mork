//! Everything that crosses a boundary: commands, answers, views, socket messages, job
//! records, the agents' reply schemas and the config. Serde only, no I/O (1.1.1).
#![forbid(unsafe_code)]

pub mod about;
pub mod action;
pub mod ask;
pub mod card;
pub mod catalog;
pub mod chat;
pub mod command;
pub mod config;
pub mod cost;
pub mod decisions;
pub mod door;
pub mod door_call;
pub mod door_config;
pub mod door_plan;
pub mod hash;
pub mod ids;
pub mod input;
pub mod job;
pub mod landing;
pub mod lease;
pub mod lookback;
pub mod memory;
pub mod note;
pub mod orchestrator;
pub mod overview;
pub mod pipeline;
pub mod plan;
pub mod ports;
pub mod protocol;
pub mod quality;
pub mod reply;
pub mod review;
pub mod rooms;
pub mod seen;
pub mod state;
pub mod suites;
pub mod usage;
pub mod view;
pub mod web_tunnel;
pub mod words;
pub mod work;

pub use command::{Command, Pick};
pub use ids::{ActionId, AgentKey, AskId, CardId, JobId, LeaseId, PlaceId, ProjectId, SeatName, SessionId, Sha};
