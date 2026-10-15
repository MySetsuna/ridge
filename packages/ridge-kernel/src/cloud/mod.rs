//! Cloud transport primitives shared by Runtime and CLI shells.
pub mod e2ee;
pub mod envelope;
pub mod ice;
pub mod mux;
pub mod protocol;
pub mod rtc;
pub mod signaling;
pub mod key_binding;
pub mod session_control;
pub mod local_tls;
pub mod workspace_scope;
pub mod chunk;
pub mod connection;
pub mod manager;
pub(crate) mod controller;
pub(crate) mod device_pins;
pub(crate) mod http;
