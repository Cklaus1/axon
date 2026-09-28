#!/usr/bin/env python3
"""The readiness RELAY never passes along a verdict from a verifier that is not
the pinned one (operator direction 2026-09-28: replacing the installed verifier
must not be an authority-changing route).

The installed verifier describes itself (sha256 + build provenance); the
operator's verifier.json pins the same fields. Any disagreement, and any
non-production / dirty / debug build, leaves the three protected components
NOT_RUN. `operator_verifier` is replaced here ONLY to point at a fake binary —
on this host the real one refuses (no /etc/axon/trust), which is tested too."""

import importlib.util
import json
import os
import stat
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
spec = importlib.util.spec_from_file_location("pvr", os.path.join(HERE, "protected_verifier_ready.py"))
pvr = importlib.util.module_from_spec(spec)
spec.loader.exec_module(pvr)

IDENT = {"sha256": "a" * 64, "build": "production", "fabric_revision": "b" * 40,
         "source_dirty": False, "rustc": "rustc 1.99.0-nightly", "profile": "release",
         "target": "x86_64-unknown-linux-gnu"}
PASS = {"status": "PASS"}


def manifest(path, **over):
    m = dict(IDENT, schema=pvr.VERIFIER_MANIFEST_SCHEMA, path=path,
             trust_roots={"qualification": pvr.QUALIFICATION_ROOT})
    m.update(over)
    return m


class Relay(unittest.TestCase):
    def verdicts(self, report_ident, **manifest_over):
        d = tempfile.mkdtemp()
        out = {"schema": "axon-fabric-readiness/1", "build": report_ident.get("build"),
               "trust_root": pvr.QUALIFICATION_ROOT, "verifier": report_ident,
               "components": {k: PASS for k in pvr.PROTECTED}}
        fake = os.path.join(d, "axon-fabric")
        with open(fake, "w") as f:
            f.write("#!/bin/sh\ncat <<'X'\n" + json.dumps(out) + "\nX\n")
        os.chmod(fake, stat.S_IRWXU)
        m = manifest(fake, **manifest_over)
        real = pvr.operator_verifier
        pvr.operator_verifier = lambda: (m, None)
        try:
            return pvr.protected_verdicts()
        finally:
            pvr.operator_verifier = real

    def test_the_pinned_clean_release_verifier_is_relayed_with_its_identity(self):
        v = self.verdicts(IDENT)
        for k in pvr.PROTECTED:
            self.assertEqual(v[k]["status"], "PASS")
            self.assertEqual(v[k]["verifier"], IDENT)
            self.assertEqual(v[k]["verifier_manifest"], pvr.VERIFIER_MANIFEST)

    def test_any_identity_field_differing_from_the_manifest_is_refused(self):
        for k in pvr.VERIFIER_IDENTITY:
            other = dict(IDENT, **{k: "different"})
            v = self.verdicts(other)
            for c in pvr.PROTECTED:
                self.assertEqual(v[c]["status"], "NOT_RUN", k)
                self.assertIn(f"verifier reports {k}=", v[c]["missing"][0])

    def test_a_dirty_debug_or_test_trust_build_is_refused_even_when_pinned(self):
        for over in ({"source_dirty": True}, {"profile": "debug"}, {"build": "test-trust"}):
            ident = dict(IDENT, **over)
            v = self.verdicts(ident, **over)  # the manifest honestly pins it
            for c in pvr.PROTECTED:
                self.assertEqual(v[c]["status"], "NOT_RUN", over)
                self.assertIn("not a clean production release build", v[c]["missing"][0])

    def test_a_manifest_naming_another_trust_root_is_refused(self):
        v = self.verdicts(IDENT, trust_roots={"qualification": "/home/agent/keys"})
        self.assertIn("not deciding over", v["protected_backend"]["missing"][0])

    def test_on_this_host_the_real_operator_root_is_absent_so_nothing_is_earned(self):
        if os.path.exists(pvr.OPERATOR_TRUST_ROOT):
            self.skipTest("an operator trust root exists on this host")
        for c, v in pvr.protected_verdicts().items():
            self.assertEqual(v["status"], "NOT_RUN", c)
            self.assertIn("earned only on the protected host", v["missing"][0])


if __name__ == "__main__":
    unittest.main()
