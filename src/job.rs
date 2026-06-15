//! The `.resize` job: the timing setup (reused from `vyges-sta-si`'s job parser) plus the
//! resize-specific knobs. A `.resize` file is a superset of a `.sta` file — it carries the
//! same `design`/`netlist`/`lib`/`clock`/… keys (read by [`StaJob`]) and adds:
//!
//! ```text
//! group:      INV INV2 INV4        # interchangeable cells, weakest -> strongest (repeatable)
//! group:      NAND2 NAND2X2
//! objective:  timing               # timing | area  (default: timing)
//! effort:     medium               # low | medium | high  (iteration budget)
//! dont_touch: clk_* *scan*         # instance-name globs to leave alone
//! ```

use vyges_sta_si::job::StaJob;

/// What to optimize for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Objective {
    /// Close setup WNS (upsize critical paths), then recover area where slack allows.
    Timing,
    /// Recover area/leakage (downsize positive-slack cells) while keeping timing met.
    Area,
}

/// The resize-specific configuration (everything beyond the timing setup).
#[derive(Debug, Clone)]
pub struct ResizeCfg {
    /// Interchangeable cell families, each ordered weakest → strongest.
    pub groups: Vec<Vec<String>>,
    pub objective: Objective,
    /// Iteration budget (derived from `effort:`).
    pub effort: usize,
    /// Instance-name globs (supporting a leading/trailing `*`) to never modify.
    pub dont_touch: Vec<String>,
}

/// A loaded resize job: the timing job + the resize config.
#[derive(Debug, Clone)]
pub struct ResizeJob {
    pub sta: StaJob,
    pub cfg: ResizeCfg,
}

impl ResizeJob {
    pub fn load(path: &str) -> Result<ResizeJob, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
        // Timing setup via the sta-si parser (it ignores the resize-only keys).
        let sta = StaJob::load(path).map_err(|e| e.to_string())?;
        let cfg = parse_cfg(&text)?;
        Ok(ResizeJob { sta, cfg })
    }
}

/// Parse the resize-only keys out of the job text.
pub fn parse_cfg(text: &str) -> Result<ResizeCfg, String> {
    let mut groups = Vec::new();
    let mut objective = Objective::Timing;
    let mut effort_word = "medium".to_string();
    let mut dont_touch = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        let Some((k, v)) = line.split_once(':') else { continue };
        let (k, v) = (k.trim().to_lowercase(), v.trim());
        match k.as_str() {
            "group" => {
                let g: Vec<String> = v.split_whitespace().map(str::to_string).collect();
                if g.len() >= 2 {
                    groups.push(g);
                }
            }
            "objective" => {
                objective = match v.to_lowercase().as_str() {
                    "area" => Objective::Area,
                    "timing" => Objective::Timing,
                    other => return Err(format!("objective must be timing|area, got {other:?}")),
                };
            }
            "effort" => effort_word = v.to_lowercase(),
            "dont_touch" => {
                dont_touch.extend(v.split([',', ' ']).map(str::trim).filter(|s| !s.is_empty()).map(str::to_string));
            }
            _ => {}
        }
    }
    let effort = match effort_word.as_str() {
        "low" => 20,
        "medium" => 100,
        "high" => 500,
        other => return Err(format!("effort must be low|medium|high, got {other:?}")),
    };
    Ok(ResizeCfg { groups, objective, effort, dont_touch })
}

/// A tiny glob matcher: supports a single leading and/or trailing `*` (e.g. `clk_*`,
/// `*scan*`, `*_reg`). Exact match otherwise.
pub fn glob_match(pat: &str, s: &str) -> bool {
    match (pat.strip_prefix('*'), pat.strip_suffix('*')) {
        (Some(_), Some(_)) => s.contains(pat.trim_matches('*')),
        (Some(suf), None) => s.ends_with(suf),
        (None, Some(pre)) => s.starts_with(pre),
        (None, None) => s == pat,
    }
}
