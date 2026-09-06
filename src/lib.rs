//! Arc Input's independent `InputSharing` bounded context.
//!
//! Configuration aggregates compile into immutable [`RuntimeRoutingSnapshot`]
//! values. The high-frequency path reads those snapshots atomically and never
//! touches persistence or UI state.

pub const PRODUCT_ID: &str = "arc.input";
pub use arcrelay_wire::{
    MAX_CONTROL_FRAME_SIZE, MAX_RELIABLE_INPUT_FRAME_SIZE, STREAM_KIND_INPUT_CONTROL,
    STREAM_KIND_RELIABLE_INPUT,
};
/// Upper bound for an opaque `CGEventCreateData` payload carried between Macs.
/// Quartz events are normally much smaller; keeping a separate bound avoids
/// giving a native decoder the entire reliable-input frame budget.
pub const MAX_NATIVE_QUARTZ_EVENT_SIZE: usize = 16 * 1024;
pub const NATIVE_QUARTZ_FORMAT_VERSION: u32 = 1;

pub use arcrelay_peer::ServiceInstanceId;

pub mod consumer;
pub mod control;
pub mod display;
pub mod gesture;
pub mod keyboard;
pub mod platform;
pub mod runtime;
pub mod topology;
pub mod types;
pub mod wire;

pub mod proto {
    include!(concat!(env!("OUT_DIR"), "/arcinput.v1.rs"));
}

pub use consumer::*;
pub use control::*;
pub use display::*;
pub use gesture::*;
pub use keyboard::*;
pub use platform::*;
pub use runtime::*;
pub use topology::*;
pub use types::*;
