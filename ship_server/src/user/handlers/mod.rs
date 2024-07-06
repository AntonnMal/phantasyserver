use crate::{Action, Error};

pub mod arksmission;
pub mod chat;
pub mod friends;
pub mod item;
pub mod login;
pub mod missionpass;
pub mod object;
pub mod palette;
pub mod party;
pub mod quest;
pub mod server;
pub mod settings;
pub mod symbolart;
pub mod player_status;

type HResult = Result<Action, Error>;
