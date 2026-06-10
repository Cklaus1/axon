# Interactive Axon — the resume runtime (R15)

Programs here use **`host_await(prompt) -> reply`**: the program *suspends*, hands
`prompt` to the host, and *resumes* with the host's reply. Under `axon run` the
host is stdin/stdout — so an Axon program can prompt for and read user input,
genuinely suspending in between (no busy-wait, no replay).

```bash
axon run examples/interactive/greet.ax
# type an answer at each prompt
```

## What's happening

`host_await` is the first primitive of the **R15 resume runtime** — the
suspend-across-host-event mechanism that gates every interactive Axon target
(browser frame loops, native/mobile UI). The program runs on a worker thread;
`host_await` blocks it on a channel (that blocking *is* the suspension) while the
host services the request. See `governance/specs/R15-resume-runtime.md`.

**Scope (v0):** str payloads, a stdin/stdout (or Rust-closure) host. Native
codegen refuses `host_await` (E0910 — interp-only). Arbitrary-value payloads (via
a same-thread stackful coroutine) and the browser binding (Asyncify / a JS
step-loop) are the next slices.
