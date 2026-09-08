## E1001/E0308 locations: what fn-granularity does and does not buy

Both codes now carry a location. The limit is worth writing down because it
looks like a bug from the outside.

`Expr::Call` has no span, and NO `Expr` variant carries one today -- adding it
to `Call` would make it the first, across 65 construction sites. The spec
already contemplates this (`spec/compiler-phase3.md:950`: "Infer -- carries span
from Expr"), so expression spans are intended future work, not an oversight to
patch around. Until then the capability walker's finest available location is
the enclosing `FnDef`.

What that means in practice, from the flagship demo:

  CVE-2024-2624/file_store_traversal.ax -> lines 12, 21, 30  (three fns, three lines)
  agent_task_evil.ax                    -> lines 27, 27, 27  (one fn, three violations)

The second case is the honest limit. It is not ambiguous in practice, because
each E1001 message names its own call verbatim (`read_file("/etc/passwd")`,
`ai_complete(...) [host api.anthropic.com]`, `exec(...)`), so the three are
distinguishable even sharing a line. But a reader jumping to line 27 lands on
the fn signature, not on the offending call several lines down.

The transitive case is the one that got strictly better: when a `@[contained]`
fn calls a helper that performs the I/O, the walker sets `site` to the HELPER's
span, so the diagnostic points at the line the author must change rather than at
the attribute.

If expression spans ever land, `check_call`'s `site` parameter is the single
place to thread the finer span through -- the emit sites already take it.
