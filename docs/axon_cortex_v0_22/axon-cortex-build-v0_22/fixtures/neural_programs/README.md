# Neural Program format fixtures — not a trained model

`minimal-source.nps`, `minimal-manifest.json`, and `minimal.np` exercise the reviewed source/container encoding. `minimal.np` contains only source JSON and an inert text marker. It is a `conformance_fixture`; no runtime may publish it as an active Neural Program. `fixture:*` references are intentionally non-production placeholders, not authenticated schema/validator/model identities.

Run `python -B tools/neural_contract_reference.py fixtures/neural_programs/minimal.np` from the package root. A successful result means source/container bytes conform to the reference format. It explicitly returns `admitted: false`, `model_executed: false` and `semantic_correctness: NOT_EVALUATED`.

Run `python -B -m unittest discover -s tests -v` for positive/negative document and inert-format tests. These do not load weights, call APIs, test a GPU/browser, establish model accuracy, or validate production confinement. Synthetic metadata is not evidence of any installed Axon runtime.
