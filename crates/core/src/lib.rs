//! Wisp core: the HTTP engine and request model.

mod engine;
mod model;

pub mod body;
pub mod request;
pub mod resolve;
pub mod template;

pub use engine::{Engine, EngineConfig, EventReceiver};
pub use model::{
    ClientSettings, Command, EngineError, ErrorKind, Event, Header, HttpVersion, Method,
    PreparedBody, PreparedPart, PreparedPartValue, PreparedRequest, RequestId,
};
