//! Timer engine: pure state machines, with no clock and no I/O.

pub mod countdown;
pub mod error;
pub mod pomodoro;
pub mod run;
pub mod session;
pub mod stopwatch;
pub mod timers;

pub use error::EngineError;
