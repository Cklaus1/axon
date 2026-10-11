# Build Program API

Build logic is programmable but its output remains inspectable and reproducible:

`BuildProgram -> BuildGraph -> Artifacts`

Build programs MAY query target/resources, generate source/bindings, process declared assets, select backends, configure optimization, declare dependencies, request compilation/tests, and emit artifacts.

The resulting BuildGraph MUST expose steps, declared inputs/outputs, environment dependencies, capabilities, cache keys, tool versions, and provenance.
