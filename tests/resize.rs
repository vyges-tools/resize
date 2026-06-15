//! End-to-end resize tests — fully offline (the sta-si timer is pure std, no simulator).

use vyges_resize::engine::run_inputs;
use vyges_resize::job::{glob_match, parse_cfg};
use vyges_sta_si::job::StaJob;
use vyges_sta_si::netlist;

const LIB: &str = r#"
library (d) {
  cell (INV) {
    pin (A) { direction : input; capacitance : 0.0015; }
    pin (Y) { direction : output;
      timing () { related_pin : "A";
        cell_rise (t)       { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.10, 0.24", "0.14, 0.32" ); }
        cell_fall (t)       { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.09, 0.22", "0.13, 0.30" ); }
        rise_transition (t) { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.04, 0.10", "0.05, 0.12" ); }
        fall_transition (t) { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.04, 0.09", "0.05, 0.11" ); } } }
  }
  cell (INV2) {
    pin (A) { direction : input; capacitance : 0.0010; }
    pin (Y) { direction : output;
      timing () { related_pin : "A";
        cell_rise (t)       { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.04, 0.10", "0.06, 0.14" ); }
        cell_fall (t)       { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.035, 0.09", "0.055, 0.13" ); }
        rise_transition (t) { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.015, 0.045", "0.02, 0.055" ); }
        fall_transition (t) { index_1 ("0.01, 0.08"); index_2 ("0.001, 0.01"); values ( "0.015, 0.04", "0.02, 0.05" ); } } }
  }
}
"#;

const NL_INV: &str = "module top ( a, y ); input a; output y; wire n1;\n\
                      INV u1 ( .A(a), .Y(n1) ); INV u2 ( .A(n1), .Y(y) ); endmodule";
const NL_INV2: &str = "module top ( a, y ); input a; output y; wire n1;\n\
                       INV2 u1 ( .A(a), .Y(n1) ); INV2 u2 ( .A(n1), .Y(y) ); endmodule";

fn sta(period: f64) -> StaJob {
    StaJob::parse(
        &format!("design: t\nnetlist: x\nlib: x\nclock: clk {period}\ninput_slew: 0.02\noutput_load: 0.005\n"),
        "",
    )
    .unwrap()
}

#[test]
fn closes_setup_violation_by_upsizing() {
    let cfg = parse_cfg("group: INV INV2\nobjective: timing\neffort: high\n").unwrap();
    let r = run_inputs(NL_INV, LIB, &sta(0.30), &cfg).unwrap();
    assert!(r.before_wns < 0.0, "the input should violate at 0.30 ns");
    assert!(r.after_wns >= 0.0, "resize should close timing: {} -> {}", r.before_wns, r.after_wns);
    assert!(!r.changed.is_empty());
    assert!(r.changed.iter().all(|(_, old, new)| old == "INV" && new == "INV2"));

    // the emitted netlist round-trips and carries the upsized cells.
    let nl2 = netlist::parse(&r.netlist_v).unwrap();
    assert_eq!(nl2.insts.len(), 2);
    assert!(nl2.insts.iter().any(|i| i.cell == "INV2"));
}

#[test]
fn dont_touch_blocks_the_fix() {
    let cfg = parse_cfg("group: INV INV2\nobjective: timing\neffort: high\ndont_touch: u1 u2\n").unwrap();
    let r = run_inputs(NL_INV, LIB, &sta(0.30), &cfg).unwrap();
    assert!(r.changed.is_empty(), "every instance is dont_touch");
    assert!(r.after_wns < 0.0, "so the violation can't be fixed");
}

#[test]
fn recovers_area_by_downsizing_when_met() {
    // start on the fast cell at a loose period -> plenty of slack -> downsize to INV.
    let cfg = parse_cfg("group: INV INV2\nobjective: timing\neffort: high\n").unwrap();
    let r = run_inputs(NL_INV2, LIB, &sta(1.0), &cfg).unwrap();
    assert!(r.before_wns >= 0.0 && r.after_wns >= 0.0, "stays met");
    assert!(!r.changed.is_empty(), "should downsize at least one cell");
    assert!(r.changed.iter().all(|(_, old, new)| old == "INV2" && new == "INV"));
    let nl2 = netlist::parse(&r.netlist_v).unwrap();
    assert!(nl2.insts.iter().any(|i| i.cell == "INV"));
}

#[test]
fn globs() {
    assert!(glob_match("clk_*", "clk_a"));
    assert!(glob_match("*_reg", "x_reg"));
    assert!(glob_match("*scan*", "u_scan_0"));
    assert!(glob_match("u1", "u1"));
    assert!(!glob_match("u1", "u2"));
    assert!(!glob_match("clk_*", "rst_a"));
}
