# R23 — eBPF Compile Target (capability-typed, deterministic Axon → BPF bytecode)

**Spec ID:** `R23-ebpf-target` (new requirement row; depends on `R17-freestanding-substrate.md` for `@[no_alloc]`/E1704, `@[total]`/E1208, the inkwell object-emission seam, and the E0910 interpreter-refusal pattern)
**Status:** Slice 1 LANDED — Axon→eBPF backend emits a kernel-verifier-accepted `.bpf.o`.
**Risk class:** Additive (new opt-in target behind `--target bpf`; the default hosted/interp paths are untouched).
**Author / date:** cklaus, 2026-06-26

> **One-line scope:** compile a restricted, capability-typed, *deterministic-by-construction* Axon subset
> to eBPF bytecode the Linux verifier accepts. The novelty: the program's determinism is proven by Axon's
> EXISTING checks (`@[no_alloc]`→E1704 = no heap, `@[total]`/bounded-`for` = no unbounded loop, no recursion),
> and the set of BPF helpers it may call is a **capability allowlist** — an un-granted helper is a clean Axon
> compile error, NOT a kernel load-time verifier reject. The verifier rejecting at load is the worse UX; Axon
> refuses at check time.

---

### 1. Motivation

eBPF is the canonical "untrusted code, sandboxed by a verifier" target: a constrained instruction/stack model,
no unbounded loops, no heap, a fixed helper ABI. That is *exactly* the shape of Axon's R17 freestanding subset
(`@[no_alloc]`, `@[total]`, no recursion). So Axon can be a *front-end* whose own type/effect discipline makes
a program verifiable BEFORE it reaches the kernel — turning the BPF verifier from a gatekeeper into a redundant
double-check. The capability angle is the differentiator: the BPF **helper set** a program may call is an
allowlist gated at compile time, so privilege is granted by construction, not discovered at load.

### 2. Surface (what the user writes)

```axon
substrate                               // BPF programs are substrate-only

// A packet counter: an array map with one slot; each invocation bumps slot 0.
@[bpf(kind: socket_filter)]             // marks an eBPF program + its program type
@[no_alloc]                             // (implied by @[bpf]) no heap — E1704 enforces
fn count(ctx: i64) -> i64 {
    let v = bpf_map_lookup_elem(0, 0)   // map #0, key 0 -> value pointer (0 = NULL/miss)
    if v != 0 {
        bpf_map_value_add(v, 1)         // atomic-add 1 to the looked-up counter
    }
    0
}
```

- **`@[bpf(kind: K)]`** marks the fn as an eBPF program of type `K` (`socket_filter`, `xdp`, `tracepoint`,
  `kprobe`). The emitted ELF section name follows libbpf convention (`socket`, `xdp`, …).
- **One map, by convention (Slice 1):** a single `BPF_MAP_TYPE_ARRAY` (key=4, value=8, max_entries=1) named
  `axon_map`, referenced as map handle `0`. (Multi-map / map-config surface is deferred — §11.)
- The body is restricted: scalar `i64` arithmetic, `if`, `let`, bounded `for`, and the **allowlisted BPF
  helper builtins**. No heap, no recursion, no unbounded `while` — enforced by R17's existing checkers.

### 3. The capability allowlist (the novelty)

BPF helpers are a **capability allowlist**. Each allowed helper is an Axon builtin that lowers to a BPF
`call <helper_id>`. A program that names a helper NOT in the allowlist gets a clean Axon error (E2300) at
check time — it never reaches the kernel verifier. Slice-1 allowlist:

| Axon builtin | BPF helper | id | Effect row |
|---|---|---|---|
| `bpf_map_lookup_elem(map, key)` | `bpf_map_lookup_elem` | 1 | `{Bpf}` |
| `bpf_map_value_add(ptr, delta)` | (lowered to `atomicrmw add`) | — | `{Bpf}` |
| `bpf_ktime_get_ns()` | `bpf_ktime_get_ns` | 5 | `{Bpf}` |
| `bpf_get_smp_processor_id()` | `bpf_get_smp_processor_id` | 8 | `{Bpf}` |

The helper id is fixed in the emitted instruction (`call <id>`) — never a runtime value. Adding a helper to
the allowlist is a one-line table edit; until then, calling it is E2300.

### 4. Semantics (what it does)

| Input class | Behavior |
|---|---|
| `axon build --target bpf foo.ax` | emits a `.bpf.o` ELF object (`elf64-bpf`) at `--out` (default `foo.bpf.o`) |
| `@[bpf(kind: K)]` fn | lowered to a BPF function in the ELF section for `K`; `@[no_alloc]`+`@[total]` auto-required |
| `bpf_map_lookup_elem(0, k)` | a `lddw` of the map (`BPF_PSEUDO_MAP_FD` reloc on `axon_map`) + `call 1`; returns the value ptr as i64 (0 = miss) |
| `bpf_map_value_add(p, d)` | `atomicrmw add` on the value ptr — a race-free counter increment |
| a BPF helper NOT on the allowlist | **E2300** at check time — clean Axon error, never a verifier reject |
| `@[bpf]` fn with an unbounded `while` | **E1208** (`@[total]` is auto-required) — refused BEFORE codegen |
| `@[bpf]` fn that allocates (str/array/dict/interp) | **E1704** (`@[no_alloc]` auto-required) — refused BEFORE codegen |
| any BPF builtin under `axon run` | **E0910** — no kernel under the interpreter; build for `--target bpf` |
| a construct outside the BPF-lowerable subset | **E2301** at codegen — clean refusal, never emit bytecode that only sometimes verifies |

### 5. Determinism / capability angle (what is PROVEN)

- **Bounded:** `@[bpf]` auto-requires `@[total]`, so an unbounded `while` is E1208 (Axon refuses before the
  kernel ever sees it). Bounded `for`/recursion-free is the verifiable shape.
- **No heap:** `@[bpf]` auto-requires `@[no_alloc]`, so any heap touch (str/array/dict/interpolation, even
  transitively through a helper) is E1704.
- **Capability:** the helper set is the allowlist of §3. An un-granted helper is E2300 at check, not a kernel
  reject. This is the Axon capability model applied to the BPF helper ABI.
- **The kernel verifier is the redundant second check**, not the first: the Slice-1 example loads clean
  (real `BPF_PROG_LOAD`, verifier ACCEPTS).

### 6. Error codes (new E23xx block)

| Code | Trigger | Message shape |
|---|---|---|
| E2300 | a BPF helper not on the capability allowlist is called from a `@[bpf]` program | `BPF helper `bpf_probe_read` is not in the Axon capability allowlist; allowed: bpf_map_lookup_elem, …` |
| E2301 | a construct outside the BPF-lowerable subset appears in a `@[bpf]` body | `unsupported construct in @[bpf] program `count`: <kind> — eBPF cannot lower it` |
| E2302 | `@[bpf(kind: K)]` has an unknown program kind | `unknown @[bpf] kind `K`; supported: socket_filter, xdp, tracepoint, kprobe` |
| E0910 (reuse) | a BPF builtin is reached by the interpreter | `BPF builtin requires `axon build --target bpf`; no kernel under `axon run`` |

### 7. Invariants touched

- **I-11 (capability boundary is real and total): EXTENDED.** A new `Bpf` capability axis (the BPF helper
  allowlist), gated identically to `Net`/`Fs`/`Hal`. Strengthens I-11.
- **I-2 (interpreter is the reference oracle): preserved by refusal.** BPF builtins have no interpreter
  semantics (no kernel under `axon run`), so they E0910-refuse — exactly the R17 HAL relationship.
- No safe/surface code gains any BPF power; `@[bpf]` is substrate-only.

### 8. Acceptance criteria (the done gate)

- [x] `axon build --target bpf examples/bpf/counter.ax` emits a valid `elf64-bpf` `.bpf.o`.
- [x] **Real verification:** the in-kernel BPF verifier ACCEPTS it (direct `bpf(BPF_PROG_LOAD)` syscall,
      `scripts/bpfload.c` — `/usr/sbin/bpftool` is a broken per-kernel wrapper under this WSL2 kernel, so the
      direct syscall loader is the strongest verification that actually works here; see §10).
- [x] `scripts/ebpf_verify.sh`: builds the object, runs the kernel-verifier load, asserts ACCEPT; SKIP-guards
      if `clang -target bpf`/`llc` absent; asserts it actually ran (no vacuous pass).
- [x] **Adversarial:** an `@[bpf]` fn with an unbounded `while` → E1208 BEFORE codegen; a heap-touching one →
      E1704; an un-allowlisted helper → E2300; a BPF builtin under `axon run` → E0910.
- [x] `bash scripts/gate.sh` green; the default gate does NOT require clang-bpf (SKIP-guarded).

### 9. Performance budget

- The counter program ≤ 16 BPF instructions, ≤ 1 map, 0 heap, 0 unbounded loops (verifier processes ≤ 32
  insns). Guarded implicitly by the verifier's `processed N insns` line in the harness output.

### 10. Honest boundaries (what WSL2's kernel could / couldn't do)

- The in-kernel **verifier load works** (`bpf(BPF_PROG_LOAD)` ACCEPTS) — this is the strong, real check.
- `/usr/sbin/bpftool` is a stub wrapper that cannot find the per-kernel binary for `6.6.x-microsoft-WSL2`,
  so `bpftool prog load` silently no-ops. The direct `bpf()` syscall loader (`scripts/bpfload.c`) replaces it
  and does a genuine verifier round-trip (creates the map, resolves the `R_BPF_64_64` map relocation, patches
  `BPF_PSEUDO_MAP_FD`, then `BPF_PROG_LOAD`).
- **Attaching** the program to a live socket/XDP hook (running it on real traffic) needs hook plumbing that is
  awkward under WSL2 and is out of Slice-1 scope; the verifier ACCEPT is the load-bearing correctness gate.

### 11. Rollout & deferred

- Behind `--target bpf` only; default hosted/interp builds untouched.
- **Deferred:** multi-map / configurable map types & sizes (Slice 1 fixes one ARRAY map); BTF/CO-RE; richer
  context-struct field access (`__sk_buff`); `rbpf` userspace execution test (the kernel verifier load is the
  stronger check and works, so rbpf is unnecessary here but remains the fallback if a future kernel can't load).
