//! D-020 / B259 — `HardwareIsolated` is satisfied ONLY by the qualified
//! protected profile (`linux_microvm_protected`).
//!
//! An unqualified KVM microVM (no jailer, unpinned guest — B263) has hardware
//! virtualisation present but no evidence that the enclosure holds. It used to
//! satisfy `HardwareIsolated` through a `(HardwareIsolated, _) => true` arm.
//! Driven through the REAL supervisor with a runtime that counts every call,
//! so "refused" is shown to mean "nothing was asked of the runtime at all".

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

#[test]
fn hardware_isolated_is_refused_by_an_unqualified_kvm_microvm_with_no_effects() {
    let d = tmp("unqualified");
    let m = job(&d);
    let rt = Probe::new(Isolation::KvmMicroVmUnqualified);
    let rec = supervise_requiring(
        &m,
        &d.join("job.axjob"),
        &m.grant.clone(),
        "rid-hw-unqualified",
        IsolationRequirement::HardwareIsolated,
        &rt,
    );
    match &rec.verdict {
        Verdict::Denied { axis, reason } => {
            assert_eq!(axis, "isolation");
            assert!(reason.contains("kvm_microvm_unqualified"), "{reason}");
        }
        other => panic!("must be refused on the isolation axis, got {other:?}"),
    }
    // Absence of effects: the runtime was never consulted beyond isolation(),
    // and the record holds only the isolation denial — no `ran` event.
    assert_eq!(rt.calls(), (0, 0, 0), "refusal must not touch the runtime");
    assert_eq!(rec.events.len(), 1, "{:?}", rec.events);
    assert_eq!(rec.events[0].action, "denied");
    assert_eq!(rec.events[0].target, "isolation");
    assert!(rec.events.iter().all(|e| e.action != "ran"));
    let _ = std::fs::remove_dir_all(&d);
}

/// The contrast, so the refusal above is about the isolation level and not
/// the job: the SAME job under the qualified profile is admitted and run.
#[test]
fn hardware_isolated_is_satisfied_by_the_qualified_protected_profile() {
    let d = tmp("qualified");
    let m = job(&d);
    let rt = Probe::new(Isolation::LinuxMicroVmProtected);
    let rec = supervise_requiring(
        &m,
        &d.join("job.axjob"),
        &m.grant.clone(),
        "rid-hw-qualified",
        IsolationRequirement::HardwareIsolated,
        &rt,
    );
    assert_eq!(rec.verdict, Verdict::Completed { value: 0 });
    assert_eq!(rt.ran.get(), 1, "premise: an admitted job runs");
    let _ = std::fs::remove_dir_all(&d);
}
