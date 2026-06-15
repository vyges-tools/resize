# vyges-resize

**STA-driven gate sizing**: a gate-level netlist in, a **resized netlist** out — drive
strengths chosen to close timing, scored by a real static-timing engine.

> **Vyges open EDA tools.** Commercial-grade silicon optimization, built on open standards
> and plain file formats — accessible to everyone, not only teams who can license a
> six-figure tool. `vyges-resize` is the first of the "close-timing" engines: where the
> analysis tools *say what's wrong*, this one *fixes it*.

## What it does

`vyges-resize` reads a netlist + Liberty + constraints and **picks a better drive strength
for each cell** — upsizing cells on critical paths to close setup violations, downsizing
cells with slack to recover area — then emits the resized netlist and a before/after timing
report. The logic never changes; only the cell variant does.

```text
  netlist + .lib + constraints ──[ vyges-resize ]──►  resized netlist  (+ before/after timing)
```

Every candidate is scored by the [`vyges-sta-si`](https://github.com/vyges-tools/sta-si)
timer, on an ordinary **CPU — no GPU, no CUDA**. It picks **sizes, not locations**: physical
placement / legalization / routing stay the flow's job. Run it **beside** the open flow —
size the synthesized netlist, then hand the better netlist to place-and-route, or run it as
an independent, legible second opinion.

## How it works

A keep-best loop over the timer: rank the critical path, try a stronger variant on each of
its instances (speculatively, via the timer's checkpoint/restore), commit the best move that
improves setup without breaking hold, and repeat until timing is met or no move helps — then
a downsizing pass recovers area where slack allows. Pure std Rust; fully unit-tested offline.

## The job

A `.resize` file is a superset of a `.sta` timing job, plus the resize knobs:

```text
design:     top
netlist:    top.v
lib:        tt.lib
spef:       top.spef               # optional — post-place ECO sizing (see below)
clock:      clk 1.2
input_slew: 0.02
output_load: 0.01
group:      INV  INV2  INV4        # interchangeable cells, weakest -> strongest (repeatable)
group:      NAND2 NAND2X2 NAND2X4
objective:  timing                 # timing | area  (default: timing)
effort:     medium                 # low | medium | high
dont_touch: clk_* *scan*           # instance-name globs to leave alone
```

The legal moves come from the `group:` families you declare (the drive-strength / Vt set from
your PDK). Nothing is foundry-confidential — the `.lib` already encodes delay, transition, and
power per variant.

## Use it

```sh
cargo build --release            # std-only (depends on the open vyges-sta-si timer)

vyges-resize run   top.resize -o sized.v          # size -> resized netlist
vyges-resize run   top.resize --json              # before/after report as JSON
vyges-resize run   top.resize --fail-on-violation # exit 3 if still violating (CI gate)
vyges-resize check top.resize                     # validate the job
vyges-resize demo                                 # size a built-in example (no files)
# common flags: -o FILE · --json · -q/--quiet · -v/--verbose · -h/--help · -V/--version
```

See [`examples/inv_chain.resize`](examples/inv_chain.resize) for a runnable example.

## Open core

`vyges-resize` is open and contains **no foundry-confidential data** — sizing is driven
entirely by the `.lib` (the legal variants and their delay/power) and the constraints. It
runs out of the box on open PDKs and on any PDK whose Liberty you have.

## Post-place ECO mode

Name a `spef:` in the job and sizing is scored against the **real interconnect** the placed
design presents — wire RC, effective capacitance, fan-out — instead of ideal nets. This is
where drive-strength sizing earns its keep: a gate driving a long, heavily-loaded net is the
one that actually needs a stronger variant, and that only shows up once the parasitics are in.
Run it as a post-place ECO: place → extract (SPEF) → `vyges-resize` → hand the resized netlist
back for legalization. Without a `spef:` it sizes pre-place against ideal interconnect; the
report states which mode it ran in.

## Status & bounds

v0 sizes a netlist → netlist on the variant families you declare; objective `timing` closes
setup WNS (and recovers area where slack allows), **pre-place** (ideal interconnect) or
**post-place** (with SPEF, above). It is **not** a place-and-route tool — it decides sizes and
hands physical realization back to the flow. Pre-place candidates are scored by a fast
cone-localized incremental timing pass; with SPEF each candidate is scored by a full timing
pass today (correct; the incremental score under parasitics lands behind the same loop). Sign-
off is still the golden timer — `vyges-resize`'s numbers are a fast, license-free guide.
