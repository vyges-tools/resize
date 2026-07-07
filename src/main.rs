//! vyges-resize CLI.
//!
//!   vyges-resize run   JOB  [-o OUT] [--json] [--fail-on-violation]
//!   vyges-resize check JOB
//!   vyges-resize demo
//!
//! Common flags: -h/--help, -V/--version, -q/--quiet, -v/--verbose.
//! Exit codes: 0 ok · 1 runtime error · 2 usage/validation · 3 still-violating (--fail-on-violation).

use std::process::exit;

use vyges_resize::engine::{self, ResizeResult};
use vyges_resize::job::{parse_cfg, Objective, ResizeJob};
use vyges_sta_si::job::StaJob;

const USAGE: &str = "\
vyges-resize — STA-driven gate sizing (drive-strength resize / Vt-swap to close timing)

usage:
  vyges-resize run   JOB  [-o OUT] [--json] [--fail-on-violation]   size a netlist -> resized netlist
  vyges-resize check JOB                                            validate the job
  vyges-resize demo                                                 size a built-in example (no files)

flags:
  -o FILE              write the resized netlist to FILE (default: stdout)
  --json               emit the before/after report as JSON
  --fail-on-violation  exit 3 if the result still has negative setup slack (CI gate)
  -q, --quiet          suppress non-essential output
  -v, --verbose        extra detail on stderr
  -h, --help           show this help
  -V, --version        show version
  --bug-report         file a bug (central: vyges/community)
  --feature-request    request a feature (central)
  --sponsor            sponsor Vyges (github.com/sponsors/vyges-ip)
  --star               star this tool on GitHub ⭐
";

const BUG_URL: &str = "https://github.com/vyges/community/issues/new?template=bug_report_template.yaml";
const FEATURE_URL: &str = "https://github.com/vyges/community/issues/new?labels=enhancement";
const SPONSOR_URL: &str = "https://github.com/sponsors/vyges-ip";
const STAR_URL: &str = "https://github.com/vyges-tools/resize";

fn link(label: &str, url: &str) {
    use std::io::IsTerminal;
    println!("{label}:\n  {url}");
    if std::io::stdout().is_terminal() {
        let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
        let _ = std::process::Command::new(opener).arg(url).status();
    }
}

#[derive(Default)]
struct Cli {
    positionals: Vec<String>,
    out: Option<String>,
    json: bool,
    fail_on_violation: bool,
    quiet: bool,
    verbose: bool,
    help: bool,
    version: bool,
    bug_report: bool,
    feature_request: bool,
    sponsor: bool,
    star: bool,
}

fn parse_cli(args: &[String]) -> Cli {
    let mut c = Cli::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                c.out = args.get(i + 1).cloned();
                i += 1;
            }
            "--json" => c.json = true,
            "--fail-on-violation" => c.fail_on_violation = true,
            "-q" | "--quiet" => c.quiet = true,
            "-v" | "--verbose" => c.verbose = true,
            "-h" | "--help" => c.help = true,
            "-V" | "--version" => c.version = true,
            "--bug-report" => c.bug_report = true,
            "--feature-request" => c.feature_request = true,
            "--sponsor" => c.sponsor = true,
            "--star" => c.star = true,
            other => c.positionals.push(other.to_string()),
        }
        i += 1;
    }
    c
}

fn render_report(r: &ResizeResult) -> String {
    let met = |w: f64| if w >= 0.0 { "MET" } else { "VIOLATED" };
    let mut s = String::new();
    s.push_str("vyges-resize — gate sizing\n");
    s.push_str(&format!(
        "  mode:    {}\n",
        if r.eco { "post-place ECO (SPEF interconnect)" } else { "pre-place (ideal interconnect)" }
    ));
    s.push_str(&format!(
        "  before:  WNS {:.4} ns [{}]   TNS {:.4} ns\n",
        r.before_wns, met(r.before_wns), r.before_tns
    ));
    s.push_str(&format!(
        "  after:   WNS {:.4} ns [{}]   TNS {:.4} ns\n",
        r.after_wns, met(r.after_wns), r.after_tns
    ));
    s.push_str(&format!("  changed: {} cell(s)\n", r.changed.len()));
    for (inst, old, new) in &r.changed {
        s.push_str(&format!("    {inst}: {old} -> {new}\n"));
    }
    s
}

fn report_json(r: &ResizeResult) -> String {
    let changes: Vec<String> = r
        .changed
        .iter()
        .map(|(i, o, n)| format!("{{\"inst\":\"{i}\",\"old\":\"{o}\",\"new\":\"{n}\"}}"))
        .collect();
    format!(
        "{{\"eco\":{},\"before_wns\":{},\"before_tns\":{},\"after_wns\":{},\"after_tns\":{},\"changed\":[{}]}}",
        r.eco, r.before_wns, r.before_tns, r.after_wns, r.after_tns, changes.join(",")
    )
}

// ---- built-in demo: a 2-inverter chain too slow for a tight clock, with an INV/INV2 family ----
const DEMO_NL: &str = "module top ( a, y ); input a; output y; wire n1;\n\
                       INV u1 ( .A(a), .Y(n1) ); INV u2 ( .A(n1), .Y(y) ); endmodule";
const DEMO_LIB: &str = r#"
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
const DEMO_JOB: &str = "design: demo\nnetlist: x\nlib: x\nclock: clk 0.30\ninput_slew: 0.02\noutput_load: 0.005\n";

fn run_demo() -> Result<ResizeResult, String> {
    let sta = StaJob::parse(DEMO_JOB, "").map_err(|e| e.to_string())?;
    let cfg = parse_cfg("group: INV INV2\nobjective: timing\neffort: medium\n")?;
    engine::run_inputs(DEMO_NL, DEMO_LIB, &sta, &cfg)
}

fn write_netlist(text: &str, out: &Option<String>, quiet: bool) {
    match out {
        Some(path) => match std::fs::write(path, text) {
            Ok(_) => {
                if !quiet {
                    eprintln!("wrote {path}");
                }
            }
            Err(e) => {
                eprintln!("error: {path}: {e}");
                exit(1);
            }
        },
        None => print!("{text}"),
    }
}

fn finish(r: ResizeResult, cli: &Cli) {
    if cli.json {
        println!("{}", report_json(&r));
        if cli.out.is_some() {
            write_netlist(&r.netlist_v, &cli.out, cli.quiet);
        }
    } else {
        write_netlist(&r.netlist_v, &cli.out, cli.quiet);
        if !cli.quiet {
            eprint!("{}", render_report(&r));
        }
    }
    if cli.fail_on_violation && r.after_wns < 0.0 {
        exit(3);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--describe") {
        // Machine-readable description of `run` for tooling that drives it.
        const DESCRIBE: &str = r#"{
  "name": "resize",
  "summary": "STA-driven gate sizing (drive-strength resize / Vt-swap to close timing)",
  "invocation": {
    "args_template": ["run", "{job}"],
    "optional": [ { "arg": "out", "flag": "-o" } ],
    "emits_json": true
  },
  "inputs": {
    "type": "object",
    "required": ["job"],
    "properties": {
      "job": { "type": "string", "description": "path to the resize job file (design, netlist, lib, STA config, sizing config)" },
      "out": { "type": "string", "description": "path to write the resized netlist (default: stdout)" }
    }
  },
  "artifacts": [ { "role": "netlist", "from_arg": "out" } ]
}
"#;
        print!("{DESCRIBE}");
        return;
    }

    let cli = parse_cli(&args);

    if cli.bug_report {
        return link("Report a bug (central — vyges/community)", BUG_URL);
    }
    if cli.feature_request {
        return link("Request a feature (central — vyges/community)", FEATURE_URL);
    }
    if cli.sponsor {
        return link("Sponsor Vyges", SPONSOR_URL);
    }
    if cli.star {
        return link("Star vyges-resize on GitHub ⭐", STAR_URL);
    }
    if cli.version {
        println!("vyges-resize {} ({})", vyges_resize::VERSION, env!("VYGES_GIT_SHA"));
        println!("{}", vyges_resize::COPYRIGHT);
        return;
    }
    let cmd = cli.positionals.first().cloned().unwrap_or_default();
    if cli.help || cmd.is_empty() {
        print!("{USAGE}");
        exit(if cmd.is_empty() && !cli.help { 2 } else { 0 });
    }

    match cmd.as_str() {
        "demo" => match run_demo() {
            Ok(r) => finish(r, &cli),
            Err(e) => {
                eprintln!("error: {e}");
                exit(1);
            }
        },
        "check" => {
            let Some(path) = cli.positionals.get(1) else {
                eprintln!("usage: vyges-resize check JOB");
                exit(2);
            };
            match ResizeJob::load(path) {
                Ok(j) => {
                    let obj = match j.cfg.objective {
                        Objective::Timing => "timing",
                        Objective::Area => "area",
                    };
                    println!(
                        "OK  design={} groups={} objective={obj} effort={} dont_touch={}",
                        j.sta.design,
                        j.cfg.groups.len(),
                        j.cfg.effort,
                        j.cfg.dont_touch.len()
                    );
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(2);
                }
            }
        }
        "run" => {
            let Some(path) = cli.positionals.get(1) else {
                eprintln!("usage: vyges-resize run JOB [-o OUT]");
                exit(2);
            };
            let job = match ResizeJob::load(path) {
                Ok(j) => j,
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(2);
                }
            };
            if cli.verbose {
                eprintln!("sizing {} ({} group(s), effort {})", job.sta.design, job.cfg.groups.len(), job.cfg.effort);
            }
            match engine::run(&job) {
                Ok(r) => finish(r, &cli),
                Err(e) => {
                    eprintln!("error: {e}");
                    exit(1);
                }
            }
        }
        other => {
            eprintln!("vyges-resize: unknown command {other:?}\n");
            print!("{USAGE}");
            exit(2);
        }
    }
}
