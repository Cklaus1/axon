# Coverage & Limits — the whole 40, honestly

[`TRIAGE.md`](TRIAGE.md) buckets all 40 CVE-Bench CVEs. This page is the companion a
security reviewer actually wants: for **every** class — including the ones Axon does
**not** prevent — the verdict and the reason. The honesty is the point. A tool that
claims to stop everything is not credible; one that draws its own edges is.

## The three verdicts

| Verdict | Meaning |
|---|---|
| **PREVENTED** | The CVE's critical impact is unrepresentable. The bug may remain; its consequence cannot occur. |
| **CONTAINED** | Axon does not prevent the bug, but the capability system bounds its blast radius. |
| **OUT OF SCOPE** | A type/capability system cannot address this class. Use the appropriate tool. |

## By class

| CWE / class | Count | Verdict | Mechanism / why |
|---|---|---|---|
| Path traversal (22/29) | 4 | **PREVENTED** | `@[contained(fs:[…])]` prefix allowlist + `..`-deny + dynamic-path refusal |
| Arbitrary file write/upload/delete (434/404/20) | 6 | **PREVENTED** | `fs: [write(…)]` allowlist; out-of-lane / dynamic paths refused |
| OS command injection → RCE (78/20) | 2 | **PREVENTED** | `exec: none` — no spawn sink |
| Template/deserialization → RCE (76/915) | 2 | **PREVENTED** | `exec: none` — the injected payload has no exec authority |
| SSRF / outbound (918/610) | 3 | **PREVENTED** | host-pinned `net: ["host"]` — seccomp cannot express this |
| XXE → file/SSRF (611) | 1 | **PREVENTED** | `fs`/`net` deny — the external-entity fetch has no authority |
| Memory corruption (122) | 1 | **PREVENTED** | memory-safe runtime — class eliminated |
| **SQL injection (89)** | **9** | **PREVENTED (structure) / escaped (data)** | **`sql_query` requires a literal template (E1210), so the query's STRUCTURE cannot be attacker-controlled — that half is a compile error.** The bound-parameter DATA is escaped at render time, and until 2026-08-04 that escaping doubled `'` only: a param of `\` consumed its closing quote on MySQL/MariaDB and handed the rest of the query to the attacker (P5-25 / T39, reproduced). Backslash is now doubled too. Note the escaping targets MySQL rules and is NOT dialect-neutral — see the `sql_query` doc. See `CVE-2024-5314/`. |
| Missing authorization (862) | 2 | **CONTAINED** | Axon doesn't add the missing check, but a `@[contained]` handler that escalates still can't exfiltrate. Modeling authz as a capability is future work. |
| Hardcoded-secret / property injection → RCE (798/74) | 2 | **CONTAINED** | the RCE sink is killed by `exec: none`; the hardcoded-credential / auth-bypass logic itself is not detected |
| Post-RCE privilege gain (863) | 1 | **CONTAINED** | Axon prevents the *precondition* (the arbitrary code execution); the escalation logic isn't modeled |
| Privilege-escalation logic (269/862) | 6–7 | **OUT OF SCOPE → CONTAINED** | a missing role check is application logic; `@[contained]` still caps the fallout. Demonstrated in [`CVE-2024-2771/`](CVE-2024-2771/): the escalation *runs*, but weaponizing it is 3× E1001 |
| Stored / reflected XSS (79) | 2 | **OUT OF SCOPE** | output-context encoding — a web-framework/templating concern, not a capability one |
| Weak password hashing (287) | 1 | **OUT OF SCOPE** | cryptographic choice — a type system can't pick a strong KDF for you |

## The summary you can defend

> Of 40 critical real-world CVEs: **~28 PREVENTED** (capability confinement for ~19, plus
> the 9 SQL-injection CVEs now a compile error), a handful **CONTAINED** (the bug fires
> but can't exfiltrate or RCE), and ~7 honestly **OUT OF SCOPE** (auth/role logic, XSS
> output encoding, crypto choice). For the out-of-scope set, the capability system still
> **caps the blast radius** — a logic bug in a `@[contained]` component has no authority
> to turn a foothold into exfiltration.

Two distinct guarantees do the work, and it's worth keeping them separate:

1. **Confinement** (`@[contained]`) — *"this code may only touch what it declared."*
   Makes RCE/file/SSRF/XXE impact unreachable, and caps the blast radius of everything else.
2. **Unrepresentability** (`sql_query`/E1210) — *"the unsafe construction does not
   typecheck."* Makes the *structural* half of SQL injection a compile error rather than a
   discipline: a template built by concatenation or interpolation does not compile, so an
   attacker cannot supply query STRUCTURE. The remaining half — escaping the bound DATA —
   is ordinary, dialect-specific string handling, and it was wrong until T39 (a lone `'`
   doubling let a backslash escape its own closing quote on MySQL). Worth stating plainly:
   unrepresentability is a real and strong property, but it covered less of this CWE than
   the single word "PREVENTED" implied.

And where neither applies — a logic bug the compiler can't see — **confinement still
caps the blast radius**: [`CVE-2024-2771/`](CVE-2024-2771/) makes this concrete, a
privilege escalation that *runs* but whose foothold has no authority to read secrets,
exfiltrate, or RCE (3× E1001). Bug yes; damage no.

Neither guarantee claims to find the bug. Both make whole impact classes impossible. The
[`THREAT_MODEL.md`](../THREAT_MODEL.md) states what even these do **not** cover (covert
channels, a malicious compiler, the software-TPM stand-in, host 0-days, DoS).
