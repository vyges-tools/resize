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

pub mod emit;
pub mod engine;
pub mod job;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const COPYRIGHT: &str = "© 2026 Vyges. All Rights Reserved.  https://vyges.com";
