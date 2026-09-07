# R43 — Bytes and binary data

Status: DRAFT
Depends on: R19 (unsigned integer types, landed), R42 (encoding builtins, landed)
Reserves error codes: E2213–E2217 (E2205–E2212 are held by R42; E2205 is now ALLOCATED
to the `re_replace_all` group-reference refusal, so R43 starts above R42's range)
Supersedes nothing. Unblocks: hashing, compression, binary file I/O, network protocols.

---

## 1. Why this spec exists — and why "add a bytes type" is the wrong framing

The R42 wrap-up named "no bytes type" as Axon's deepest stdlib gap. That framing was
wrong, and finding out why took ten minutes of reading the code rather than trusting
the summary:

* `u8` is a real type and has been since R19 landed unsigned integers through
  codegen (`unsigned_parity.sh` is green).
* `Type::Slice(Box<Type>)` already exists and lowers to `{ i64 len, ptr data }`, so
  **`[u8]` is expressible today** — `fn take(xs: &[u8]) -> i64 { len(xs) }`
  type-checks, and `65 as u8` is a valid cast.

So Axon has a byte-sequence *type*. What it does not have is

1. a **runtime representation** that is not catastrophically wasteful,
2. **builtins that produce or consume one**, and
3. any way to write one down conveniently.

Item 2 is the real gap and the only one that blocks anything today. Item 1 is a
performance property that becomes a correctness property at scale. Item 3 is
ergonomics and is deliberately deferred (§7.5).

### 1.1 The capability hole this closes, precisely

`base64_decode` and `hex_decode` currently **refuse** any input whose decoded bytes
are not valid UTF-8:

```
{name}: E2204 decoded bytes are not valid UTF-8 (Axon has no bytes type ...)
```

That is not a conservative choice, it is a missing capability wearing a refusal's
clothes: **arbitrary base64 cannot be decoded in Axon at all.** Since base64 exists
almost entirely to carry non-text — keys, images, signatures, compressed blobs —
the decoder is unavailable for its primary purpose. Hashing has the same shape: a
hash consumes arbitrary bytes, and there is nothing to hand it.

This is the measured-need argument R42's admission test demands. It is not "other
languages have bytes".

---

## 2. The admission test, applied honestly

R42's rule: an addition is admissible only if a measured failure traces to its
absence AND it either cannot be written in userland Axon or is a soundness fix.

**Can a byte sequence be written in userland today?** Yes — `[i64]` with a
0..=255 convention, or `[u8]` with `as u8` casts. So a *userland* bytes module is
admissible and a *builtin type* is not, on the strength of expressiveness alone.

**So what justifies runtime work?** Two things that userland cannot reach:

* **The decode refusal above.** `base64_decode` is a builtin; only a builtin can
  change what it returns. No userland module can make it emit non-UTF-8 bytes.
* **Memory.** In the interpreter a `Value` is a large tagged union, so a byte per
  `Value::Int` costs on the order of 24–32 bytes. A 10 MB file read as `[i64]`
  is **240–320 MB of live heap** — and this language runs model-authored code
  inside a sandbox with a memory ceiling. A representation that turns a 10 MB
  read into a 300 MB allocation is a denial-of-service surface, not an
  inefficiency. `Vec<u8>` makes the same read 10 MB.

The second point is why this spec adds a runtime representation rather than
shipping a userland `bytes.ax` over `[i64]`. It is the same reasoning that made the
regex engine's linear-time bound a containment property in R42 §6.

---

## 3. Design: a new `Value` variant, but NO new `Type` variant

```
runtime:  Value::Bytes(Vec<u8>)          // new, compact
surface:  [u8]  ==  Type::Slice(Box<Type::U8>)   // already exists, unchanged
```

This is the whole design decision, and it is deliberately asymmetric. The static
side needs nothing: `hex_decode(s) -> Result<[u8], str>` is expressible with
today's `Type` enum, so `infer.rs`, `checker.rs` and `codegen/types.rs` need no new
type case. The dynamic side gets one variant so a byte stays a byte in memory.

Empirical blast radius, from the two comparable precedents in this repo:

| Change | Files | Lines | Notes |
|---|---|---|---|
| R21 `Value::Decimal(i128)` + `Type::Decimal` + literal syntax | 21 | ~760 | included `token.rs`/`parser.rs` for `1.50d` literals |
| R19 `Value::SizedInt { val, ty }` | — | — | explicitly isolated blast radius by leaving `Int(i64)` alone |

R43 should land **smaller than R21** because it skips `Type`, `token.rs` and
`parser.rs` entirely (no literal syntax — §7.5). The cost is concentrated in
`interp.rs` + `interp/builtins.rs`, plus the mandatory registration sites.

`Value::Array` is matched at 119 sites and `Value::Str` at 192, most behind `_`
fallbacks. The risk is not the count, it is that a `_` arm silently treats
`Bytes` as "some other value" and produces a wrong answer instead of an error.
See §6.2 — this is the single most likely way this spec ships a bug.

### 3.1 Indexing and length must agree with the surface type

`bs[0]` where `bs: [u8]` must evaluate to `Value::SizedInt { val, ty: U8 }`, not
`Value::Int`, because the static type says `u8` and R19 established `SizedInt` as
the representation for non-i64 widths. `len(bs)` returns the byte count.

There is a real trap here: `str_len` counts BYTES and `str_len_chars` counts
CHARACTERS (R42 Slice 2). `len` on `[u8]` is a byte count by definition, so
`len(str_to_bytes(s)) == str_len(s)` must hold for every `s`, and a test must
assert it on non-ASCII input. If those two ever disagree, one of them is lying
about what a byte is.

---

## 4. Slice 1 — the representation and the string boundary

```
str_to_bytes(s: str) -> [u8]                 // UTF-8 encoding of s, never fails
bytes_to_str(b: [u8]) -> Result<str, str>    // E2213 when not valid UTF-8
                                             // NO bytes_len — `len` already works
bytes_slice(b: [u8], start: i64, end: i64) -> [u8]
bytes_concat(a: [u8], b: [u8]) -> [u8]
bytes_eq(a: [u8], b: [u8]) -> bool
```

`bytes_to_str` returns `Result`, not a panic: decoding foreign data is an expected
failure, and R42 Slice 3 established that malformed-input builtins return `Err`
values rather than aborting.

**`bytes_len` is redundant and should not ship.** Verified against the real
compiler rather than assumed — this program runs today and prints `3 / 3 / 66`:

```axon
fn take(xs: &[u8]) -> i64 { len(xs) }

fn main() -> i64 {
    let bs = [65 as u8, 66 as u8, 67 as u8]
    println(to_str(len(bs)))      // 3
    println(to_str(take(&bs)))    // 3 — passes as &[u8]
    println(to_str(bs[1]))        // 66
    0
}
```

So `len`, borrowing and indexing over `[u8]` already work end to end. The only
thing missing from that program is a way to obtain the bytes from something other
than hand-written casts. (Q3 resolved: `len` alone.)

`bytes_slice` slices on byte indices with no boundary rule — unlike `str_slice`,
which now refuses splitting a UTF-8 character (R42 Slice 1, E2200). That
asymmetry is correct and must be stated in the doc string: bytes have no
character structure to violate. This is the *point* of having bytes.

## 5. Slice 2 — close the decode refusal

```
base64_decode_bytes(s: str) -> Result<[u8], str>
hex_decode_bytes(s: str) -> Result<[u8], str>
base64_encode(b: [u8] | str) -> str     // polymorphic; see below
hex_encode(b: [u8] | str) -> str
```

The existing `base64_decode`/`hex_decode` keep their `Result<str, str>` signatures
and their E2204 refusal, because changing a shipped builtin's return type breaks
callers. The `_bytes` variants are the ones that can decode anything.

For the encoders, **make the existing names polymorphic over `str | [u8]`** rather
than adding `_bytes` twins. Precedent: `to_str` is already polymorphic over
scalars. A caller who has bytes and must remember a different function name for
them will guess wrong, and `hex_encode` on bytes is the overwhelmingly common
case.

**This must not silently succeed on the wrong thing.** `base64_encode(b)` where
`b` is a `[u8]` and `base64_encode(s)` where `s` is a `str` must produce the
same output for `str_to_bytes(s)` — a differential test, not two independent
tests.

## 6. Slice 3 — binary file I/O

```
file_read_bytes(path: str) -> Result<[u8], str>
file_write_bytes(path: str, b: [u8]) -> Result<i64, str>
file_append_bytes(path: str, b: [u8]) -> Result<i64, str>
```

### 6.1 Capability classification is NOT optional here

R42 T0 widened `classify_call` to return `Vec<(IoKind, usize)>` because enforcement
reads only the argument at a recorded index. Every builtin above needs a row:

```
"file_read_bytes"   => FsRead  at arg 0
"file_write_bytes"  => FsWrite at arg 0
"file_append_bytes" => FsWrite at arg 0
```

A missing row is not a missing feature — it is a **sandbox escape**, because an
unclassified path argument is never checked against the `@[contained]` allowlist.
The R42 exfiltration test (`write_only_fn_cannot_file_copy_out_of_a_read_denied_path`)
must gain a sibling for the bytes readers, and it must FAIL before the rows are
added.

### 6.2 The `_` fallback hazard, stated as a required check

Adding a `Value` variant is dangerous precisely because the compiler will not tell
you where you needed to handle it. Before this slice is called done:

* grep every `Value::Str(` and `Value::Array(` match in `interp/builtins.rs` whose
  arm ends in `_ =>` and decide, per site, whether `Bytes` belongs there;
* confirm `value_type_tag` names `Bytes` (R42 learned the hard way that an error
  path rendering an unknown value with `{:?}` can stack-overflow);
* confirm `to_str`/`println` on a `[u8]` either works deliberately or refuses —
  never prints a Rust `Debug` rendering into a user's stdout.

## 7. Slice 4 — hashing, and the ownership question this resolves

```
sha256_hex(b: [u8] | str) -> str
sha256_bytes(b: [u8] | str) -> [u8]
hmac_sha256_hex(key: [u8] | str, msg: [u8] | str) -> str
```

R42 left hashing as `needs-human` for a stated reason: **R28, R33 and R42 all
wanted to own `sha256`, and shipping it under one of them creates two answers.**
This spec claims it, and the argument is now stronger than "someone must":
a hash consumes arbitrary bytes, so hashing *belongs* wherever bytes are defined.
Any other home would have to define bytes again.

Implementation must be hand-written or a single vetted dependency — this is TCB
surface. A hash that is subtly wrong is worse than no hash, because callers build
integrity checks on it. Test against published vectors (the empty string, "abc",
and the 1-million-`a` case), not self-consistency: a wrong implementation is
perfectly self-consistent.

### 7.5 Deferred: byte-string literals

No `b"..."` syntax in this spec. Rationale:

* it costs `token.rs` + `parser.rs` + `fmt.rs` (the R21 literal work), and
* every construction path that matters is a *conversion* — from a `str`, a file,
  base64, or hex — not a literal a human types.

The exception worth noting: the **session dump**. `AXON_DUMP_BINDINGS` writes
module-level bindings back as Axon literals so the next RLM cell can re-bind
them, and `Value::Bytes` needs a spelling there or a bytes binding breaks the
session. `hex_decode_bytes("...")` returns a `Result`, so the dump would have to
emit a `match` — unacceptable. See Q1.

---

## 8. Cross-cutting requirements

1. **Registration at all four sites** for every new builtin, per CLAUDE.md:
   `BUILTINS`, the interpreter dispatch, capability classification where a path or
   host resource is involved, and `wasm_parity.sh`'s `HOST_BUILTINS`.
2. **Codegen refuses, and does not diverge.** `Value::Bytes` is interp-only in
   this spec; every builtin here must E0910-refuse under `axon build`, with a test
   per group asserting the refusal names the builtin (I-2, sound-by-refusal).
   Note this is *temporary by construction*: `[u8]` is already a lowerable type,
   so a later slice can lower these natively without a surface change.
3. **`value_as_literal` handles `Bytes`** or the session dump regresses (Q1).
4. **E2213–E2217 registered** in `error.rs` as `pub const` and added to the ALL
   list — R42 shipped this wrong and the guard test caught it.
5. **Expected-VALUE gates, not agreement gates.** R42's Q6 lesson: `fuzz_parity.sh`
   compares interp against native and is structurally blind to a bug they share.
   Every slice here needs at least one row asserting a *known-correct constant*
   (published hash vectors, a known base64 blob), not just cross-engine agreement.

---

## 9. Open questions

**Q1 — how does the session dump spell a `[u8]`?** `hex_decode_bytes` returns
`Result`, so the dump cannot emit it bare. Options: (a) an infallible
`bytes_unhex(s) -> [u8]` that panics on malformed input, documented as the dump's
inverse and never meant for hand use; (b) emit an array literal of `as u8` casts,
which is unusable for anything but tiny arrays; (c) refuse to dump bytes bindings,
as R42 did for aliased dicts. **Recommendation: (a)**, because (b) does not scale
and (c) silently breaks a session that reads a file. Needs a decision before
Slice 1 lands, since the dump is how the RLM engine carries state between cells.

**Q2 — does `to_str` on `[u8]` refuse, hex-render, or UTF-8 decode?** All three are
defensible; silence is not. Recommendation: **refuse** (E2214), and point at
`bytes_to_str` and `hex_encode`. A `[u8]` that prints as garbled text or as an
unexpected hex string is the silent-wrong-answer shape this repo keeps finding.

**Q3 — `bytes_len` or just `len`?** RESOLVED: `len` alone. Verified working over
`[u8]` against the real compiler (§4), so `bytes_len` would be redundant surface.

**Q4 — is `Value::Bytes` `Send`?** R15's `host_await_val` crosses an owned deep
clone between threads and REFUSES `Chan` payloads. `Vec<u8>` clones cleanly, so
bytes should cross a suspend fine — but it must be added to `SendValue`
deliberately and tested, not assumed.

**Q5 — memory ceiling.** `file_read_bytes` on a 2 GB file allocates 2 GB. R42's
regex engine bounded its program size (`MAX_PROGRAM`) precisely because
model-authored code runs in a sandbox. Should `file_read_bytes` take a cap, or
should there be a global allocation ceiling? This is a containment question, not
an ergonomics one, and it is the reason `file_read_bytes` should not ship before
it is answered.

---

## 10. Deliberately out of scope

* `b"..."` literals (§7.5).
* Compression (gzip/zstd) — needs bytes first; a separate spec.
* Sockets and streaming I/O — needs bytes first; a separate spec.
* `file_remove`, still `needs-human` from R42 Q3 (irreversible deletion, R11 risk
  integration unresolved). Bytes changes nothing about that.
* Native lowering of these builtins (see §8.2 — refusal now, lowering later).
* Non-cryptographic hashes (fnv/xxhash) and MD5. MD5 especially: shipping a broken
  hash next to a good one invites its use.

---

## 11. Stop condition

Per the R42 §12.1 amendment, this enumerates **every configuration**, because a
green number in one configuration says nothing about another.

```
DONE = Value::Bytes lands with `[u8]` as its surface type and NO new Type variant
   AND every new builtin registered at all four sites (§8.1)
   AND every new builtin E0910-refuses under `axon build`, with a test per group
   AND capability rows exist for all three file builtins, each with a FAILING-FIRST
       exfiltration test (§6.1)
   AND the `_`-fallback audit in §6.2 is done and its findings recorded
   AND Q1 (dump spelling), Q2 (to_str behaviour) and Q5 (memory ceiling) are
       DECIDED, not deferred — Q5 gates file_read_bytes specifically
   AND sha256 passes PUBLISHED vectors (empty, "abc", 1e6 x 'a'), not self-checks
   AND len(str_to_bytes(s)) == str_len(s) asserted on non-ASCII input (§3.1)
   AND base64_encode(str_to_bytes(s)) == base64_encode(s) asserted differentially
   AND `cargo test --workspace` (codegen ON) shows no new failures vs baseline
   AND `cargo test -p axon-core --no-default-features` (gate.sh:68's stage) is GREEN
   AND scripts/gate.sh passes end to end, in its own order
```

The last three clauses are the ones R42 got wrong: it verified one configuration,
reported a green number, and left `gate.sh` red for a week without knowing.
