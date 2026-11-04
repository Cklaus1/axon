# containment-b4 - spec (batch 4 Lane A: C-1..C-4)

**Status: DRAFT (after Step 1 review). Not authorized for build.**

Subject: the interpreter's fs path scope (`AXON_ALLOWED_FS_READ/WRITE`, the `axon-ceiling/1` `fs_*` keys, `sandbox_create_scoped`, the
axon-os job grant that becomes both), the `--source-fd` / outcome-fd pairing, and the refusal-coverage scan set. Base: `b4/specs`
9300d9fd (= main). Every anchor is file:line at 9300d9fd. Reference binary: `/home/cklaus/projects/axon-lanes/.doc-build/b4-draft/
debug/axon` (`axon 0.1.0 (9300d9fd)`, no `codegen` feature). Probes and scratch: `/var/tmp/r51-tmp/Containment/`.

### Step 1 review (2026-11-04: AuditContain CBA-1..6, DevloopContain CBD-1..8)

Every finding was re-checked before it was applied (probes `/var/tmp/r51-tmp/Containment/step1.sh`, reference binary; anchors re-read).
Accepted 14 (13 as proposed, CBA-6 modified), rejected 0.

| id | finding | verified by | disposition |
|---|---|---|---|
| CBA-1 | a trailing-`/` root makes `dir_list("./data/")` exit 8 (and with it `genloop.rs:397`) | `DL rc=0 2`, abs `rc=0 2`, `DC dir_create("./out2/") rc=0 MADE`; `capabilities.rs:287-289` | accepted: root matching is by component (§C-1 Fix 3), V15/V16 |
| CBA-2 | V13 (`dir_create("./out")` under `./out/`) is refused today, by the lexical check | `V13 rc=8 ... restricts it to ["./out2/"]`; grant `./out2` gives `rc=0 MADE` | accepted: V13 respelled |
| CBA-3 | a nested `sandbox_create_scoped` widens the fs scope; the "nested-narrowing" sentence was false | `NEST rc=0 READ:gpumaster 0`; `builtins.rs:4917-4961` compares effects only | accepted: new item C-4 (fix, not log) |
| CBA-4 | the resolver leaves the "unflagged retry gives ENOENT" case unstated; writes through `/proc/self/root` are untested | `PW rc=0 W`, `outside/pw.txt: x` | accepted: rule in Fix 1, new fail-first test, M4803 |
| CBA-5 | hard links and bind mounts are unstated residuals | argument (no link/mount builtin, `capabilities.rs:117-171`) | accepted: Residual and Docs |
| CBA-6 | the fallback splice budget may differ from the kernel's | `[INFERENCE]`, not probed (no non-Linux host) | accepted, modified: exhaustion asks the OS; ELOOP there means `Opaque`, anything else means violation (no false refusal of V11 on the fallback) |
| CBD-1 | cost rows wrong both ways | CBD's measurements (cli_run 347 s, 12-30 s per g01 row) | accepted: §4 |
| CBD-2 | lane C also edits `outcome.rs`, `lib.rs`, `interp.rs`; line 336 was false | `ci-hygiene-spec.md:307-316` | accepted: §5 table, `installed_fd` `#[cfg(unix)]` |
| CBD-3 | the `MUTATIONS` list closes at `:15106`; lanes A and B shared a marker anchor | `v022_g01_mutations.py:15106` `]`, `v022_attack_markers.py:4180` `})` | accepted: §3 |
| CBD-4 | one strict gate at the integrated head, not one per lane | - | accepted: §4 |
| CBD-5 | `--freeze` passes today; C-3 makes it refuse; no PSV freeze is scheduled | `--freeze` rc 0 (7.7 s); `NOT_YET_SCANNED = {}` `:340-341` | accepted: stated as fact, NH-2 reworded |
| CBD-6 | `axon-intent` links axon-core and axon-os, so its PREREG pin moves too | `crates/axon-intent/Cargo.toml:23,26`; `PREREG-genviab.md:32-33` | accepted: §5 |
| CBD-7 | the cortex tests run `axon` under scopes with symlinks | `fence.rs:35-36` | accepted: C-1 Acceptance |
| CBD-8 | `axon-rt` also refuses `AXON_CEILING` | `axon-rt/src/lib.rs:2155-2158` | accepted: out-of-scope 2 |

## 0. Source entries

| item | entry (`tasks/opportunities.md`) | tier |
|---|---|---|
| C-1 | O-GV-08 [high] fs_read/fs_write prefixes are checked lexically (`:3183-3191`) | P0 |
| C-2 | O-GV-12 [low] `--source-fd` equal to an installed outcome fd that is a regular file (`:3214-3220`) | P2 |
| C-3 | O-NHR-04 [medium] refusal coverage does not scan `crates/axon-intent` (`:3323-3331`) | P1 |

### Out of scope (logged, not dropped)
1. **TOCTOU-free fd-based host ops.** C-1 checks a resolved path and then the host opens the path again (§C-1 Residual). Making
   `DefaultHost` (`crates/axon-core/src/host.rs:189-247`) open through the fd the check resolved would close the window. That changes
   the host seam and `RecordingHost`/`ReplayHost` (`replay.rs:334`, `:610`). New O-entry text for the integrator: "O-C-01 [low] the
   fs scope's resolve-then-open window: a concurrent same-uid writer can swap a path component between C-1's check and the open".
2. **Native codegen / AOT.** The native runtime reads no scope. `axon-rt` refuses to start under `AXON_ALLOWED_FS_READ/WRITE`
   (`crates/axon-rt/src/lib.rs:2089` `__axon_rt_refuse_interp_only_env`, rows `:2135-2144`, exit 2), and also under `AXON_CEILING`
   (its `fs_*` keys; row `:2155-2158`) `[REVISED: CBD-8]`. `axon build` refuses to emit while one is set (E0910,
   `crates/axon-core/tests/ambient_scope.rs:431-437`, `main.rs:4013-4028`). `sandbox_create_scoped` is interpreter-only
   (`interp/builtins.rs:4838`; no match in `crates/axon-core/src/codegen/` or `crates/axon-rt/src`). Parity rule, unchanged: **native
   refuses, the interpreter enforces**. The VM engine shares `pre_effect_gate` (`interp/eval.rs:1242`; CBA observed the R1 escape
   under `AXON_ENGINE=vm`), so C-1 and C-4 cover both interpreter engines. No codegen edit is planned, and that tree belongs to the
   AX-* session. Could not observe: the reference binary has no `codegen` (`axon build` printed "requires building axon with the
   `codegen` feature").
3. **wasm32.** WASI is not `cfg(unix)`, so the wasip1 interpreter bundle keeps the lexical check, inside the WASI preopens.
   `AXON-COMPLETENESS.md:216` says so. C-1 adds this sentence to the env text and changes nothing else on wasm.
4. **Static `@[contained]` stays lexical.** It judges literals at check time, against a filesystem that may differ at run time.
   Runtime enforcement is the scope. Doc sentence only (C-1 Docs).
5. **`exec` bypasses every fs scope.** This is by design: `exec` is its own capability (`capabilities.rs:153`). Unchanged.
6. **C-3 full judging.** C-3 has 250 sites. Lane A's g01 block holds 20 ids, so C-3 makes the drift detectable now and leaves each
   site's row or exemption to a later batch, which becomes a prerequisite of the next PSV freeze (NH-1) `[REVISED: CBD-5]`.

## 1. Priority tiers

| tier | items | why |
|---|---|---|
| P0 | C-1 | A granted prefix reaches any file through symlinks or `/proc` magic links. Observed for every fs builtin, for the ambient scope and for `sandbox_create_scoped` (repro below). This is the containment axon-os jobs and genviab grade through. |
| P1 | C-3 | 250 refusal sites in `crates/axon-intent` sit outside the scan. Rows exist only where lanes chose to add them. |
| P1 | C-4 | A program-created sandbox nested inside another may grant itself a wider or re-rooted fs/net scope (observed, `[REVISED: CBA-3]`). Ambient-scoped runs (every axon-os job) are already capped by the ambient check, so the escape is limited to program-made sandboxes. |
| P2 | C-2 | The defect needs a supervisor that is already miswired (an outcome fd that is a regular file and also the source fd). |

## 2. Items

### C-1 (P0) The fs scope is lexical: symlinks and `/proc` magic links leave the granted prefix

**Anchors (every fs entry point that checks a prefix).** There is one check, `scope_violation`
(`crates/axon-core/src/interp/builtins.rs:213-331`). `pre_effect_gate` calls it for the active sandbox (`:1087`) and for the ambient
entry (`:1100`). It tests the path TEXT with `capabilities::path_is_under` (`capabilities.rs:273-292`), after `normalize_path`
(`:220-236`) and the `..` refusal (`:322`). Then the host opens the path and follows every link. The paths come from:
- per-argument table `classify_call_paths`/`classify_call` (`capabilities.rs:117-171`), test at `builtins.rs:325`. These are
  `read_file` (impl `:1287`, host `host.rs:189`), `file_size` (`:1324`, through host `read_file`) and `file_exists` (`:1691`,
  `host.rs:217` `Path::exists`), all read. `dir_list` (`:1704`, `host.rs:225`) is a read. `write_file` (`:1295`, `host.rs:193`)
  and `append_file` (`:1308`, read-modify-write through the host) are writes. `dir_create` (`:1696`, `host.rs:221`
  `create_dir_all`) is a write. `file_copy` (`:1714`, `host.rs:239`) reads arg 0 and writes arg 1 (`capabilities.rs:124`).
  `file_rename` (`:1714`, `host.rs:245`) writes both (`:127`).
- the durable store: `dstore_open/apply/clear` (`:5297`, `:5340`, `:5415`; direct `std::fs` at `:5310`, `:5364-5367`, `:5422`).
  These are checked by DIRECTORY, `store_dir_path()` (`:467`), against both scopes (`:275-291`).
- the `@[adaptive]` provenance row: `provenance_write_permitted` (`interp.rs:5941`, test `:5953`) suppresses the row.
- the scope itself: `SandboxScope` (`interp.rs:1585-1590`), `parse_scope_list` (`:1597`), built in `ambient_sandbox`
  (`:8067`, `:8071`; ceiling meet `ceiling.rs:461-482`) and in `sandbox_create_scoped` (`builtins.rs:4839`, `:4855`). axon-os makes
  the job grant the ambient scope (`crates/axon-os/src/runtime.rs:1366-1367`) and the wrapper's `sandbox_create_scoped`.
- No other builtin takes a path. `remove`/`open`/`link` builtins do not exist (checked against the `BuiltinFn` names in
  `builtins.rs`). No `--write-prefix` CLI fence shares this code: `path_is_under` has callers only at `ceiling.rs:402,469,474`,
  `interp.rs:5953` and `builtins.rs:287,325`. `[REVISED: CBA-3]` There is NO fs/net narrowing between nested program sandboxes:
  `sandbox_create_scoped` (`builtins.rs:4865-4877`) and `sandbox_run` (`:4917-4961`) compare effects only, and the comment at
  `:4836` is about reusing the static checker's helpers. That defect is C-4.

**Reproduction** (`bash /var/tmp/r51-tmp/Containment/repro_gv08.sh <axon>`, output `repro_gv08.out`, run 2026-11-04 with
`axon 0.1.0 (9300d9fd)`). Fixture `gv08/`: `data/link -> /etc/hostname`, `data/etcdir -> /etc`, `data/alias -> real.txt`,
`data/dirlink -> realdir`, `data/cross -> ../shared/s.txt`, `out/wlink -> $W/outside/victim.txt` (dangling), `out/odir -> $W/outside`,
`out/dangle_in -> newfile.txt` (dangling), `datalink -> $W/data`. `R=AXON_ALLOWED_FS_READ`, `W=AXON_ALLOWED_FS_WRITE`.
```
R1 R=./data/ read_file("./data/link")                      rc=0  READ:gpumaster           (the host name)
R2 R=./data/ read_file("./data/etcdir/hostname")           rc=0  READ:gpumaster
R3 R=./data/,/proc/ read_file("/proc/self/root$W/outside/secret.txt")  rc=0  READ:secret outside
R4 R=/proc/self/ read_file("/proc/self/cwd/outside/secret.txt")        rc=0  READ:secret outside
R5 R=./data/ read_file("/etc/hostname")  (control)         rc=8  axon: sandbox violation: builtin `read_file` is not permitted ...
L1 R=./data/ dir_list("./data/etcdir")                     rc=0  274
L2 R=./data/ file_exists("./data/etcdir/shadow")           rc=0  true
L3 R=./data/ file_size("./data/link")                      rc=0  10
W1 W=./out/ write_file("./out/wlink","pwned")              rc=0  WROTE      outside/victim.txt: pwned
W2 W=./out/ write_file("./out/odir/w2.txt","pwned")        rc=0  WROTE      outside/w2.txt: pwned
W3 W=./out/ append_file("./out/odir/secret.txt","+pwned")  rc=0  APPENDED   outside/secret.txt: secret outside +pwned
W4 W=./out/ dir_create("./out/odir/made")                  rc=0  MADE       outside/made: exists
W5 R=./data/ W=./out/ file_copy("./data/link","./out/copied.txt")  rc=0  COPIED  out/copied.txt: gpumaster
W6 W=./out/ file_rename("./out/r.txt","./out/odir/r.txt")  rc=0  RENAMED    outside/r.txt: r
S1 sandbox_create_scoped(p,"IO","./data/","","") + sandbox_run reading ./data/link   rc=0  READ:gpumaster 1
```
The valid twins passed (rc 0): V1 `./data/alias`, V2 `./data/dirlink/d.txt`, V3 `./data/cross` under `./data/,./shared/`, V4 grant
`./datalink/`, V5 absolute grant `$W/datalink/`, V6 write `./out/dangle_in`, V7 `file_rename("./out/wlink","./out/wlink2")`.
Kernel semantics, probed with ctypes `openat2` on kernel 7.0.0-43 (`openat2_probe.py`, O_PATH, `RESOLVE_NO_MAGICLINKS`, real path
from `readlink /proc/self/fd/N`). `./data/link` gave OK `/etc/hostname`. `./data/cross` gave OK `$W/shared/s.txt`. `/proc/self/root…`,
`/proc/self/cwd/…`, `/proc/self/fd/0`, `/dev/fd/0`, `/dev/stdin` and `/proc/self/exe` gave ELOOP. `/proc/self/environ` gave OK
`/proc/<pid>/environ`. `./out/wlink` (dangling) gave ENOENT. `RESOLVE_BENEATH` from a `./data` dirfd turned `cross` into **EXDEV**.

**Design comparison.**
- (a) canonicalize-then-check: `realpath` the target, or the parent for a new file, and test it against canonical grants. Correct
  for the escapes above, because `realpath` reads `/proc/self/root` as `/`. It has a TOCTOU window, and a dangling final link
  needs special handling (W1). It also misreads fd magic links (`pipe:[N]`, and paths from another mount namespace).
- (b) `openat2(dirfd_of_grant, rel, RESOLVE_BENEATH|RESOLVE_NO_MAGICLINKS)` per granted prefix. This has no TOCTOU if the op then
  uses that fd, but it is Linux >= 5.6 only. **It falsely refuses V3**: a link from one granted prefix into another gives EXDEV
  (observed). It needs every host op rewritten to `*at` calls (out-of-scope 1).
- (c) `O_NOFOLLOW` per component: refuses every in-prefix symlink (V1, V2, V4-V6). That is a broad false refusal. Rejected.
- **Chosen: (a) done with the kernel's resolver (`resolve_for_scope`).** It is checked against every pinned grant of the same kind.
  The lexical check stays first and unchanged, so no path becomes newly allowed. On Linux, resolution is `openat2(AT_FDCWD, p,
  O_PATH|O_CLOEXEC, RESOLVE_NO_MAGICLINKS)` plus `readlink("/proc/self/fd/N")`. On ENOSYS/EPERM (kernel < 5.6, seccomp) and on other
  unix, a userspace lstat/readlink walk does it. libc 0.2.186 (`Cargo.lock`) has `SYS_openat2`, `open_how` and `RESOLVE_*`, and
  axon-core already depends on it on Linux (`crates/axon-core/Cargo.toml:103-104`). No `Cargo.toml` change. CI runs only Linux for
  axon-core: `.github/workflows/ci.yml` uses `ubuntu-latest`, and the macOS job tests `axon-rt` only (`ios.yml:42,111`).

**Fix.**
1. New `crates/axon-core/src/path_scope.rs`, `cfg(unix)`. `pub(crate) fn resolve_for_scope(path: &str, follow_final: bool) ->
   Resolution`, with `Resolution` one of `Real(PathBuf)` (absolute), `MagicLink`, or `Opaque` (cannot traverse). Algorithm, at most
   40 splices:
   find the longest prefix `E` of the path that resolves, and take `real(E)`.
   If nothing remains, the result is `real(E)`.
   If the next component `c` of the remainder is a symlink by `lstat` (dangling, since `E` was longest), splice its target
   (`real(E)/target` for a relative target) and restart.
   Otherwise the result is `real(E)` joined with the remainder. None of the remainder exists, and it has no `..` (lexical refusal).
   `[REVISED: CBA-4]` **ELOOP rule.** ELOOP under `RESOLVE_NO_MAGICLINKS`, on the full path or on any prefix probed, is retried on
   the same string without the flag. If the retry also gives ELOOP, the result is `Opaque` (a real link loop, which the op hits
   too, V11). Any other retry result (success, ENOENT, ENOTDIR, EACCES) is `MagicLink`. So `/proc/self/root$W/outside/new.txt`
   (ELOOP, then ENOENT) is a violation, not a new file outside.
   Other errors on a prefix (EACCES, ENOTDIR) make it non-resolvable. The op cannot traverse it either.
   With `follow_final=false`, the parent is resolved and the final name appended unresolved.
   On the fallback walk, a link whose text is not a path (`pipe:[N]`, `socket:[N]`, `anon_inode:…`) is `MagicLink`.
   `[REVISED: CBA-6, modified]` On the fallback walk, running out of the 40-splice budget calls `std::fs::metadata(path)`. ELOOP
   from it is `Opaque` (the op fails the same way). Any other result is a violation, because the OS resolved a chain the walk could
   not verify. A real loop therefore stays an `Err` (V11) on every resolver.
2. **Pinned grants.** `SandboxScope` (`interp.rs:1585`) gains `fs_read_roots`/`fs_write_roots: Option<Vec<PathBuf>>`. They are
   computed once, when the scope is built (`ambient_sandbox` `:8071` after the ceiling meet; `sandbox_create_scoped`
   `builtins.rs:4855`), as `resolve_for_scope(prefix, true)`. A grant that resolves to `MagicLink`/`Opaque`
   gets no root and so admits nothing by resolution. `None`/`*` stays `None` and is not resolved. Pinning matters because a program
   can `file_rename` an existing symlink over its own grant directory. Re-resolving the grant per call would follow it (M4793).
   Programs cannot create symlinks: there is no builtin for it.
3. **The check** (`scope_violation`, after the lexical pass at `:325` and the dstore test at `:287`). It runs only when that kind's
   scope is `Some`. Per (kind, arg) let `follow` be false for `file_rename` (both args) and `dir_create`, and true for everything
   else. Then:
   `MagicLink` gives a violation.
   `Real(r)` gives a violation unless `r` is absolute and `r` is under some pinned root `t` of that kind, **by component**
   `[REVISED: CBA-1]`: let `t'` be `t` with trailing `/` removed (`/` stays `/`), and require `r == t'`, or `t' == "/"`, or `r` starts
   with `t' + "/"`. `path_is_under` is not used here: it compares `starts_with(prefix)` when the prefix ends in `/`
   (`capabilities.rs:287-289`), and `readlink` never returns a trailing `/`, so the grant root itself (`dir_list("./data/")` under
   `./data/`, `dir_create("./out2/")` under `./out2/`) would exit 8. The root may come from any grant entry, not only the one that
   matched lexically (V3). A non-absolute `r` never matches.
   `Opaque` passes, and the op returns its own Err.
   The dstore check resolves `store_dir_path()` the same way. `provenance_write_permitted` (`interp.rs:5953`) resolves its log path
   and suppresses the row when the path is outside.
   The message is the existing `deny()` text plus `: it resolves outside every granted prefix (through a symbolic link)`, or `(through
   a /proc magic link)`. The resolved path is not printed. Exit 8, `SandboxViolation`, outcome class `sandbox`, audited as today
   (`:1101`).
4. **Unscoped runs are untouched.** With no `AXON_ALLOWED_FS_*`, no ceiling `fs_*`, `*`, or no scoped sandbox, nothing is
   resolved. No syscall is added.
5. `cfg(not(unix))`: `resolve_for_scope` is absent, the check stays lexical, and the doc text says so.

**Residual (documented, not closed).** A concurrent same-uid process can swap a component between the check and the open. The
program itself cannot. The interpreter is single-threaded (`spawn` runs eagerly, `interp/eval.rs:852-856`). There is no chdir
builtin (`set_current_dir` only in `main.rs:1680`, `cmd_caps`), and `exec` already escapes every scope (out-of-scope 1 and 5).
`[REVISED: CBA-5]` Hard links and bind mounts inside a grant are not detected: the scope is by path, and resolution sees neither.
Programs cannot create either (no link or mount builtin, `capabilities.rs:117-171`). Only same-uid or root setup outside the run can.

**`/proc`, `/sys`, `/dev/fd` grants: narrowed, not refused (engineering decision §6.8; was NH-1) `[REVISED: Step 2]`.** With `RESOLVE_NO_MAGICLINKS` a `/proc/` grant reaches
procfs and no further: root, cwd, exe, fd/* and map_files/* are refused. `/dev/fd/N` and `/dev/stdin` under a `/dev/` grant are
refused too, because `/dev/fd` leads to `/proc/self/fd`. A grant-time E-code would refuse runs whose grant says exactly what they
read. Two axon-os attack tests deliberately grant `/proc/` (`sandbox_capture.rs:592`, `sandbox_console.rs:213`). A `/proc/` grant
still exposes the procfs entries of same-uid dumpable processes (e.g. a supervisor's `/proc/<ppid>/environ`). The doc text states
this.

**Valid programs that must keep working** (each a test in the new `crates/axon-core/tests/scope_resolution.rs`; all pass today,
V13/V15 re-run with the reference binary in Step 1):
| # | program / grant | test |
|---|---|---|
| V1 | `R=./data/`, read `./data/alias` (-> `real.txt`) | `an_in_prefix_file_symlink_still_reads` |
| V2 | `R=./data/`, read `./data/dirlink/d.txt` | `an_in_prefix_dir_symlink_still_reads` |
| V3 | `R=./data/,./shared/`, read `./data/cross` (-> `../shared/s.txt`) | `a_symlink_into_another_granted_prefix_still_reads` |
| V4/V5 | grant `./datalink/` or `$W/datalink/` (a symlinked prefix), read under it | `a_symlinked_grant_prefix_still_reads` |
| V6 | `W=./out/`, write `./out/dangle_in` (dangling -> `newfile.txt` in prefix) | `writing_through_an_in_prefix_dangling_link_still_works` |
| V7 | `W=./out/`, `file_rename("./out/wlink","./out/wlink2")` (link to outside, renamed as a link) | `renaming_a_symlink_inside_the_write_prefix_still_works` |
| V8 | `R=./data/`, `read_file("./data/missing.txt")` gives `Err`, `file_exists` gives `false`, rc 0 | `a_missing_file_under_the_prefix_is_an_err_not_a_refusal` |
| V9 | `R=/proc/`, read `/proc/self/environ`, `/proc/self/cmdline`, `/proc/cpuinfo` | `procfs_files_under_a_proc_grant_still_read` |
| V10 | no scope variables: `read_file("/proc/self/root/etc/hostname")` rc 0 | `an_unscoped_run_resolves_nothing` |
| V11 | `R=./data/`, a link loop `./data/loop -> loop` gives `Err` (ELOOP), rc 0 | `a_symlink_loop_is_an_err_not_a_refusal` |
| V12 | `W=./out/`, `dir_create("./out/a/b/c")` then write inside | `nested_dir_create_in_prefix_still_works` |
| V13 | `[REVISED: CBA-2]` `W=./out/` where `./out` does not exist at start, `dir_create("./out/")` then `write_file("./out/x")` (the earlier spelling `dir_create("./out")` is refused today by the lexical check) | `a_grant_created_during_the_run_admits_writes` |
| V14 | `sandbox_create_scoped` + `sandbox_run` reading V1 | `a_scoped_sandbox_still_reads_in_prefix_links` |
| V15 | `[REVISED: CBA-1]` `R=./data/` `dir_list("./data/")` and `R=$W/data/` `dir_list("$W/data/")` (rc 0 `2` observed); `W=./out2/` `dir_create("./out2/")` (rc 0 `MADE` observed) | `a_trailing_slash_grant_root_argument_still_works` |
| V16 | `[REVISED: CBA-1]` `crates/axon-intent/tests/genloop.rs:392-400` `expected_outputs_are_never_present_in_the_sandbox` (`dir_list("./out/")` under `fs_read: ./data/, ./out/`), unchanged | existing test, added to Acceptance |

**Programs whose behaviour changes (intended):** R1-R4, L1-L3, W1-W6 and S1 go from rc 0 to rc 8. Under a scope that lexically
admits them, reads of `/dev/stdin`, `/dev/fd/N`, `/proc/*/fd/*`, `/proc/*/{root,cwd,exe}` and `/proc/*/map_files/*` go from rc 0 to
rc 8. `file_exists`/`file_size`/`dir_list` through a link that leads outside go to rc 8, so whether an outside file exists cannot
be read as `true`/`false`. Tracked corpus: a `git grep` for `/proc`, `/sys` or `/dev` inside `fs_read`/`fs_write` grants in
`.ax`/`.axjob`/`.toml` found only the test fixtures above and genviab's `c-star-proc` fixture, which genviab already refuses
(`selftest.py:279,372`).

**One existing test changes. Lane B (NhrEvidence) owns axon-os tests; it was messaged with no reply recorded, so the integrator assigns this edit.**
`crates/axon-os/tests/sandbox_console.rs:213-236` `a_job_forging_completion_through_proc_is_denied`. Its `/proc/self/root` read now
stops the job with exit 8, so the verdict is `Denied{axis:"sandbox"}` (`runtime.rs:1632-1634`), and `is_runtime_denial` (`:204`)
FAILS. The change: drop the `/proc/self/root` loop from that test (environ + cmdline + `exit(3)` keep it the M4773 killer: a job exit
inside its sandbox is `inner-exit`). Add `a_job_reading_its_wrapper_through_proc_root_is_denied` (same grant, the root read, expect
`Denied{axis:"sandbox"}`). `sandbox_capture.rs:592` reads `/proc/self/environ`, which is not a magic link, so it is unaffected
`[INFERENCE: from the openat2 probe; confirmed by S20]`.

**Fail-first tests** (`scope_resolution.rs`, each C-1 test `#[cfg(target_os = "linux")]` and the C-4 tests ungated, helpers as `ambient_scope.rs:28-62`, fixture as the repro;
each FAILS today because it observed rc 0 where it expects rc 8 and the target unchanged):
`a_symlink_under_the_read_prefix_cannot_reach_outside` (R1, R2, L1-L3),
`a_proc_grant_does_not_reach_the_root_through_a_magic_link` (R3, R4),
`a_magic_link_into_the_grant_is_still_refused` (`R=./data/,/proc/`, `/proc/self/root$W/data/real.txt`),
`a_write_through_proc_root_to_a_new_file_is_refused` `[REVISED: CBA-4]` (`W=./out/,/proc/`,
`write_file("/proc/self/root$W/outside/pw.txt","x")`; today `rc=0 W` and the file holds `x`; expected rc 8, no file),
`a_dangling_symlink_under_the_write_prefix_cannot_create_outside` (W1),
`a_dir_symlink_under_the_write_prefix_cannot_write_outside` (W2-W4, W6),
`file_copy_through_a_read_link_is_refused` (W5),
`a_scoped_sandbox_resolves_symlinks_too` (S1),
`a_grant_root_swapped_for_a_symlink_mid_run_stays_pinned` (`R=./data/`, `W=./`, fixture `./spare -> /etc`, the program renames
`./data` -> `./old` and `./spare` -> `./data`, then reads `./data/hostname`; rc 8),
`a_symlinked_store_dir_outside_the_write_prefix_is_refused` (`XDG_CACHE_HOME=$W/cache`, `W=$W/cache/`,
`$W/cache/axon/stores -> $W/outside`, `dstore_apply`; rc 8, nothing written outside),
`the_provenance_row_is_suppressed_when_its_log_dir_resolves_outside`.
Unit tests in `path_scope.rs`: `a_non_absolute_resolution_never_matches`, `fallback_walk_splices_a_dangling_link` (calls the
fallback directly), `fallback_walk_reports_pipe_links_as_magic`, `fallback_budget_exhaustion_on_a_real_loop_is_opaque`
`[REVISED: CBA-6]`. V15 passes today: it is the regression guard for the trailing-slash defect the draft design had (row M4802).

**Acceptance.** Every R/L/W/S line of the reproduction, and the `/proc/self/root` write, flips to rc 8 with the target unchanged.
V1-V16 stay rc 0 with identical stdout. `ambient_scope.rs`, `cli_run.rs`, `sandbox_capture.rs`, `sandbox_console.rs` and
`acceptance.rs` pass, and so do `cargo test -p axon-intent --test genloop` (V16) and `cargo test -p axon-cortex --test unattended
--test envelope_preview --test cli` `[REVISED: CBD-7]`: cortex runs `axon` under `AXON_ALLOWED_FS_*` (`fence.rs:35-36`,
`bin/cortex.rs:2524`), and each of those tests' symlinks is refused by cortex's own walk first or is the `axon` binary path
(CBD read them; `[INFERENCE]` no breakage). The tracked corpus sets no scope that admits a link (`git grep`, re-checked by CBA).
`axon card --sha256` is unchanged (no builtin added).

**Docs.** `env_registry.rs:58-59` text, and `AXON_REFERENCE.md:111-112` and `:582`, synced: "...and the path's resolved target
(symlinks followed; `/proc` magic links refused) must lie under a granted prefix's target, pinned when the scope is created
(unix; wasm keeps the text match). A `/proc/` grant still exposes same-uid processes' procfs entries. Hard links and bind mounts
inside a grant are not detected. Residual: a concurrent same-uid writer can race the check." `[REVISED: CBA-5]` The `@[contained]` reference text says that the static check is lexical. CHANGELOG (integrator):
"fs scopes resolve symlinks and refuse /proc magic links (O-GV-08); reads through links that leave the grant now exit 8".

### C-4 (P1) A nested program sandbox can widen the fs/net scope `[REVISED: CBA-3, new item]`

**Anchors.** `pre_effect_gate` checks the ACTIVE sandbox's scope (`interp/builtins.rs:1087`) and the ambient entry's (`:1100`),
never an enclosing program sandbox. `sandbox_create_scoped` refuses wider EFFECTS than the active sandbox (`:4865-4877`).
`sandbox_run` refuses entering a sandbox with wider effects (`:4917-4961`). Neither compares scopes. `active_sandbox`
(`interp.rs:1833`) holds one handle; `sandbox_run` saves it (`builtins.rs:4964`) and restores it (`:4999`). A sealed call copies the
innermost entry into the sealed kernel (`:4973-4987`, restored `:4995-4998`).

**Reproduction** (`/var/tmp/r51-tmp/Containment/step1.sh`, reference binary, 2026-11-04). No scope variables. `main` runs `outer` in
sandbox A = `sandbox_create_scoped(p,"IO","./data/","","")`. `outer` creates B = `sandbox_create_scoped(q,"IO","*","","")` and runs
`inner`, which reads `/etc/hostname`: `NEST rc=0 READ:gpumaster 0`. After C-1 alone, B = `"./data/etcdir/"` escapes the same way
(it passes the lexical check and its pinned root is `/etc`).

**Fix (enforce at use, not at creation).** The kernel (`interp.rs:1833`) gains `scope_chain: RefCell<Vec<SandboxEntry>>`, a clone of
every enclosing non-ambient sandbox. `sandbox_run` pushes the outgoing active entry (if `active >= 0`, `active != sb_handle`, and
the entry is not ambient) after `:4964`, and pops it beside the restore at `:4999`. No early return exists between the two
(`call_fn`'s result is bound first). The sealed copy (`:4973-4987`) also clones the chain into `kernels[1]` and restores it at
`:4995-4998`. In `pre_effect_gate`, after the active check at `:1087`, `scope_violation(op_name, args, e)` runs for each `e` in the
chain, with the same message, exit 8 and audit. This applies to net scopes too.
A creation-time refusal (inner prefixes must lie under outer ones) was rejected. It would refuse a program whose inner sandbox
names a wider grant but only touches paths inside both, and that works today.

**Valid programs that must keep working** (in `scope_resolution.rs`, not `cfg`-gated): `a_narrower_nested_sandbox_still_reads`
(A `./data/`, B `./data/sub/`, read `./data/sub/x`). `an_unscoped_inner_sandbox_reads_inside_the_outer_scope` (B from
`sandbox_create(p,"IO")`, read `./data/a.txt`). `a_sandbox_run_returning_restores_the_outer_scope` (after `sandbox_run(A, …)`
returns, `main` with no scope reads `./other.txt`, rc 0). Tracked tests create scoped sandboxes only in sequence, never nested
(`git grep -c` ≥ 2: `ambient_scope.rs`, `cli_run.rs`, `dstore_capability.rs`, each in separate programs), so no tracked program
changes `[INFERENCE: read at the cited lines, not run]`.

**Fail-first test.** `an_inner_sandbox_cannot_widen_an_outer_fs_scope`: the reproduction, expecting rc 8 and stderr naming
`read_file` and `["./data/"]`. Sub-cases: B = `"./data/etcdir/"`, and a net scope (A net `api.x.com`, B net `*`, `http_get` of
another host, refused before any connect as in `ambient_scope.rs:235`). It fails today: rc 0.

**Acceptance.** The NEST line becomes rc 8. The three valid programs and `ambient_scope.rs:250-315` (the ambient nesting tests)
pass unchanged.

**Docs.** `AXON_REFERENCE.md:582` (`sandbox_create_scoped`): "an inner sandbox's fs/net scope is checked together with every
enclosing sandbox's: a call must be inside all of them". CHANGELOG: "nested scoped sandboxes no longer widen the fs/net scope
(O-GV-08 follow-up); a nested escape now exits 8".

### C-2 (P2) `--source-fd` equal to the outcome fd

**Anchors.** `take_outcome_channel` (`main.rs:1177-1314`) accepts any open fd above 2 (`:1213-1239`) and installs it
(`outcome::install`, `:1304`; `outcome.rs:103`). `read_source_fd` (`main.rs:8650-8677`) refuses fds 0-2 (`:8657`) and non-regular
files (`:8666-8670`), then reads to EOF and closes the fd (`:8675`). `dispatch` installs the channel before `cmd_run` (`:1317`,
`:6550`).

**Reproduction** (`python3 -B /var/tmp/r51-tmp/Containment/probe_gv12.py <axon>`, a copy of `/var/tmp/r51-tmp/SweepB3/probe.py`,
2026-11-04, `axon 0.1.0 (9300d9fd)`):
```
[src fd5 (distinct)]           rc=0 out='hi\n' outcome='{..."kind":"exit","code":0,"class":"return"...}'
[src fd3 == outcome fd] (pipe) rc=1 err='error: --source-fd 3: not a regular file' outcome='{..."kind":"compile","code":1,"class":"source"...}'
[src fd4 == nonce fd]          rc=1 err='error reading --source-fd 4: Bad file descriptor (os error 9)' outcome='{..."kind":"compile",...}'
[src fd3 == outcome fd (regular O_RDWR file)] rc=2 err='error: no `main` function defined — ...' outcome=''
```

**Fix (option: refuse the equality; rejected: require a pipe).** `outcome.rs` gains `#[cfg(unix)] pub fn installed_fd() ->
Option<i32>` after `installed()` (`:108-110`) `[REVISED: CBD-2: read_source_fd is #[cfg(unix)] (main.rs:8649), so lane C's CH-8
cfg_attr on Channel.fd (outcome.rs:81-82) stays valid]`. In `read_source_fd`, before the `metadata` call, `installed_fd() == Some(fd)` refuses through the existing
`refuse` (rc 1, a `compile`/`source` record written to the outcome fd, as every other `--source-fd` refusal does) with
`error: --source-fd {fd} is the outcome fd named by AXON_OUTCOME_FD; the program cannot be read from it`. Requiring a pipe would
refuse a supervisor that points `AXON_OUTCOME_FD` at a file (`3>out.json`). No in-repo caller does that (all pass pipes: `runtime.rs:440`,
`cli_run.rs:47453,48085`), but it works today and is not a defect. That would be a false refusal. The nonce-fd case is already
refused with a record (above) and is unchanged.

**Fail-first test.** `cli_run.rs`, appended after `run_source_fd_provenance_hashes_fd_bytes` (`:47958`):
`run_source_fd_equal_to_the_outcome_fd_is_refused`. A regular file on fd 3 (`AXON_OUTCOME_FD=3`), the nonce pipe on fd 4,
`--source-fd 3`. Expect rc 1, stderr naming the outcome fd, and the file holding exactly one `axon-outcome/1` line with `"kind":
"compile","class":"source"`. It fails today: rc 2, "no main", empty file. Control: `run_source_fd_matches_path_run` unchanged.

**Acceptance.** The 4th probe line becomes rc 1 with a record. The other three are byte-identical.

### C-3 (P1) Refusal coverage scans `crates/axon-intent`

**Anchors.** `scripts/v022_refusal_coverage.py`: `ROOT_CRATES` `:88`, `SCOPE_DIRS = scope_dirs()` `:312`, `NOT_YET_SCANNED` `:340`
(re-measured both ways, `:7432-7437`; a freeze refuses while non-empty, `:7409`, `v022_freeze_manifest.py:117-128`),
`verdict_types` `:5389-5423`, `EXEC_CONSTRUCTORS` `:6215-6230`. Gate stage `gate.sh:581-582`, `:708`.

**Measurement** (read-only, `python3 -B /var/tmp/r51-tmp/Containment/nhr04.py {base,intent}`, imports the script, appends
`axon-intent` to `ROOT_CRATES`, recomputes `SCOPE_DIRS`, runs `check()`; 8 s each):
- base: 14 protected crates, 200 in-scope files, 0 BAD.
- with axon-intent: 15 crates, **217 files (+17)**, **254 BAD = 250 uncovered sites + 4 exec-constructor lines**. By file:
  intent.rs 58, authorize.rs 49, genloop.rs 42, confidence.rs 21, audit_runs.rs 16, cli.rs 16, generator.rs 16, policy.rs 12,
  grant_infer.rs 4, emit.rs 3, synth.rs 3, admit.rs/approval.rs/pipeline.rs/refusal.rs 1 each. That is **244 in axon-intent**.
  Spill-over: `axon-loop-contracts/src/error.rs` 3 (`:46,61,65`), `schema.rs` 1 (`:58`), and `axon-cortex/src/runner.rs` 2 (`:3827`
  `BoundExceeded::output`, `:3840` `unread`). The entry's 172 was measured at be47b23e. axon-intent has had 15 commits since.
- Why the spill-over: verdict types are derived by NAME from every in-scope file. axon-intent adds the enums `Refusal`, `Decision`,
  `LoopOutcome`, `GenFailure`, `Confidence` and `Produce`, plus a decided type named `Error` (measured by diffing `verdict_types()`).
  Functions returning `loop-contracts::Refusal` or `std::io::Error` then match.
- Exec constructors: `authorize.rs` `mint`, `generator.rs` `generate`, `genloop.rs` `check_source` and `load_card`.

**Fix.**
1. Add `"axon-intent"` to `ROOT_CRATES` (`:88`), with a comment line saying why: authorize-repair, audit-runs and the intent
   compiler decide what a repair or job may do.
2. Add the 4 functions to `EXEC_CONSTRUCTORS` as `builder: own .arg/.env sites`. That label must be checked against each body at
   build time. Any that takes argv from its caller goes to `EXEC_WRAPPERS`.
3. The 6 spill-over sites get checkable exemptions with a fragment. Each one is a helper that builds an error value and decides
   nothing (it collides on the type name). NOT a rule change, so no other count moves.
4. The 17 axon-intent files go into `NOT_YET_SCANNED` with their measured counts (rule: the gate re-measures and refuses a stale
   count in either direction). From then on, a new or removed axon-intent refusal fails the gate until its count is updated. That
   is the mechanism the entry asks for, with the per-site judging deferred (NH-1).
5. Re-measure the counts on the lane's final tree. axon-intent may move before the merge.

**Fail-first.** `python3 -B scripts/v022_refusal_coverage.py` with only step 1 applied prints the 254 BAD lines and fails. After
steps 2-4 it exits 0. With step 4 applied and one added `return Err(` in `authorize.rs`, it fails with "NOT_YET_SCANNED says N".
Neither the script nor axon-intent has a cargo test, so there are no g01 rows. The script's own drift check is the killer.

**Acceptance.** Gate stage "v0.22 protected refusal-site coverage" passes. `[REVISED: CBD-5]` `python3 -B
scripts/v022_refusal_coverage.py --freeze` exits 0 today (`NOT_YET_SCANNED = {}`, `:340-341`; measured 7.7 s). After C-3 it
refuses, naming 17 files NOT YET SCANNED, so `v022_freeze_manifest.py:117-128` refuses a PSV freeze until the judging batch lands.
No PSV freeze is scheduled in `tasks/` or `governance/specs/v022-psv-*.md` (CBD grep, 2026-11-04). The integrator records this in
the batch notes.

## 3. g01 rows (block M4790-M4809; M4790-M4805 used, M4806-M4809 unused)

Format as `v022_g01_mutations.py:14991-15000`. `[REVISED: CBD-3]` Rows go before the `]` that closes `MUTATIONS` at `:15106`
(`:15107-15109` is the `__main__` block). Markers go in a new `ATTACK_MARKERS.update({...})` block for Lane A after
`v022_attack_markers.py:4180`, one block per lane, applied A, B, C. Both files are integrator-owned, so the lane supplies the rows.
Find strings are post-fix code `[INFERENCE: exact text fixed at build time]`. `SR` = axon-core `--no-default-features --test
scope_resolution`.

| id | item | file | mutation | crate / args | killing test (marker) |
|---|---|---|---|---|---|
| M4790 | C-1 | interp/builtins.rs | the resolved check is skipped for `FsRead` | SR | `a_symlink_under_the_read_prefix_cannot_reach_outside` (ATTACK: a read through an in-prefix symlink reached a file outside the grant) |
| M4791 | C-1 | interp/builtins.rs | the resolved check is skipped for `FsWrite` | SR | `a_dir_symlink_under_the_write_prefix_cannot_write_outside` (ATTACK: a write through an in-prefix symlink landed outside the grant) |
| M4792 | C-1 | path_scope.rs | `resolve` = 0 (no `RESOLVE_NO_MAGICLINKS`) | SR | `a_magic_link_into_the_grant_is_still_refused` (ATTACK: a /proc magic link was followed under a scope) |
| M4793 | C-1 | interp.rs | the roots are re-resolved per call, not pinned | SR | `a_grant_root_swapped_for_a_symlink_mid_run_stays_pinned` (ATTACK: a renamed symlink re-pointed the grant) |
| M4794 | C-1 | interp/builtins.rs | `follow` is true for `file_rename` | SR | `renaming_a_symlink_inside_the_write_prefix_still_works` (FALSE REFUSAL: renaming an in-prefix link was refused) |
| M4795 | C-1 | path_scope.rs | the dangling-link splice is skipped | SR | `a_dangling_symlink_under_the_write_prefix_cannot_create_outside` (ATTACK: a dangling link created a file outside the grant) |
| M4796 | C-1 | path_scope.rs | ELOOP is `Opaque` without the retry | SR | `a_proc_grant_does_not_reach_the_root_through_a_magic_link` (ATTACK: /proc/self/root reached outside the grant) |
| M4797 | C-1 | path_scope.rs | the absolute-path guard is removed | axon-core `--no-default-features --lib` | `path_scope::tests::a_non_absolute_resolution_never_matches` (ATTACK: a non-path link text matched a relative grant) |
| M4798 | C-1 | path_scope.rs | ENOENT on the full path is `MagicLink` | SR | `a_missing_file_under_the_prefix_is_an_err_not_a_refusal` (FALSE REFUSAL: a missing in-prefix file exited 8) |
| M4799 | C-1 | interp/builtins.rs | the root must be the entry that matched lexically | SR | `a_symlink_into_another_granted_prefix_still_reads` (FALSE REFUSAL: a link between granted prefixes was refused) |
| M4800 | C-1 | interp/builtins.rs | the dstore dir is not resolved | SR | `a_symlinked_store_dir_outside_the_write_prefix_is_refused` (ATTACK: the durable store wrote through a symlinked dir) |
| M4801 | C-2 | main.rs | the `installed_fd() == Some(fd)` refusal is removed | axon-core `--no-default-features --test cli_run` | `run_source_fd_equal_to_the_outcome_fd_is_refused` (ATTACK: the outcome file was read as the program) |
| M4802 | C-1 | interp/builtins.rs | the root's trailing `/` is kept in the component match `[REVISED: CBA-1]` | SR | `a_trailing_slash_grant_root_argument_still_works` (FALSE REFUSAL: the grant root itself was refused) |
| M4803 | C-1 | path_scope.rs | an unflagged retry that is not ELOOP gives `Opaque` `[REVISED: CBA-4]` | SR | `a_write_through_proc_root_to_a_new_file_is_refused` (ATTACK: a write through /proc/self/root created a file outside the grant) |
| M4804 | C-4 | interp/builtins.rs | the `scope_chain` loop in `pre_effect_gate` is removed | SR | `an_inner_sandbox_cannot_widen_an_outer_fs_scope` (ATTACK: a nested sandbox widened the fs scope) |
| M4805 | C-4 | interp/builtins.rs | the pop beside the restore at `sandbox_run` is removed | SR | `a_sandbox_run_returning_restores_the_outer_scope` (FALSE REFUSAL: a returned sandbox still scoped its caller) |

**Re-anchored:** M4773 (`v022_g01_mutations.py:15020-15023`). Its killer `a_job_forging_completion_through_proc_is_denied` keeps
the name and loses the root read, so it stays the killer. Re-verify. `[INFERENCE: if the build changes the test differently, retarget
M4773 to a_job_that_calls_exit_itself_is_not_completed, which also observes inner-exit]`. The provenance test and the fallback unit
tests have no rows. They share `resolve_for_scope` with rows M4790/M4795, and the block is kept for a later batch.

## 4. Gates and cost

| what | command | cost |
|---|---|---|
| lane tests `[REVISED: CBD-1]` | iterate with `--test scope_resolution --test ambient_scope` and `cli_run run_source_fd` filtered; full `cli_run` once before handoff; `cargo test -p axon-os --test sandbox_console --test sandbox_capture --test acceptance`; `-p axon-intent --test genloop`; `-p axon-cortex --test unattended --test envelope_preview --test cli` | ambient_scope ≈ 15 s cold, seconds incremental; full `cli_run` 347 s (1009 tests); axon-os pair 20 s (CBD measured); cortex/intent `[INFERENCE: minutes]` |
| repro re-run | `repro_gv08.sh` and `probe_gv12.py` against the lane binary | < 10 s measured (draft binary) |
| refusal coverage | `python3 -B scripts/v022_refusal_coverage.py` (and `--freeze` to read the count) | 8 s measured |
| g01 `[REVISED: CBD-1]` | one `--only=M4790,...,M4805,M4773` (17 rows), then `python3 -B scripts/g01_anchors.py` | ≈ 4-9 min (12-30 s/row measured: lfc 12 rows 3.5 min, int31 94 rows 21 min) |
| repo | `cargo fmt --all -- --check`; clippy `-p axon-core -p axon-os`; `scripts/pci_delta.py --check`; `scripts/spec_ledger.py`; `axon card --sha256` before == after | standard |
| gate `[REVISED: CBD-4]` | ONE strict `scripts/gate.sh --strict` at the integrated head after A+B+C, at the integrator. Stages for A: S16 "axon-core tests" (`gate.sh:1096`, new `scope_resolution`, `cli_run` ≈ 6 min of it), S20 "required crate verification" (`:1165`, axon-os), S14 clippy (`:1016`), refusal coverage (`:708`), g01 anchors (`:718`), S23 genviab-selftest (`:1198`, new binary) | ≈ 44 min (2663 s, batch 3 measured) + Lane B's S09u `[INFERENCE: 5-15 min]` |
| cortex receipt regress | not needed: no `crates/axon-cortex/src` change | $0 |
| paid | none | **$0** |

## 5. Sequencing and lanes

**Merge before the genviab PREREG freeze.** `[REVISED: CBD-6]` C-1, C-2 and C-4 change both pinned binaries: `axon` and
`axon-intent` (which links axon-core and axon-os, `crates/axon-intent/Cargo.toml:23,26`; pins `PREREG-genviab.md:32-33`). The freeze
runs once, after A, B and C have merged. One build lane, `b4/containment`, branched from main at its tip. Commit order: C-1
(resolver and scope pins, then tests, the sandbox_console edit and docs), C-4, C-2, C-3.

`[REVISED: CBD-2]` **File ownership (batch 4; same table as `ci-hygiene-spec.md:309-316`).** The draft's claim that no other lane
owns these files was false.

| file | owner |
|---|---|
| `interp/builtins.rs`, `path_scope.rs` (new), `tests/scope_resolution.rs` (new), `env_registry.rs` (text), `scripts/v022_refusal_coverage.py` (`ROOT_CRATES`, `EXEC_CONSTRUCTORS`, `NOT_YET_SCANNED`), `AXON_REFERENCE.md` env and builtin rows | A |
| `interp.rs` `:1585-1597`, `:1833` and `:1882` (C-4 `scope_chain`), `:5941-5953`, `:8067-8162` | A |
| `interp.rs` `:7973` | C (CH-8 `cfg`) |
| `lib.rs` module list `:42-75` (`path_scope`) | A |
| `lib.rs` `:622` | C |
| `outcome.rs` after `:110` (`installed_fd`, `#[cfg(unix)]`) | A |
| `outcome.rs` `:81-82` | C (CH-8 `cfg_attr` on `Channel.fd`) |
| `main.rs` `read_source_fd` `:8650-8677` | A |
| `main.rs` `:3849-3980` | C reads it (CH-5 matches message texts); no edit by either |
| `cli_run.rs` after `:47958` (append) | A |
| `cli_run.rs` `:985-1017`, `:35164-35251` | C |
| `axon-os/tests/sandbox_console.rs` `:204-236` | integrator assigns to A, because C-1 causes the change (Lane B's test dir) |
| `axon-os/tests/acceptance.rs`, `crates/axon-cortex/src` | B |
| `ci.yml`, `ios.yml`, `android_lifecycle.sh`, `harness_integrity.rs` | C |
| `gate.sh` (S09u), `v022_g01_mutations.py`, `v022_attack_markers.py`, `CHANGELOG.md`, `opportunities.md`, B's refusal-coverage anchors after A's merge | integrator |

Not touched by A: `crates/axon-core/src/codegen/`, `crates/axon-cortex/`, `host.rs`. All shared-file hunks are disjoint.

**Sequence `[REVISED: CBD-2/CBD-4/CBD-6]`.** 1. A, B and C build in parallel. 2. Merge Lane C's CH-8 `cfg` commit first, so
`installed_fd` lands beside the final `Channel`. Then A (C-1, C-4, C-2, C-3), then B. 3. The integrator appends B's
refusal-coverage anchors after A's `ROOT_CRATES` change, adds the g01 rows and marker blocks (A, B, C), runs one `--only` per lane
(A ≈ 4-9 min), and runs one strict gate. 4. The PREREG freeze runs last.
Integrator: CHANGELOG, pci-themes JSON for the C-1/C-4/C-2 commits (all touch `crates/axon-core/src` behaviour), the O-C-01
entry, and `opportunities.md` stamps O-GV-08 (C-1 and C-4), O-GV-12 and O-NHR-04 (the last "PARTLY: scanned, 17 files
NOT_YET_SCANNED").
Wall clock `[INFERENCE]`: lane ≈ 5-7 h (C-1 dominates; the test loop is not the bottleneck), integrator ≈ 1 h gate + g01.

## 6. Engineering decisions

1. C-1 is resolve-then-check, not per-prefix `RESOLVE_BENEATH`, because BENEATH falsely refuses V3 (EXDEV observed). It is not
   `O_NOFOLLOW`, which falsely refuses V1/V2/V4-V6. The TOCTOU residual is stated, and its fix is logged (O-C-01).
2. The lexical check stays first and unchanged. Resolution can only refuse more, never admit more.
3. Grants are pinned when the scope is created, so a run cannot re-point its own grant.
4. `file_rename` and `dir_create` resolve the parent only (rename(2)/mkdir(2) do not follow the final name). Every other op follows.
5. Missing paths, loops and EACCES pass through to the op's own `Err`. Only an escape is exit 8 (V8, V11).
6. C-2 refuses the equality rather than requiring a pipe: no false refusal of a file-backed supervisor.
7. C-3 stages through the script's own `NOT_YET_SCANNED` mechanism, not by inventing 250 exemptions.
8. `[REVISED: Step 2; was NH-1]` `/proc`, `/sys` and `/dev` grants are narrowed (magic links refused), not refused when parsed. This
   is engineering, not product: the brief asked the spec to decide, it is reversible, and refusing at parse would falsely refuse
   runs whose grant says exactly what they read (two attack tests grant `/proc/`). The remaining exposure (same-uid procfs, e.g. a
   parent's environ) is what the grant text names, and genviab already refuses non-relative grants.
9. `[REVISED: CBA-1]` Resolved paths match pinned roots by component with trailing `/` stripped, not through `path_is_under`.
10. `[REVISED: CBA-3]` C-4 is enforced at use through an enclosing-scope chain, not at creation, so a wide-but-unused inner
    grant keeps working.
11. `[REVISED: CBA-6]` On fallback budget exhaustion, the OS decides: its ELOOP gives `Opaque`; any other result is a violation.

## 7. Needs-human `[REVISED: Step 2]`

Every other open question is engineering and is decided in §6 (former NH-1 is §6.8).

- **NH-1 (was NH-2) C-3 blocks the next PSV freeze.** Fact `[REVISED: CBD-5]`: `--freeze` passes today and refuses after C-3 (17
  files NOT YET SCANNED). No PSV freeze is scheduled. Deciding to gate a protocol freeze on a not-yet-planned judging batch is a
  process decision. (A) accept: C-3 lands as specified, and the 244-site judging batch becomes a prerequisite of the next PSV
  freeze. (B) land only steps 2-3 and log, which leaves the entry's "no mechanism" defect open. **Recommend A**: the freeze would
  otherwise bind evidence over decision code no rule has read, and nothing is scheduled to break.

## 8. Verification log (2026-11-04, 9300d9fd)

Observed: the C-1 reproduction (all 25 lines above) and the C-2 probe (4 lines), both with `axon 0.1.0 (9300d9fd)`. The openat2 /
EXDEV kernel probe (kernel 7.0.0-43). The C-3 counts, by importing the unmodified script read-only (base 0 BAD / 200 files;
+axon-intent 254 BAD / 217 files; the verdict-type diff; re-run by CBA). The axon-os verdict mapping of class `sandbox`
(`runtime.rs:1632-1634`), read. Every anchor, by `grep -n`/`read` at 9300d9fd.
Step 1 re-checks (2026-11-04, `step1.sh`, reference binary): V15 (`dir_list("./data/")` rc 0, absolute rc 0, `dir_create("./out2/")`
rc 0), V13 old spelling rc 8, grant `./out2` rc 0, the `/proc/self/root` write rc 0 with `x` written outside, NEST rc 0. `--freeze`
rc 0. The anchors `v022_g01_mutations.py:15106`, `v022_attack_markers.py:4180`, `axon-rt/src/lib.rs:2155-2158`,
`axon-intent/Cargo.toml:23,26`, `PREREG-genviab.md:29-33`, `ci-hygiene-spec.md:307-316`, `builtins.rs:1076-1104,4917-5002`,
`genloop.rs:392-400`.
Not verified: native behaviour (the reference binary lacks `codegen`; parity comes from source and `ambient_scope.rs:437`).
macOS/fallback resolver behaviour (no non-Linux run). That `sandbox_capture.rs:592` passes after C-1 (inferred from the probe).
The per-row g01 kill. CI's container seccomp allowing `openat2` (fallback covers ENOSYS/EPERM). C-4's sealed-kernel chain copy
(no probe with a sealed candidate). The cortex tests after C-1 (CBD read them, not run).
