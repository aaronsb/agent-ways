//! The message verbs as library calls (ADR-501 items 2 and 4).
//!
//! Each verb takes typed arguments and returns a value or a typed error.
//! Nothing here prints, reads argv or exits the process, so the `attend`
//! CLI and any other frontend (an MCP server, a Claude Code mod, a script
//! through `attend <verb> --json`) run the same body and see the same
//! result. Results derive `Serialize`; their JSON is what `--json` prints.
//!
//! Every call derives the session afresh by process ancestry (ADR-171), once
//! per call, and reads the bus from disk, holding nothing for the next call
//! (ADR-501 items 1 and 4); a session id changed by `/clear` is the new id on
//! the next call. The caller's process must therefore run inside the Claude Code
//! session it acts for; a process with no session ancestry acts as the
//! external `$USER@<terminal>` sender, as a plain terminal does.
//!
//! The inbound side stays outside: `attend run` (the Monitor sensor loop)
//! and `attend message inbox --drain` (the Stop hook) own delivery and the seen-set
//! (ADR-172). `inbox::list`, `inbox::read` and `unread::unread` here mark
//! nothing read. `listen` sets whether the drain delivers to this session at
//! all (ADR-406 item 10).

pub mod channels;
pub mod delete;
pub mod inbox;
pub mod listen;
pub mod recipients;
pub mod peers;
pub mod send;
pub mod status;
pub mod tombstone;
pub mod unread;

#[cfg(feature = "sensor-peers")]
pub use send::reply;
pub use send::{send, Replied, SendError, SendRequest, Sent, Threading};
