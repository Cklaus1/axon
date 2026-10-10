//! C9 round 4c, ADMIT (amendment 76): direct tests of the supervisor's isolation guard.

use std::cell::Cell;
use std::path::{Path, PathBuf};

use axon_os::manifest::parse;
use axon_os::verdict::Verdict;
use axon_os::{
    supervise_requiring, Budget, DeclaredEffects, Grant, Isolation, IsolationRequirement,
    PrincipalHandle, RunOutcome, Runtime,
};

/// A runtime that states `iso` and records every call made to it. When it is
/// actually run it reports one `ran` event — the observable "effect".
struct Probe {
    iso: Isolation,
    declared: Cell<u32>,
    minted: Cell<u32>,
    ran: Cell<u32>,
}

impl Probe {
    fn new(iso: Isolation) -> Probe {
        Probe {
            iso,
            declared: Cell::new(0),
            minted: Cell::new(0),
            ran: Cell::new(0),
        }
    }
    fn calls(&self) -> (u32, u32, u32) {
        (self.declared.get(), self.minted.get(), self.ran.get())
    }
}

impl Runtime for Probe {
    fn isolation(&self) -> Isolation {
        self.iso
    }
    fn declared_effects(&self, _p: &Path) -> DeclaredEffects {
        self.declared.set(self.declared.get() + 1);
        axon_os::runtime::scan_effects("fn main() -> i64 { 0 }\n")
    }
    fn mint_principal(&self, _g: &Grant) -> PrincipalHandle {
        self.minted.set(self.minted.get() + 1);
        PrincipalHandle(0)
    }
    fn run_sandboxed(
        &self,
        _p: &Path,
        _h: &PrincipalHandle,
        _g: &Grant,
        _b: &Budget,
        _s: u64,
    ) -> RunOutcome {
        self.ran.set(self.ran.get() + 1);
        RunOutcome {
            events: vec![axon_os::RawEvent::new(
                "ran",
                "probe",
                Default::default(),
                "",
            )],
            verdict: Verdict::Completed { value: 0 },
        }
    }
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-os-hwiso-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn job(dir: &Path) -> axon_os::JobManifest {
    std::fs::write(dir.join("prog.ax"), "fn main() -> i64 { 0 }\n").unwrap();
    std::fs::write(
        dir.join("job.axjob"),
        "program = \"prog.ax\"\nintent = \"t\"\nseed = 1\nprofile = \"restricted\"\n\
         [grant]\nmax_label = \"internal\"\n\
         [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n",
    )
    .unwrap();
    parse(
        &std::fs::read_to_string(dir.join("job.axjob")).unwrap(),
        dir,
    )
    .unwrap()
}

/// C9 round 4c, ADMIT (amendment 76): LIBRARY tests of the supervisor's
/// isolation guard and of `satisfied_by`'s ProcessScoped arm (M1796, M1797). On
/// Fabric's route `backend::select` refuses such a request first (M1052), so
/// these two are reached only by a direct call of `supervise_requiring`, which
/// is what this does. A request requiring hardware isolation is refused on the
/// isolation axis, with the runtime never asked to run, when the runtime is a
/// plain process.
#[test]
fn a_hardware_isolation_requirement_is_never_met_by_a_process_scoped_runtime() {
    for required in [
        IsolationRequirement::HardwareIsolated,
        IsolationRequirement::MicroVm,
    ] {
        let d = tmp(&format!("process-{required:?}"));
        let m = job(&d);
        let rt = Probe::new(Isolation::ProcessScoped);
        let rec = supervise_requiring(
            &m,
            &d.join("job.axjob"),
            &m.grant.clone(),
            "rid-hw-process",
            required,
            &rt,
        );
        if rt.calls().2 != 0 || !matches!(rec.verdict, Verdict::Denied { .. }) {
            panic!(
                "ATTACK: a request requiring {required:?} ran on a process-scoped runtime: {:?}, {:?} runtime calls",
                rec.verdict,
                rt.calls()
            );
        }
        assert!(matches!(&rec.verdict, Verdict::Denied { axis, .. } if axis == "isolation"));
        let _ = std::fs::remove_dir_all(&d);
    }
}
