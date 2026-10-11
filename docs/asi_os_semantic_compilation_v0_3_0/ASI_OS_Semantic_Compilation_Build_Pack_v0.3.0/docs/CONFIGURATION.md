# Configuration and Tunable Values

## Source of truth
`config/defaults.json` is the pack's example local configuration. In a real repository, map these keys into the existing configuration system and delete duplicate authorities. Prompts, display labels, rubrics, model-route definitions, budgets, timeouts and retry limits are versioned assets rather than scattered literals.

Wire enums, schema versions and unit definitions are protocol constants, not runtime tuning sliders. Authorization policy is separately controlled. `config/policy.example.json` is illustrative and grants no authority. `config/profiles.json` lists operator-selectable objective preferences and inherits defaults.

## Resolution
Start with implementation defaults, then the authorized environment profile, then tenant policy bounds, then task requests that only tighten limits. The effective run snapshot is validated and hashed. Secret values are not present in it; secret handles are resolved at dispatch by a broker. Store which layer set each key for audit.

Changing cost/latency tradeoffs can create a new experiment arm. An active run keeps its admitted snapshot except emergency revocation or tighter limits. Never silently hot-reload a different judge threshold halfway through a benchmark.

## Safe initial configuration
The package defaults to captured/offline behavior, one candidate, bounded repair, disabled exploration, disabled automatic promotion and disabled external effects. These are conservative engineering starting points, not measured optimal settings. Production promotion requires an approved calibration/experiment policy; no score threshold is supplied as universally valid.

## Validation
`config.schema.json` rejects unknown keys, negative limits and invalid operation modes. Semantic validation ensures fixture node/run limits do not exceed the selected example config. Production budget reservations enforce actual bounds; passing config validation is not evidence that a provider obeys cancellation or pricing caps.
