# R25's evidence gate cannot pass — a real codegen defect, verified

`governance/REQUIREMENTS.md:70` cites `scripts/zephyr_qemu_gate.sh` as the
evidence that R25 landed: *"an Axon program runs AS a Zephyr application on an
ARM Cortex-M target, verified headlessly under QEMU."*

That gate cannot pass on this host, and the reason is a compiler defect rather
than a missing toolchain.

## The defect

`crates/axon-core/src/codegen/expr.rs`, `synthesize_freestanding_trap` (~782):

    let outb_fn_ty = void_ty.fn_type(&[i16_ty.into(), i8_ty.into()], false);
    let outb_asm = self.ir.context.create_inline_asm(
        outb_fn_ty,
        "outb $1, $0".to_string(),
        "{dx},{al},~{memory}".to_string(),
        …
    );

`outb` is x86 port I/O and `{dx}` / `{al}` are x86 register names. There is no
target check anywhere in the function — it emits this for EVERY
`--freestanding` build, including `thumbv7m`, where those registers do not
exist:

    error: couldn't allocate input reg for constraint '{dx}'

## How it was established

Two independent routes, from opposite directions:

1. **Symptom.** A subagent ran the gate in a fresh worktree with a
   codegen-capable `axon`. Every skip guard passed — `west`, `cmake`, `ninja`,
   `qemu-system-arm`, `ZEPHYR_BASE` and the `arm-zephyr-eabi` SDK are all
   present on this host — and the ARM build then died with the error above.
2. **Mechanism.** Reading the function confirms unconditional x86 asm and the
   absence of any target guard in the surrounding lines.

A third attempt of mine produced a NON-result and is recorded so it is not
mistaken for a contradiction: running the gate in a worktree whose `axon` was
built `--no-default-features` SKIPPED at the codegen guard, exit 0,
`SKIP: axon binary lacks codegen support`. That says nothing about the defect.
A skip is not a pass and is not a refutation either.

## What this means for the requirement

R25 is recorded as landed on the strength of a gate that has never been able to
pass. This is distinct from the orphaned-gate class also found this session: an
orphan is a working check nobody runs, whereas this is a check nobody could
have run successfully. Both produce the same reader experience — a requirement
that looks verified — from opposite causes.

Wiring the gate is therefore blocked. Wiring it today would turn `gate.sh` red
for a real reason, which is honest but not useful while the underlying defect
is unfixed; leaving it unwired keeps a false "verified" reading in the register.
Neither is acceptable as an endpoint, so the defect is being fixed rather than
the gate being labelled around.

## Status

A dedicated lane is repairing `synthesize_freestanding_trap` to be
target-aware: x86 keeps `outb` (the QEMU debug-exit port), ARM Cortex-M gets an
appropriate halt (`bkpt`/`wfi`), selected from the target triple. Its bar is
end-to-end: the verbatim failure before, the verbatim success after,
`qemu_boot_test.sh` unregressed on x86, and a mutation forcing the wrong branch
to prove the dispatch actually picks.

Until that lands, R25's status in `REQUIREMENTS.md` is **unsupported by its
cited evidence**, and this file is the record of why.
