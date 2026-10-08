//! Per-phase compile timing: `axon check --time-passes` / `axon build
//! --time-passes` (AX-36).
//!
//! Modelled on `rustc -Z time-passes` and `tsc --extendedDiagnostics`: every
//! front-end phase and, for `build`, every codegen stage records its wall time,
//! and the driver prints one line per phase plus the total to stderr:
//!
//! ```text
//! time: parse 17.104
//! time: resolve 3.215
//! …
//! time: total 61.512
//! ```
//!
//! The format is the contract: `time: <phase> <milliseconds>`, phase names
//! never contain whitespace, milliseconds always carry three decimals, phases
//! appear in the order they first ran, and `total` is last. A phase that runs
//! more than once (a parse per file) is summed into one line. `check --json
//! --time-passes` prints the same data as one `axon-time-passes/1` JSON object
//! instead.
//!
//! Phases are LEAVES that do not nest, so their sum accounts for the total; the
//! difference is driver glue between phases. Off by default and then free: the
//! recorder is one relaxed atomic load per phase, with no clock read, lock,
//! allocation or output.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State {
    start: None,
    phases: Vec::new(),
});

struct State {
    start: Option<Instant>,
    phases: Vec<(&'static str, Duration)>,
}

fn state() -> std::sync::MutexGuard<'static, State> {
    // A panic while holding the lock leaves plain data behind; keep timing.
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Start recording. The `total` reported later is measured from here.
pub fn enable() {
    let mut s = state();
    s.start = Some(Instant::now());
    s.phases.clear();
    ENABLED.store(true, Ordering::Relaxed);
}

/// Whether phases are being recorded.
#[inline]
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Run `f` as phase `name`, recording its wall time when timing is enabled.
#[inline]
pub fn time<R>(name: &'static str, f: impl FnOnce() -> R) -> R {
    if !enabled() {
        return f();
    }
    let t = Instant::now();
    let r = f();
    record(name, t.elapsed());
    r
}

/// Add `d` to phase `name` (created at the end of the list on first use).
pub fn record(name: &'static str, d: Duration) {
    if !enabled() {
        return;
    }
    let mut s = state();
    match s.phases.iter_mut().find(|(n, _)| *n == name) {
        Some((_, acc)) => *acc += d,
        None => s.phases.push((name, d)),
    }
}

/// Consecutive phases through straight-line code: each [`Laps::lap`] records
/// the time since the previous lap (or [`Laps::start`]) under its name, so the
/// laps tile the region with no gaps. Inert (no clock read) when timing is off.
pub struct Laps(Option<Instant>);

impl Laps {
    #[inline]
    pub fn start() -> Self {
        Laps(enabled().then(Instant::now))
    }

    #[inline]
    pub fn lap(&mut self, name: &'static str) {
        if let Some(t) = &mut self.0 {
            let now = Instant::now();
            record(name, now - *t);
            *t = now;
        }
    }
}

/// The recorded phases (first-run order) and the total since [`enable`].
#[derive(Debug, Clone)]
pub struct Report {
    pub phases: Vec<(&'static str, Duration)>,
    pub total: Duration,
}

/// Snapshot the recording, or `None` when timing is off.
pub fn report() -> Option<Report> {
    if !enabled() {
        return None;
    }
    let s = state();
    Some(Report {
        phases: s.phases.clone(),
        total: s.start.map(|t| t.elapsed()).unwrap_or_default(),
    })
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

impl Report {
    /// One `time: <phase> <ms>` line per phase, then `time: total <ms>`.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for (name, d) in &self.phases {
            out.push_str(&format!("time: {name} {:.3}\n", ms(*d)));
        }
        out.push_str(&format!("time: total {:.3}\n", ms(self.total)));
        out
    }

    /// The `axon-time-passes/1` JSON object (one line, no trailing newline).
    pub fn json(&self, command: &str) -> String {
        let phases: Vec<String> = self
            .phases
            .iter()
            .map(|(name, d)| format!("{{\"name\":\"{name}\",\"ms\":{:.3}}}", ms(*d)))
            .collect();
        format!(
            "{{\"schema\":\"axon-time-passes/1\",\"command\":\"{command}\",\"phases\":[{}],\"total_ms\":{:.3}}}",
            phases.join(","),
            ms(self.total)
        )
    }
}

/// Print the report to stderr (text, or JSON when `json`). No-op when off.
pub fn emit(command: &str, json: bool) {
    let Some(r) = report() else { return };
    if json {
        eprintln!("{}", r.json(command));
    } else {
        eprint!("{}", r.text());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_json_formats_are_stable() {
        let r = Report {
            phases: vec![
                ("parse", Duration::from_micros(17_104)),
                ("resolve", Duration::from_micros(3_215)),
            ],
            total: Duration::from_micros(21_000),
        };
        assert_eq!(
            r.text(),
            "time: parse 17.104\ntime: resolve 3.215\ntime: total 21.000\n"
        );
        assert_eq!(
            r.json("check"),
            "{\"schema\":\"axon-time-passes/1\",\"command\":\"check\",\"phases\":[\
             {\"name\":\"parse\",\"ms\":17.104},{\"name\":\"resolve\",\"ms\":3.215}],\
             \"total_ms\":21.000}"
        );
    }
}
