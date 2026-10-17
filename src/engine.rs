//! The sizing loop: drive the `vyges-sta-si` [`Timer`] — rank the critical path, try
//! stronger variants on its instances (keep the best non-hold-breaking improvement), then
//! recover area by downsizing slack instances. The timer's `checkpoint`/`update`/`restore`
//! makes each candidate a try-and-keep; the logic of the design never changes.

use std::collections::{HashMap, HashSet};

use vyges_sta_si::job::StaJob;
use vyges_sta_si::liberty::Lib;
use vyges_sta_si::netlist;
use vyges_sta_si::spef::Spef;
use vyges_sta_si::sta::Timer;

use crate::emit;
use crate::job::{glob_match, Objective, ResizeCfg, ResizeJob};

/// Outcome of a sizing run.
#[derive(Debug, Clone)]
pub struct ResizeResult {
    pub before_wns: f64,
    pub before_tns: f64,
    pub after_wns: f64,
    pub after_tns: f64,
    /// `(instance, old_cell, new_cell)` for every committed swap, in order.
    pub changed: Vec<(String, String, String)>,
    /// The resized netlist as structural Verilog.
    pub netlist_v: String,
    /// Whether sizing was scored against real interconnect parasitics (a SPEF was supplied,
    /// i.e. post-place ECO mode) rather than ideal interconnect.
    pub eco: bool,
}

/// Build a [`Timer`] from a job, reading the netlist / Liberty / SPEF files it names.
fn build_timer(sta: &StaJob) -> Result<Timer, String> {
    // the timer's own reader: libraries merged in the first one's units, SPEF in the netlist's names
    let (nl, lib, spef) =
        vyges_sta_si::engine::load_inputs(sta, Default::default()).map_err(|e| e.to_string())?;
    Timer::build(&nl, &lib, sta, spef.as_ref()).map_err(|e| e.to_string())
}

/// Run a sizing job loaded from disk. **Post-place ECO mode** kicks in automatically when the
/// job names a `spef:` — sizing is then scored against the real wire RC the placed design
/// presents, not ideal interconnect.
pub fn run(job: &ResizeJob) -> Result<ResizeResult, String> {
    optimize(build_timer(&job.sta)?, &job.cfg, job.sta.spef.is_some())
}

/// Run on already-parsed inputs (the `demo` path; ideal interconnect, no SPEF).
pub fn run_inputs(
    nl_text: &str,
    lib_text: &str,
    sta: &StaJob,
    cfg: &ResizeCfg,
) -> Result<ResizeResult, String> {
    run_inputs_spef(nl_text, lib_text, None, sta, cfg)
}

/// Run on already-parsed inputs with optional SPEF text — the offline form of post-place ECO
/// sizing. With `spef_text = Some(..)` every candidate is scored against real interconnect.
pub fn run_inputs_spef(
    nl_text: &str,
    lib_text: &str,
    spef_text: Option<&str>,
    sta: &StaJob,
    cfg: &ResizeCfg,
) -> Result<ResizeResult, String> {
    let nl = netlist::parse(nl_text).map_err(|e| e.to_string())?;
    let lib = Lib::parse(lib_text).map_err(|e| e.to_string())?;
    let spef = spef_text.map(|t| {
        let mut s = Spef::parse(t);
        s.rename_to_design(&nl); // the netlist's spelling of a net tied to a port, as the timer joins it
        s
    });
    let timer = Timer::build(&nl, &lib, sta, spef.as_ref()).map_err(|e| e.to_string())?;
    optimize(timer, cfg, spef.is_some())
}

/// The nearest member of `group` past position `pi` — toward the end when `up`, else toward the
/// start — that the library does not mark `dont_use`. A library-withheld cell is stepped over,
/// never chosen (the reference's resizer skips it in every candidate list), though an instance
/// may already hold one and still move off it.
fn neighbour(group: &[String], pi: usize, up: bool, lib: &Lib) -> Option<String> {
    let usable = |c: &&String| !lib.cells.get(c.as_str()).is_some_and(|l| l.dont_use);
    if up {
        group.iter().skip(pi + 1).find(usable).cloned()
    } else {
        group.iter().take(pi).rev().find(usable).cloned()
    }
}

/// The optimizer over a built [`Timer`]. `eco` records whether the timer carries interconnect
/// parasitics (post-place ECO sizing) for the report.
pub fn optimize(mut timer: Timer, cfg: &ResizeCfg, eco: bool) -> Result<ResizeResult, String> {
    // cell name -> (group index, position weakest..strongest)
    let mut pos: HashMap<String, (usize, usize)> = HashMap::new();
    for (gi, g) in cfg.groups.iter().enumerate() {
        for (pi, c) in g.iter().enumerate() {
            pos.insert(c.clone(), (gi, pi));
        }
    }
    let before_wns = timer.wns();
    let before_tns = timer.tns();
    let mut changed: Vec<(String, String, String)> = Vec::new();

    let dont = |inst: &str| cfg.dont_touch.iter().any(|p| glob_match(p, inst));
    let cell_of = |t: &Timer, inst: &str| -> Option<String> {
        t.netlist()
            .insts
            .iter()
            .find(|i| i.name == inst)
            .map(|i| i.cell.clone())
    };

    // ---- timing: upsize the critical path until setup is met or no move helps ----
    if cfg.objective == Objective::Timing {
        for _ in 0..cfg.effort {
            if timer.wns() >= 0.0 {
                break;
            }
            let (base_wns, base_whs) = (timer.wns(), timer.whs());
            // candidate upsizes: instances on the worst path that have a stronger variant.
            let mut cands: Vec<(String, String, String)> = Vec::new();
            let mut seen: HashSet<String> = HashSet::new();
            for node in timer.worst_path() {
                let Some((inst, _)) = node.label.split_once('/') else {
                    continue; // a port, not an instance pin
                };
                if !seen.insert(inst.to_string()) || dont(inst) {
                    continue;
                }
                let Some(cur) = cell_of(&timer, inst) else {
                    continue;
                };
                if let Some(&(gi, pi)) = pos.get(&cur) {
                    if let Some(up) = neighbour(&cfg.groups[gi], pi, true, timer.lib()) {
                        cands.push((inst.to_string(), cur, up));
                    }
                }
            }
            if cands.is_empty() {
                break;
            }
            // keep the best non-hold-breaking improvement (speculative eval per candidate).
            let mut best: Option<(f64, (String, String, String))> = None;
            for cand in &cands {
                let ck = timer.checkpoint();
                timer.resize(&cand.0, &cand.2);
                timer.update().map_err(|e| e.to_string())?;
                let (w, h) = (timer.wns(), timer.whs());
                timer.restore(ck);
                if w > base_wns + 1e-12 && h >= base_whs - 1e-9 {
                    let better = best.as_ref().map(|(bw, _)| w > *bw).unwrap_or(true);
                    if better {
                        best = Some((w, cand.clone()));
                    }
                }
            }
            match best {
                Some((_, (inst, old, new))) => {
                    timer.resize(&inst, &new);
                    timer.update().map_err(|e| e.to_string())?;
                    changed.push((inst, old, new));
                }
                None => break, // no improving move on the critical path
            }
        }
    }

    // ---- area recovery: downsize slack instances while timing stays met (one greedy pass) ----
    if timer.wns() >= 0.0 {
        let insts: Vec<(String, String)> = timer
            .netlist()
            .insts
            .iter()
            .map(|i| (i.name.clone(), i.cell.clone()))
            .collect();
        for (inst, cur) in insts {
            if dont(&inst) {
                continue;
            }
            let Some(&(gi, pi)) = pos.get(&cur) else {
                continue;
            };
            let Some(weaker) = neighbour(&cfg.groups[gi], pi, false, timer.lib()) else {
                continue; // already the weakest usable
            };
            let base_whs = timer.whs();
            let ck = timer.checkpoint();
            timer.resize(&inst, &weaker);
            timer.update().map_err(|e| e.to_string())?;
            if timer.wns() >= 0.0 && timer.whs() >= base_whs - 1e-9 {
                changed.push((inst, cur, weaker)); // accept: timing still met
            } else {
                timer.restore(ck); // reject: would violate
            }
        }
    }

    Ok(ResizeResult {
        before_wns,
        before_tns,
        after_wns: timer.wns(),
        after_tns: timer.tns(),
        changed,
        netlist_v: emit::to_verilog(timer.netlist()),
        eco,
    })
}
