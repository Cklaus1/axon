# Axon compared

Honest positioning against the languages and sandboxing substrates Axon is most often measured
against. **Where a claim is measured, it says so and gives the number. Where it is a design
difference, it says that instead.** A comparison document is the easiest place in a repo to drift
into marketing; the guard used here is that every quantitative claim names its source.

---

## 1. The one-sentence positioning

Every mainstream language was designed for humans to write and other humans to review. Axon's premise
is that **the author is increasingly a model, and the reviewer increasingly cannot read everything**
— so the compiler, not trust, has to be the boundary.

That reframes what "safe" means. Rust's safety is about memory. Go's is about simplicity. Axon's is
about **authority**: what can this code reach, who authorised it, and can I reproduce exactly what it
did?

---

## 2. Against general-purpose languages

| | Axon | Rust | Go | C / C++ | Python | JS / TS |
|---|---|---|---|---|---|---|
| Memory safety | ownership, no GC (`own`/`ref`) | ownership + borrowck | GC | manual | GC | GC |
| Null | none — `Option<T>` | none — `Option<T>` | `nil` | `NULL` | `None` | `null`/`undefined` |
| Exceptions | none — `Result<T,E>` | none — `Result<T,E>` | error values | exceptions | exceptions | exceptions |
| Type inference | full HM | local | local | none/`auto` | gradual | gradual |
| Refinement types | **yes** (`T where p`) | no | no | no | no | no |
| Effect tracking | **yes** (row-polymorphic) | no | no | no | no | no |
| Capability sandbox in the *language* | **yes** (`@[contained]`) | no | no | no | no | no |
| Deterministic replay of a whole run | **yes** (`AXON_REPLAY`) | no | no | no | no | no |
| Compile-time execution | `comptime` | `const fn` | no | `constexpr` | no | no |
| Ecosystem | ~none | large | large | vast | vast | vast |
| Maturity | pre-1.0, one implementation | production | production | decades | decades | decades |

### Where Axon genuinely differs

**Capabilities are a language construct, not a deployment concern.** Everywhere else, "this code may
only read `./data`" is a container, a seccomp profile, or a code review. In Axon it is a type-checked
annotation, enforced transitively — a helper cannot launder the I/O one call away — and refused at
compile time with `E1001`. The runtime sandbox and the ambient `AXON_ALLOWED_EFFECTS` ceiling are
defence in depth behind it, not the mechanism.

**Whole-run determinism is a first-class feature.** `AXON_RECORD` journals every host interaction —
files, network, stdin, clock, subprocess, env — and `AXON_REPLAY` serves the run back from that
journal with the filesystem deleted and the environment stripped. Any departure is a *divergence*
that exits 11 and **the program cannot catch it**. Python and JS have nothing equivalent; the nearest
neighbours are record/replay debuggers (`rr`) that operate below the language, not within it.

**Refinement types without a solver requirement.** `fn clamp(n: i64) -> (i64 where _ >= 0)` is checked
statically where an SMT backend can prove it and at runtime otherwise (exit 6), in *both* engines
byte-identically. Rust has no refinements; Ada/SPARK and F* do, but require the prover.

### Where Axon loses, plainly

* **Ecosystem.** There is no crates.io. If your problem is solved by a library, every language in
  that table beats Axon outright.
* **Maturity.** One implementation, pre-1.0, no stability guarantee.
* **Performance.** Native codegen exists and works, but the default and reference path is a
  tree-walking interpreter. Nothing here competes with Rust or C on throughput.
* **Fluency.** See §3 — this is measured, and it is the honest weak point.

---

## 3. The fluency result (measured, and it cuts both ways)

Axon was benchmarked as an RLM code-execution surface against other languages, scoring **tasks
completed out of 8**:

| Condition | Axon | Lua | Python |
|---|---|---|---|
| Zero-shot (no primer) | **0/8** | 0–1/8 | 8/8 |
| README-derived primer | 3/8 | — | — |
| Purpose-built language card | **7/8** | 6/8 (primed) | — |

**Read this honestly in both directions.** Zero-shot 0/8 is real: a model has essentially never seen
Axon, and a boundary the model cannot write code for is a boundary that does not exist. But a
language card is a fixed, one-time cost in a system prompt, amortised across a session — and with
one, Axon reaches 7/8, above primed Lua.

The interesting failure mode found along the way was **advice-blindness**: for the remaining failure
the compiler emitted a perfect diagnostic — code, location, and the fix — and the repair round still
did not apply it. A *prompt-side* fix (the card) beat an *error-side* fix (better diagnostics) on the
same defect. That is worth knowing before investing in diagnostics as a fluency lever.

A second measured result cuts against naive statefulness: on a stateful task set, Python's kernel
scored **3/5 against a stateless control's 5/5**, because the model reused a binding and guessed its
shape wrong — *a name carries no type*. Axon's accumulating session (`axon session`) is the direct
answer: every prior binding is re-type-checked before the new cell runs, so using one at the wrong
type is a compile error before anything executes.

> **Caveat, stated because it matters.** The arm-by-arm benchmark sweeps were **stopped on
> 2026-09-08**: the original thesis came out unsupported and a stateless control *won* the error
> column, which means that metric was measuring structure rather than reuse. The fluency numbers
> above (0/8 → 3/8 → 7/8) are from the earlier card measurements and replicated across three runs;
> the *arm comparison* numbers are not quoted here because they did not survive scrutiny. Do not cite
> anything from that harness without re-reading `AXON_FOR_RLM.md` and the notes in `tasks/`.

---

## 4. Against sandboxing and isolation substrates

Axon's containment story overlaps with — and is deliberately layered *with* — conventional isolation.

| | Axon `@[contained]` | seccomp / Landlock | gVisor | Firecracker / Kata | WASM (wasmtime/WASI) | Docker |
|---|---|---|---|---|---|---|
| Enforcement point | **compile time** + runtime | kernel syscall filter | userspace kernel | hardware virt | runtime host calls | namespaces/cgroups |
| Granularity | per **function** | per process | per process | per VM | per module | per container |
| Can express "this fn may read `./data` only" | **yes** | path-based (Landlock) | partial | no | via preopens | no |
| Network host allowlist | **yes** (`net: ["*.x.io"]`) | **no** (ABI 3 cannot) | yes | yes | no | partial |
| Failure mode | build error `E1001` | runtime `EPERM` | runtime error | runtime | trap | runtime |
| Attests what ran | yes (R31/R34 chain) | no | no | yes (with TEE) | no | image digest only |
| Overhead | zero (static) | ~zero | moderate | ~125 ms boot | low | low |

**The row that matters is `net`.** In the measured containment comparison, Axon filled the network
allowlist row that **Landlock ABI 3 structurally could not** — a host-level allowlist is simply not
expressible there. That is not a benchmark artefact; it is a capability difference.

**These are complements, not rivals.** Axon's own stack uses both: `@[contained]` is the compile-time
layer, and `axon-vm` (R26) runs programs inside a **confidential microVM** with attestation, so a
deployment gets language-level authority control *and* hardware isolation. `AXON_VM.md` and
`governance/specs/R26-confidential-microvm-substrate.md` cover that substrate; R31 extends the
attestation chain and R34 rolls the measurement forward as each program loads, so an auditor can
reconstruct exactly what ran in what order.

### The honest limit of compile-time containment

`@[contained]` binds code that went through *this* compiler. It says nothing about a native FFI call
(`R13` explicitly downgraded invariant I-11 from *total* to *edge-enforced + trusted-within* for
exactly this reason, and carries a TCB-delta ledger), and nothing about a bug in the checker itself.
That is why the runtime sandbox, the audit ledger, and the microVM exist: each layer assumes the one
above it can fail.

---

## 5. When to choose Axon, and when not to

**Reasonable to choose it when** the code is model-authored and consequential; when you need to prove
after the fact exactly what a run touched; when "this function may not reach the network" has to be a
compile error rather than a policy document; or when you want capability attenuation (`principal_mint`)
as a first-class runtime notion.

**Do not choose it when** you need libraries, throughput, hiring, or stability. Nothing in §2's right-hand
columns is a small gap, and none of them will close soon.

---

## 6. Sources

Every number above traces to one of: `AXON_FOR_RLM.md` (fluency measurements and their caveats),
`governance/specs/R26-confidential-microvm-substrate.md` and `AXON_VM.md` (microVM), `R13-native-ffi.md`
(the I-11 downgrade), `R31`/`R34` (attestation chain), `ARCHITECTURE.md` (safety layering), and
`AXON_REFERENCE.md` (the generated surface). Where this document and a spec disagree, the spec wins —
and the disagreement is a bug in this file.
