# CVE-Bench × Axon — Triage of 40 critical real-world CVEs

Source: **CVE-Bench** (ICML 2025, `security/pentest/cve-bench`) — 40 critical-severity
CVEs from the NVD, each a real web-application vulnerability with a working exploit,
scored on 8 attack outcomes: DoS, File Access, RCE, DB Modify, DB Access, Admin Login,
Privilege Escalation, Outbound Service.

**The claim being tested:** for how many of these does Axon's capability system make the
CVE's *critical impact* unreachable — not by fixing the bug, but by denying the
authority the bug needs to matter (least privilege, enforced at compile time)?

## The precise framing

Axon does not detect the parsing/validation flaw. It removes the *ambient authority*
the flaw exploits. A path-traversal bug that can only read files the program was granted
is not a critical file-disclosure CVE. An SSTI bug in code that cannot `exec` is not RCE.
The bug remains; its impact class is gone. The honest WEAK column (logic/auth/output
bugs Axon can't help with) is what makes the STRONG column credible.

## Tally

| Bucket | Count | Meaning |
|---|---|---|
| **STRONG** | **19** | Critical impact (RCE / file access / file write/delete / SSRF / XXE / memory corruption) refused by `@[contained]` or eliminated by memory safety. |
| **MEDIUM** | **14** | SQL injection + missing-authz + injection-with-secondary-RCE: catchable with `Tainted<T>`→`Trusted` sink discipline or partially via the exec/net cap, not by `@[contained]` alone. |
| **WEAK** | **7** | Privilege-escalation logic, stored XSS, weak password hashing — app logic / output encoding, out of scope. |

**≈ half of 40 critical CVEs are a bug class Axon refuses by construction** — and a
disproportionate share of the STRONG set is in **AI tooling** (★).

## STRONG — impact refused by construction (19)

| CVE | Project | CWE | Impact | Axon refusal |
|---|---|---|---|---|
| CVE-2024-34359 ★ | llama-cpp-python | 76 | Jinja2 SSTI → RCE | `exec: none` |
| CVE-2024-5452 ★ | pytorch-lightning | 915 | deserialization → RCE | `exec: none` |
| CVE-2024-2359 ★ | lollms-webui | 78 | OS command injection → RCE | `exec: none` |
| CVE-2024-2624 ★ | lollms-webui | 29 | path traversal + file upload | `fs` prefix + `..`-deny |
| CVE-2024-4320 ★ | lollms-webui | 29 | path traversal → RCE | `fs` + `exec: none` |
| CVE-2024-3234 ★ | chuanhuchatgpt | 22 | path traversal (file read) | `fs` prefix |
| CVE-2024-36858 ★ | Jan | 434 | file upload → RCE | `fs` write + `exec: none` |
| CVE-2024-32964 ★ | Lobe Chat | 918 | SSRF | host-pinned `net:` |
| CVE-2024-4701 | Genie | 22 | path traversal → RCE | `fs` + `exec: none` |
| CVE-2024-25641 | Cacti | 20 | arbitrary file write | `fs` write allowlist |
| CVE-2024-5084 | Hash Form (WP) | 434 | arbitrary file upload | `fs` write allowlist |
| CVE-2024-31611 | SeaCMS | 404 | arbitrary file deletion | `fs` write allowlist |
| CVE-2024-32167 | Medicine Ordering | — | arbitrary file deletion | `fs` write allowlist |
| CVE-2024-4442 | Salon Booking (WP) | — | arbitrary file deletion | `fs` write allowlist |
| CVE-2024-22120 | Zabbix | 20 | command injection (audit) | `exec: none` |
| CVE-2024-36675 | LyLme_spage | 918 | SSRF | host-pinned `net:` |
| CVE-2024-32980 | Spin (WASM) | 610 | self-request SSRF | host-pinned `net:` |
| CVE-2024-37388 | lxml | 611 | XXE → file read / DoS | `fs` + `net` deny |
| CVE-2024-4323 | Fluent Bit | 122 | heap corruption → RCE/DoS | memory safety (no UB) |

## MEDIUM — catchable with taint/refinement, not `@[contained]` alone (14)

| CVE | Project | CWE | Impact | Axon angle |
|---|---|---|---|---|
| CVE-2024-3495 | CSC Dropdown (WP) | 89 | SQL injection | `Tainted<T>`→`Trusted` query sink |
| CVE-2024-3552 | Web Directory (WP) | 89 | SQL injection | taint sink |
| CVE-2024-36412 | SuiteCRM | 89 | SQL injection | taint sink |
| CVE-2024-36779 | Stock Mgmt | 89 | SQL injection | taint sink |
| CVE-2024-37831 | Payroll Mgmt | 89 | SQL injection | taint sink |
| CVE-2024-37849 | Billing System | 89 | SQL injection | taint sink |
| CVE-2024-5314 | Dolibarr | 89 | SQL injection | taint sink |
| CVE-2024-5315 | Dolibarr | 89 | SQL injection | taint sink |
| CVE-2024-4443 | Business Directory | — | time-based SQLi | taint sink |
| CVE-2024-2771 | Fluent Forms (WP) | 862 | missing capability check | authz as capability |
| CVE-2024-4223 | Tutor LMS (WP) | 862 | missing capability check | authz as capability |
| CVE-2024-3408 ★ | dtale | 798 | hardcoded key → auth bypass + RCE | RCE sink via `exec: none` (partial) |
| CVE-2024-32986 | PWAsForFirefox | 74 | property injection → exec | `exec: none` (partial) |
| CVE-2024-35187 | Stalwart Mail | 863 | post-ACE privilege gain | prevents the precondition (ACE) |

## WEAK — Axon does not meaningfully prevent (7)

| CVE | Project | CWE | Why out of scope |
|---|---|---|---|
| CVE-2023-37999 | HT Mega (WP) | 269 | privilege-escalation logic |
| CVE-2023-51483 | WP Frontend Profile | 269 | privilege-escalation logic |
| CVE-2024-30542 | WholesaleX (WP) | 269 | privilege-escalation logic |
| CVE-2024-32511 | Simple Registration (WP) | 269 | privilege-escalation logic |
| CVE-2024-34070 | Froxlor | 79 | stored XSS (output encoding) |
| CVE-2024-34716 | PrestaShop | 79 | XSS (output encoding) |
| CVE-2024-34340 | Cacti | 287 | weak password hashing (crypto) |

★ = AI tooling. The AI-stack CVEs (lollms-webui, llama-cpp-python, Lobe Chat,
pytorch-lightning, Jan, chuanhuchatgpt, dtale) cluster in STRONG — the headline for an
AI-first language: *real critical CVEs in the AI tools you already run, contained by
construction.*

## Reproductions

- `CVE-2024-34359/` — llama-cpp-python Jinja2 SSTI → RCE (built). `axon check` refuses
  the RCE / file-access / outbound payload (3× E1001). Run `./run.sh`.
- _next_: a path-traversal exemplar (lollms-webui CVE-2024-2624) and an SSRF exemplar
  (Lobe Chat CVE-2024-32964).
