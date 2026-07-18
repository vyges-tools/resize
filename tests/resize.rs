//! End-to-end resize tests — fully offline (the sta-si timer is pure std, no simulator).

use vyges_resize::engine::{run_inputs, run_inputs_spef};
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
    assert!(
        r.after_wns >= 0.0,
        "resize should close timing: {} -> {}",
        r.before_wns,
        r.after_wns
    );
    assert!(!r.changed.is_empty());
    assert!(r
        .changed
        .iter()
        .all(|(_, old, new)| old == "INV" && new == "INV2"));

    // the emitted netlist round-trips and carries the upsized cells.
    let nl2 = netlist::parse(&r.netlist_v).unwrap();
    assert_eq!(nl2.insts.len(), 2);
    assert!(nl2.insts.iter().any(|i| i.cell == "INV2"));
}

#[test]
fn dont_touch_blocks_the_fix() {
    let cfg =
        parse_cfg("group: INV INV2\nobjective: timing\neffort: high\ndont_touch: u1 u2\n").unwrap();
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
    assert!(r
        .changed
        .iter()
        .all(|(_, old, new)| old == "INV2" && new == "INV"));
    let nl2 = netlist::parse(&r.netlist_v).unwrap();
    assert!(nl2.insts.iter().any(|i| i.cell == "INV"));
}

// post-place ECO: a heavy-RC net on n1 (the wire u1 drives). With SPEF the driver sees ~8×
// the static load, so the path violates and sizing has real leverage; ideal interconnect
// doesn't see it.
const SPEF_HEAVY_N1: &str = r#"
*SPEF "IEEE 1481-1999"
*C_UNIT 1 FF
*R_UNIT 1 OHM
*NAME_MAP
*1 n1
*2 u1
*3 u2
*D_NET *1 250.000000
*CONN
*I *2:Y O
*I *3:A I
*CAP
1 *2:Y 125.000000
2 *1 125.000000
*RES
1 *1 *2:Y 3000.000000
2 *1 *3:A 3000.000000
*END
"#;

#[test]
fn eco_spef_gives_real_sizing_leverage() {
    let cfg = parse_cfg("group: INV INV2\nobjective: timing\neffort: high\n").unwrap();
    // a period the ideal-interconnect path comfortably meets...
    let ideal = run_inputs(NL_INV, LIB, &sta(0.6), &cfg).unwrap();
    assert!(!ideal.eco);
    assert!(
        ideal.before_wns >= 0.0,
        "ideal should meet at 0.6 ns: {}",
        ideal.before_wns
    );
    assert!(
        ideal.changed.is_empty(),
        "nothing to do on a met, already-weakest design"
    );

    // ...but the real wire RC pushes it into violation, and sizing recovers it.
    let eco = run_inputs_spef(NL_INV, LIB, Some(SPEF_HEAVY_N1), &sta(0.6), &cfg).unwrap();
    assert!(eco.eco);
    assert!(
        eco.before_wns < ideal.before_wns,
        "SPEF must add real wire delay"
    );
    assert!(
        eco.before_wns < 0.0,
        "the heavy net should violate: {}",
        eco.before_wns
    );
    assert!(eco.after_wns > eco.before_wns, "sizing should improve it");
    // the leverage is on u1 — the driver of the heavy net.
    assert!(
        eco.changed.iter().any(|(inst, _, _)| inst == "u1"),
        "expected u1 (heavy-net driver) to be upsized, got {:?}",
        eco.changed
    );
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
