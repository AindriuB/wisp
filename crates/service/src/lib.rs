//! The GPUI-free service layer shared by the GUI and the CLI: send planning, the one
//! `Command::Send` construction, outcomes, history recording and keychain wiring.

pub mod history;
pub mod outcome;
pub mod plan;
pub mod send;
pub mod vault;
