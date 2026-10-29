//! Wisp core: the HTTP engine and request model.

mod engine;
mod model;

pub use engine::{Engine, EngineConfig, EventReceiver};
pub use model::{
    Command, EngineError, ErrorKind, Event, Header, Method, PreparedRequest, RequestId,
};
