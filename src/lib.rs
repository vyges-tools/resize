//! vyges-resize — STA-driven gate sizing.
//!
//! Reads a gate-level netlist + Liberty + constraints and **chooses a better drive strength
//! for each cell** — upsizing cells on critical paths to close setup/transition violations,
//! downsizing cells with slack to recover area — then emits the resized netlist plus a
//! before/after timing report. Every decision is scored by the `vyges-sta-si` timer; the
//! logic never changes.
//!
//! It is the first of the "close-timing" engines: where the analysis engines *say what's
//! wrong*, this one *fixes it*. It picks **sizes, not locations** — physical
//! legalization/placement stays the flow's job; resize runs beside it (pre-place: netlist
//! in, better netlist out).
//!
//! v0 scope: pre-place drive-strength sizing on a declared variant family (`group:` lines),
//! objective = close setup WNS (and recover area where there's slack), driven by the timer's
//! query + speculative checkpoint/restore API. Pure std (the timer is too) — fully
//! unit-tested offline, no simulator.

// The crate's own name inside it: the command-line entry ([`cli_main`]) is built into the
// library too, and its `vyges_resize::…` paths resolve here.
extern crate self as vyges_resize;

pub mod emit;
pub mod engine;
pub mod job;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "© 2026 Vyges. All Rights Reserved.  https://vyges.com";

/// The `vyges-resize` command-line entry (the binary's `main`), built into the library too so a
/// suite binary can link every engine and run the one it is invoked as.
#[cfg(feature = "suite")]
#[doc(hidden)]
#[path = "main.rs"]
pub mod cli_main;
