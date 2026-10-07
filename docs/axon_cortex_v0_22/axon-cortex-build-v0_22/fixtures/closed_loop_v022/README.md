# 0.22 model-free fixtures

`bundle.json` contains only synthetic policy/context/episode/transition records. Its issuer strings, digests, timestamps, pass counts and costs are fabricated **test inputs**, not claimed source results or trusted credentials. Tests mutate these inputs to exercise rejection paths.

`pilot-template.json` leaves owner-approved corpus/statistical/authority parameters unset and deployment disabled. It may pass template-schema validation, but must fail runtime-readiness checks until independently configured/approved.

`empty-runtime-evidence.json` is intentionally unqualified and contains no product receipts. Running `tools/check_v022_runtime_evidence.py` on it must exit 2 with NOT_QUALIFIED. No fixture can enable runtime or establish improvement.
