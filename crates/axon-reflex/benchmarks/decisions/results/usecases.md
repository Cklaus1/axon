#### `loc`: fault localization (57 items: 44 distinct labels; majority class 0.053)

| model | accuracy [95% CI] | tied with leader | balanced acc. | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|
| `anthropic/claude-haiku-5.5` (chat) | 0.965 [0.91, 1.00] | leader | – | 1 | 3.08 | 0.546 |
| `microsoft/microsoft-decision-1` | 0.930 [0.86, 0.98] | yes | – | 0 | 0.36 | 0.083 |
| `inception/mercury-decide` | 0.930 [0.86, 0.98] | yes | – | 0 | 0.47 | 0.040 |
| `typesafe/jev-1.13` | 0.912 [0.84, 0.98] | yes | – | 0 | 0.29 | 0.096 |
| `cloudflare/clef` | 0.912 [0.82, 0.98] | yes | – | 0 | 0.60 | 0.507 |
| `liquid/d1` | 0.877 [0.79, 0.95] | no, gap [+0.02, +0.18] | – | 0 | 0.27 | 0.081 |
| `openai/gpt-6-luna-decisions` **rec.** | 0.877 [0.79, 0.95] | yes | – | 0 | 0.28 | 0.201 |
| `cloudflare/clef-omni` | 0.860 [0.77, 0.95] | yes | – | 0 | 0.30 | 0.317 |
| `upstage/solar-decide` | 0.842 [0.74, 0.93] | no, gap [+0.04, +0.23] | – | 0 | 9.74 | 0.115 |
| `nace-ai/drex-v1.5` | 0.772 [0.67, 0.88] | no, gap [+0.07, +0.32] | – | 0 | 0.42 | 0.077 |
| `cloudflare/clef-flash` | 0.772 [0.65, 0.88] | no, gap [+0.07, +0.32] | – | 0 | 0.65 | 0.080 |
| `togethercomputer/tev1-4b-experimental` | 0.667 [0.54, 0.79] | no, gap [+0.16, +0.44] | – | 0 | 0.31 | 0.087 |
| `upstage/solar-decide-flash` | 0.526 [0.39, 0.67] | no, gap [+0.32, +0.58] | – | 0 | 0.76 | 0.115 |
| `jaredpalmer/kev-4b` | 0.509 [0.39, 0.63] | no, gap [+0.33, +0.60] | – | 0 | 0.64 | 0.081 |
| `perplexity/pplx-decider-v1.1-27b` | 0.474 [0.33, 0.60] | no, gap [+0.35, +0.65] | – | 0 | 0.37 | 0.041 |

Winner: `anthropic/claude-haiku-5.5`; statistically tied: `microsoft/microsoft-decision-1`, `inception/mercury-decide`, `typesafe/jev-1.13`, `cloudflare/clef`, `openai/gpt-6-luna-decisions`, `cloudflare/clef-omni`. Recommended: `openai/gpt-6-luna-decisions`.

#### `repair`: repair verdict (36 items: False 18, True 18; majority class 0.500)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /18 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `inception/mercury-decide` | 0.972 [0.92, 1.00] | leader | 0.972 | 0 | 0 | 0.45 | 0.032 |
| `cloudflare/clef` | 0.972 [0.92, 1.00] | yes | 0.972 | 0 | 0 | 0.55 | 0.401 |
| `anthropic/claude-haiku-5.5` (chat) | 0.972 [0.92, 1.00] | yes | 0.972 | 0 | 0 | 2.68 | 0.474 |
| `liquid/d1` **rec.** | 0.917 [0.81, 1.00] | yes | 0.917 | 0 | 0 | 0.27 | 0.066 |
| `typesafe/jev-1.13` | 0.889 [0.78, 0.97] | yes | 0.889 | 1 | 0 | 0.28 | 0.077 |
| `openai/gpt-6-luna-decisions` | 0.833 [0.69, 0.94] | no, gap [+0.03, +0.25] | 0.833 | 0 | 0 | 0.28 | 0.168 |
| `microsoft/microsoft-decision-1` | 0.833 [0.72, 0.94] | yes | 0.833 | 1 | 0 | 0.36 | 0.066 |
| `upstage/solar-decide` | 0.750 [0.61, 0.89] | no, gap [+0.08, +0.36] | 0.750 | 0 | 0 | 0.95 | 0.096 |
| `togethercomputer/tev1-4b-experimental` | 0.722 [0.56, 0.86] | no, gap [+0.08, +0.42] | 0.722 | 5 | 0 | 0.30 | 0.069 |
| `jaredpalmer/kev-4b` | 0.722 [0.58, 0.86] | no, gap [+0.08, +0.42] | 0.722 | 1 | 0 | 0.66 | 0.064 |
| `cloudflare/clef-flash` | 0.694 [0.56, 0.83] | no, gap [+0.14, +0.42] | 0.694 | 1 | 0 | 0.67 | 0.063 |
| `nace-ai/drex-v1.5` | 0.667 [0.50, 0.81] | no, gap [+0.17, +0.47] | 0.667 | 0 | 0 | 0.39 | 0.061 |
| `cloudflare/clef-omni` | 0.639 [0.47, 0.81] | no, gap [+0.19, +0.50] | 0.639 | 0 | 0 | 0.31 | 0.250 |
| `upstage/solar-decide-flash` | 0.583 [0.42, 0.75] | no, gap [+0.22, +0.56] | 0.583 | 0 | 0 | 0.70 | 0.096 |
| `perplexity/pplx-decider-v1.1-27b` | 0.444 [0.28, 0.61] | no, gap [+0.36, +0.69] | 0.444 | 15 | 0 | 0.36 | 0.033 |

Unsafe = approved a patch that fails a check.

Winner: `inception/mercury-decide`; statistically tied: `cloudflare/clef`, `anthropic/claude-haiku-5.5`, `liquid/d1`, `typesafe/jev-1.13`, `microsoft/microsoft-decision-1`. Recommended: `liquid/d1`.

#### `tamper`: check tampering (38 items: False 18, True 20; majority class 0.526)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /20 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `cloudflare/clef` | 0.947 [0.87, 1.00] | leader | 0.950 | 2 | 0 | 0.55 | 0.462 |
| `liquid/d1` **rec.** | 0.921 [0.82, 1.00] | yes | 0.922 | 2 | 0 | 0.28 | 0.075 |
| `microsoft/microsoft-decision-1` | 0.921 [0.82, 1.00] | yes | 0.922 | 2 | 0 | 0.35 | 0.077 |
| `anthropic/claude-haiku-5.5` (chat) | 0.921 [0.82, 1.00] | yes | 0.922 | 2 | 0 | 1.68 | 0.368 |
| `typesafe/jev-1.13` | 0.895 [0.79, 0.97] | yes | 0.897 | 3 | 0 | 0.29 | 0.087 |
| `inception/mercury-decide` | 0.895 [0.79, 0.97] | yes | 0.897 | 3 | 0 | 0.43 | 0.036 |
| `openai/gpt-6-luna-decisions` | 0.868 [0.76, 0.97] | yes | 0.869 | 3 | 0 | 0.29 | 0.190 |
| `cloudflare/clef-flash` | 0.842 [0.71, 0.95] | no, gap [+0.03, +0.21] | 0.850 | 6 | 0 | 0.67 | 0.073 |
| `cloudflare/clef-omni` | 0.763 [0.63, 0.89] | no, gap [+0.08, +0.32] | 0.775 | 9 | 0 | 0.32 | 0.289 |
| `togethercomputer/tev1-4b-experimental` | 0.763 [0.63, 0.89] | no, gap [+0.08, +0.32] | 0.775 | 9 | 0 | 0.32 | 0.079 |
| `nace-ai/drex-v1.5` | 0.737 [0.58, 0.87] | no, gap [+0.08, +0.34] | 0.747 | 9 | 0 | 0.38 | 0.070 |
| `jaredpalmer/kev-4b` | 0.737 [0.61, 0.87] | no, gap [+0.08, +0.34] | 0.750 | 9 | 1 | 0.68 | 0.070 |
| `upstage/solar-decide` | 0.737 [0.58, 0.87] | no, gap [+0.05, +0.37] | 0.725 | 1 | 0 | 0.79 | 0.109 |
| `upstage/solar-decide-flash` | 0.632 [0.47, 0.79] | no, gap [+0.13, +0.50] | 0.611 | 0 | 0 | 0.70 | 0.109 |
| `perplexity/pplx-decider-v1.1-27b` | 0.579 [0.42, 0.74] | no, gap [+0.21, +0.53] | 0.575 | 7 | 0 | 0.38 | 0.038 |

Unsafe = cleared a patch that games the checks.

Winner: `cloudflare/clef`; statistically tied: `liquid/d1`, `microsoft/microsoft-decision-1`, `anthropic/claude-haiku-5.5`, `typesafe/jev-1.13`, `inception/mercury-decide`, `openai/gpt-6-luna-decisions`. Recommended: `liquid/d1`.

#### `audit`: post-verification audit (cortex repair) (48 items: False 24, True 24; majority class 0.500)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /24 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `liquid/d1` **rec.** | 1.000 [1.00, 1.00] | leader | 1.000 | 0 | 0 | 0.28 | 0.095 |
| `typesafe/jev-1.13` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.29 | 0.106 |
| `openai/gpt-6-luna-decisions` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.29 | 0.239 |
| `microsoft/microsoft-decision-1` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.36 | 0.097 |
| `perplexity/pplx-decider-v1.1-27b` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.38 | 0.048 |
| `cloudflare/clef` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.51 | 0.579 |
| `inception/mercury-decide` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 0.61 | 0.046 |
| `anthropic/claude-haiku-5.5` (chat) | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0 | 1.46 | 0.436 |
| `nace-ai/drex-v1.5` | 0.979 [0.94, 1.00] | yes | 0.979 | 1 | 0 | 0.34 | 0.088 |
| `upstage/solar-decide` | 0.917 [0.83, 0.98] | no, gap [+0.02, +0.17] | 0.917 | 0 | 0 | 1.57 | 0.134 |
| `togethercomputer/tev1-4b-experimental` | 0.833 [0.73, 0.94] | no, gap [+0.06, +0.27] | 0.833 | 8 | 0 | 0.29 | 0.100 |
| `cloudflare/clef-omni` | 0.833 [0.73, 0.94] | no, gap [+0.06, +0.27] | 0.833 | 8 | 0 | 0.32 | 0.362 |
| `jaredpalmer/kev-4b` | 0.833 [0.73, 0.94] | no, gap [+0.06, +0.27] | 0.833 | 8 | 0 | 0.66 | 0.093 |
| `cloudflare/clef-flash` | 0.833 [0.73, 0.94] | no, gap [+0.06, +0.27] | 0.833 | 8 | 0 | 0.77 | 0.092 |
| `upstage/solar-decide-flash` | 0.708 [0.58, 0.83] | no, gap [+0.17, +0.42] | 0.708 | 0 | 0 | 0.69 | 0.134 |

Unsafe = cleared a patch that games the checks.

By item kind (correct/n): `liquid/d1` cheat 8/8, exit0 8/8, forge 8/8, real-agent 24/24; `anthropic/claude-haiku-5.5` cheat 8/8, exit0 8/8, forge 8/8, real-agent 24/24.

Winner: `liquid/d1`; statistically tied: `typesafe/jev-1.13`, `openai/gpt-6-luna-decisions`, `microsoft/microsoft-decision-1`, `perplexity/pplx-decider-v1.1-27b`, `cloudflare/clef`, `inception/mercury-decide`, `anthropic/claude-haiku-5.5`, `nace-ai/drex-v1.5`. Recommended: `liquid/d1`.

#### `route`: cheap-vs-escalate routing (59 items: cheap_first_try 21, escalate 38; majority class 0.644)

| model | accuracy [95% CI] | tied with leader | balanced acc. | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|
| `anthropic/claude-haiku-5.5` (chat) **rec.** | 0.932 [0.86, 0.98] | leader | 0.915 | 0 | 2.54 | 0.534 |
| `typesafe/jev-1.13` | 0.763 [0.66, 0.86] | no, gap [+0.08, +0.27] | 0.741 | 0 | 0.30 | 0.095 |
| `inception/mercury-decide` | 0.763 [0.66, 0.86] | no, gap [+0.05, +0.27] | 0.709 | 0 | 0.47 | 0.039 |
| `cloudflare/clef` | 0.729 [0.61, 0.83] | no, gap [+0.08, +0.32] | 0.619 | 0 | 0.54 | 0.491 |
| `liquid/d1` | 0.712 [0.59, 0.83] | no, gap [+0.10, +0.34] | 0.638 | 0 | 0.28 | 0.081 |
| `upstage/solar-decide` | 0.678 [0.56, 0.80] | no, gap [+0.14, +0.39] | 0.601 | 0 | 9.57 | 0.113 |
| `openai/gpt-6-luna-decisions` | 0.627 [0.51, 0.75] | no, gap [+0.17, +0.44] | 0.647 | 0 | 0.29 | 0.198 |
| `upstage/solar-decide-flash` | 0.627 [0.49, 0.75] | no, gap [+0.19, +0.44] | 0.487 | 0 | 0.73 | 0.113 |
| `nace-ai/drex-v1.5` | 0.610 [0.49, 0.73] | no, gap [+0.20, +0.46] | 0.484 | 0 | 0.41 | 0.076 |
| `cloudflare/clef-omni` | 0.593 [0.46, 0.71] | no, gap [+0.22, +0.47] | 0.461 | 0 | 0.31 | 0.307 |
| `microsoft/microsoft-decision-1` | 0.593 [0.46, 0.71] | no, gap [+0.20, +0.49] | 0.578 | 0 | 0.39 | 0.082 |
| `cloudflare/clef-flash` | 0.576 [0.44, 0.69] | no, gap [+0.22, +0.49] | 0.533 | 0 | 0.64 | 0.078 |
| `perplexity/pplx-decider-v1.1-27b` | 0.492 [0.36, 0.61] | no, gap [+0.29, +0.59] | 0.584 | 0 | 0.37 | 0.040 |
| `togethercomputer/tev1-4b-experimental` | 0.441 [0.32, 0.56] | no, gap [+0.36, +0.63] | 0.481 | 0 | 0.31 | 0.084 |
| `jaredpalmer/kev-4b` | 0.356 [0.24, 0.47] | no, gap [+0.42, +0.73] | 0.457 | 0 | 0.61 | 0.080 |

Winner: `anthropic/claude-haiku-5.5` (no tie). Recommended: `anthropic/claude-haiku-5.5`.

#### `route55`: cheap-first routing, Haiku 5.5 (60 items: cheap_first_try 24, escalate 36; majority class 0.600)

| model | accuracy [95% CI] | tied with leader | balanced acc. | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|
| `perplexity/pplx-decider-v1.1-27b` **rec.** | 0.933 [0.87, 0.98] | leader | 0.924 | 0 | 0.39 | 0.040 |
| `anthropic/claude-haiku-5.5` (chat) | 0.933 [0.87, 0.98] | yes | 0.924 | 0 | 2.33 | 0.517 |
| `typesafe/jev-1.13` | 0.817 [0.72, 0.90] | no, gap [+0.03, +0.22] | 0.792 | 0 | 0.29 | 0.094 |
| `inception/mercury-decide` | 0.733 [0.62, 0.83] | no, gap [+0.08, +0.32] | 0.694 | 0 | 0.62 | 0.039 |
| `liquid/d1` | 0.717 [0.60, 0.82] | no, gap [+0.12, +0.33] | 0.667 | 0 | 0.27 | 0.081 |
| `cloudflare/clef` | 0.683 [0.57, 0.80] | no, gap [+0.13, +0.38] | 0.604 | 0 | 0.50 | 0.486 |
| `upstage/solar-decide` | 0.667 [0.55, 0.78] | no, gap [+0.15, +0.38] | 0.625 | 0 | 10.91 | 0.112 |
| `openai/gpt-6-luna-decisions` | 0.633 [0.50, 0.75] | no, gap [+0.18, +0.43] | 0.646 | 0 | 0.28 | 0.196 |
| `cloudflare/clef-omni` | 0.600 [0.48, 0.72] | no, gap [+0.22, +0.47] | 0.500 | 0 | 0.33 | 0.304 |
| `upstage/solar-decide-flash` | 0.600 [0.48, 0.72] | no, gap [+0.22, +0.47] | 0.500 | 0 | 0.70 | 0.112 |
| `nace-ai/drex-v1.5` | 0.583 [0.45, 0.70] | no, gap [+0.22, +0.48] | 0.500 | 0 | 0.37 | 0.075 |
| `microsoft/microsoft-decision-1` | 0.533 [0.40, 0.67] | no, gap [+0.25, +0.55] | 0.528 | 0 | 0.35 | 0.081 |
| `cloudflare/clef-flash` | 0.533 [0.42, 0.65] | no, gap [+0.25, +0.55] | 0.514 | 0 | 0.62 | 0.077 |
| `togethercomputer/tev1-4b-experimental` | 0.417 [0.30, 0.53] | no, gap [+0.37, +0.65] | 0.438 | 0 | 0.29 | 0.084 |
| `jaredpalmer/kev-4b` | 0.383 [0.27, 0.50] | no, gap [+0.40, +0.70] | 0.451 | 0 | 0.63 | 0.079 |

Winner: `perplexity/pplx-decider-v1.1-27b`; statistically tied: `anthropic/claude-haiku-5.5`. Recommended: `perplexity/pplx-decider-v1.1-27b`.

#### `gate`: MiCode risk tier (32 items: Critical 8, Read 8, Routine 8, Sensitive 8; majority class 0.250)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /24 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `microsoft/microsoft-decision-1` **rec.** | 0.750 [0.59, 0.91] | leader | 0.750 | 4 | 0 | 0.31 | 0.009 |
| `inception/mercury-decide` | 0.719 [0.56, 0.88] | yes | 0.719 | 4 | 0 | 0.34 | 0.005 |
| `anthropic/claude-haiku-5.5` (chat) | 0.719 [0.56, 0.88] | yes | 0.719 | 5 | 0 | 1.62 | 0.150 |
| `upstage/solar-decide` | 0.719 [0.56, 0.88] | yes | 0.719 | 4 | 0 | 10.02 | 0.026 |
| `cloudflare/clef-omni` | 0.688 [0.53, 0.84] | yes | 0.688 | 7 | 0 | 0.26 | 0.049 |
| `nace-ai/drex-v1.5` | 0.688 [0.53, 0.84] | yes | 0.688 | 7 | 0 | 0.35 | 0.008 |
| `typesafe/jev-1.13` | 0.656 [0.47, 0.81] | yes | 0.656 | 6 | 0 | 0.27 | 0.022 |
| `cloudflare/clef-flash` | 0.656 [0.47, 0.81] | yes | 0.656 | 7 | 0 | 0.39 | 0.012 |
| `liquid/d1` | 0.625 [0.44, 0.78] | no, gap [+0.03, +0.25] | 0.625 | 7 | 0 | 0.26 | 0.009 |
| `cloudflare/clef` | 0.625 [0.44, 0.78] | no, gap [+0.03, +0.25] | 0.625 | 7 | 0 | 0.46 | 0.079 |
| `jaredpalmer/kev-4b` | 0.531 [0.34, 0.69] | no, gap [+0.06, +0.41] | 0.531 | 7 | 0 | 0.57 | 0.009 |
| `perplexity/pplx-decider-v1.1-27b` | 0.500 [0.34, 0.69] | no, gap [+0.03, +0.44] | 0.500 | 14 | 0 | 0.31 | 0.005 |
| `togethercomputer/tev1-4b-experimental` | 0.406 [0.25, 0.56] | no, gap [+0.16, +0.53] | 0.406 | 9 | 0 | 0.29 | 0.012 |
| `upstage/solar-decide-flash` | 0.312 [0.16, 0.47] | no, gap [+0.19, +0.66] | 0.312 | 4 | 0 | 0.64 | 0.026 |
| `openai/gpt-6-luna-decisions` | 0.250 [0.09, 0.41] | no, gap [+0.28, +0.72] | 0.250 | 4 | 1 | 0.28 | 0.028 |

Unsafe = assigned a LOWER tier than MiCode's test asserts.

Winner: `microsoft/microsoft-decision-1`; statistically tied: `inception/mercury-decide`, `anthropic/claude-haiku-5.5`, `upstage/solar-decide`, `cloudflare/clef-omni`, `nace-ai/drex-v1.5`, `typesafe/jev-1.13`, `cloudflare/clef-flash`. Recommended: `microsoft/microsoft-decision-1`.

#### `trust`: MiCode Tier-B trust judge (36 items: fabricated 7, genuine 29; majority class 0.806)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /7 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `liquid/d1` | 0.833 [0.69, 0.94] | leader | 0.626 | 5 | 0 | 0.25 | 0.012 |
| `typesafe/jev-1.13` **rec.** | 0.833 [0.69, 0.94] | yes | 0.788 | 2 | 0 | 0.27 | 0.024 |
| `cloudflare/clef` | 0.833 [0.69, 0.94] | yes | 0.626 | 5 | 0 | 0.40 | 0.092 |
| `jaredpalmer/kev-4b` | 0.833 [0.69, 0.94] | yes | 0.571 | 6 | 0 | 0.59 | 0.012 |
| `togethercomputer/tev1-4b-experimental` | 0.806 [0.67, 0.92] | yes | 0.500 | 7 | 0 | 0.26 | 0.015 |
| `cloudflare/clef-omni` | 0.806 [0.67, 0.92] | yes | 0.500 | 7 | 0 | 0.27 | 0.057 |
| `perplexity/pplx-decider-v1.1-27b` | 0.806 [0.67, 0.92] | yes | 0.608 | 5 | 0 | 0.34 | 0.007 |
| `cloudflare/clef-flash` | 0.806 [0.67, 0.92] | yes | 0.500 | 7 | 0 | 0.42 | 0.015 |
| `inception/mercury-decide` | 0.806 [0.67, 0.92] | yes | 0.663 | 4 | 0 | 0.48 | 0.006 |
| `upstage/solar-decide-flash` | 0.806 [0.67, 0.92] | yes | 0.608 | 5 | 0 | 0.62 | 0.030 |
| `upstage/solar-decide` | 0.806 [0.67, 0.92] | yes | 0.608 | 5 | 0 | 0.63 | 0.030 |
| `nace-ai/drex-v1.5` | 0.778 [0.64, 0.89] | yes | 0.754 | 2 | 0 | 0.35 | 0.011 |
| `anthropic/claude-haiku-5.5` (chat) | 0.722 [0.56, 0.86] | yes | 0.719 | 2 | 0 | 1.48 | 0.123 |
| `microsoft/microsoft-decision-1` | 0.639 [0.47, 0.78] | yes | 0.667 | 2 | 0 | 0.36 | 0.012 |
| `openai/gpt-6-luna-decisions` | 0.556 [0.39, 0.72] | no, gap [+0.08, +0.44] | 0.562 | 3 | 0 | 0.27 | 0.037 |

Unsafe = called a fabricated answer genuine.

By item kind (correct/n): `typesafe/jev-1.13` cc-haiku-4.5 9/12, micode-corpus 4/6, micode-haiku-5.5 8/9, micode-opus-5.5 9/9; `anthropic/claude-haiku-5.5` cc-haiku-4.5 6/12, micode-corpus 3/6, micode-haiku-5.5 8/9, micode-opus-5.5 9/9; `liquid/d1` cc-haiku-4.5 8/12, micode-corpus 5/6, micode-haiku-5.5 8/9, micode-opus-5.5 9/9.

Winner: `liquid/d1`; statistically tied: `typesafe/jev-1.13`, `cloudflare/clef`, `jaredpalmer/kev-4b`, `togethercomputer/tev1-4b-experimental`, `cloudflare/clef-omni`, `perplexity/pplx-decider-v1.1-27b`, `cloudflare/clef-flash`, `inception/mercury-decide`, `upstage/solar-decide-flash`, `upstage/solar-decide`, `nace-ai/drex-v1.5`, `anthropic/claude-haiku-5.5`, `microsoft/microsoft-decision-1`. Recommended: `typesafe/jev-1.13`.

#### `axbool`: Axon `ai_extract::<bool>` (48 items: False 24, True 24; majority class 0.500)

| model | accuracy [95% CI] | tied with leader | balanced acc. | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|
| `anthropic/claude-haiku-5.5` (chat) | 0.938 [0.85, 1.00] | leader | 0.938 | 1 | 2.03 | 0.234 |
| `perplexity/pplx-decider-v1.1-27b` | 0.917 [0.83, 0.98] | yes | 0.917 | 0 | 0.34 | 0.011 |
| `cloudflare/clef` | 0.875 [0.77, 0.96] | yes | 0.875 | 0 | 0.39 | 0.141 |
| `liquid/d1` **rec.** | 0.854 [0.75, 0.94] | yes | 0.854 | 0 | 0.27 | 0.020 |
| `inception/mercury-decide` | 0.854 [0.75, 0.94] | yes | 0.854 | 0 | 0.45 | 0.010 |
| `typesafe/jev-1.13` | 0.812 [0.69, 0.92] | yes | 0.812 | 0 | 0.28 | 0.031 |
| `openai/gpt-6-luna-decisions` | 0.771 [0.65, 0.88] | no, gap [+0.04, +0.31] | 0.771 | 0 | 0.27 | 0.061 |
| `nace-ai/drex-v1.5` | 0.750 [0.62, 0.88] | no, gap [+0.04, +0.35] | 0.750 | 0 | 0.33 | 0.019 |
| `cloudflare/clef-omni` | 0.729 [0.60, 0.85] | no, gap [+0.08, +0.35] | 0.729 | 0 | 0.27 | 0.088 |
| `microsoft/microsoft-decision-1` | 0.729 [0.58, 0.85] | no, gap [+0.08, +0.38] | 0.729 | 0 | 0.33 | 0.021 |
| `cloudflare/clef-flash` | 0.667 [0.52, 0.79] | no, gap [+0.15, +0.42] | 0.667 | 0 | 0.40 | 0.022 |
| `upstage/solar-decide-flash` | 0.667 [0.52, 0.79] | no, gap [+0.10, +0.44] | 0.667 | 0 | 0.66 | 0.042 |
| `togethercomputer/tev1-4b-experimental` | 0.646 [0.50, 0.77] | no, gap [+0.17, +0.44] | 0.646 | 0 | 0.27 | 0.023 |
| `jaredpalmer/kev-4b` | 0.646 [0.50, 0.77] | no, gap [+0.15, +0.44] | 0.646 | 0 | 0.59 | 0.020 |
| `upstage/solar-decide` | 0.625 [0.48, 0.75] | no, gap [+0.15, +0.48] | 0.625 | 0 | 9.48 | 0.042 |

By item kind (correct/n): `liquid/d1` eval 13/16, tests 14/16, typecheck 14/16; `anthropic/claude-haiku-5.5` eval 15/16, tests 15/16, typecheck 15/16.

Winner: `anthropic/claude-haiku-5.5`; statistically tied: `perplexity/pplx-decider-v1.1-27b`, `cloudflare/clef`, `liquid/d1`, `inception/mercury-decide`, `typesafe/jev-1.13`. Recommended: `liquid/d1`.

#### `improve`: `axon improve discover --ai` template (40 items: bool-simplify 8, compare-fold 8, constant-fold 8, fold-arith-identities 8, redundant-branch-fold 8; majority class 0.200)

| model | accuracy [95% CI] | tied with leader | balanced acc. | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|
| `cloudflare/clef` | 1.000 [1.00, 1.00] | leader | 1.000 | 0 | 0.43 | 0.237 |
| `inception/mercury-decide` | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 0.50 | 0.018 |
| `anthropic/claude-haiku-5.5` (chat) | 1.000 [1.00, 1.00] | yes | 1.000 | 0 | 2.13 | 0.316 |
| `perplexity/pplx-decider-v1.1-27b` | 0.975 [0.93, 1.00] | yes | 0.975 | 0 | 0.37 | 0.018 |
| `liquid/d1` **rec.** | 0.950 [0.88, 1.00] | yes | 0.950 | 0 | 0.26 | 0.034 |
| `microsoft/microsoft-decision-1` | 0.925 [0.82, 1.00] | yes | 0.925 | 0 | 0.35 | 0.036 |
| `upstage/solar-decide` | 0.925 [0.82, 1.00] | yes | 0.925 | 0 | 10.18 | 0.059 |
| `typesafe/jev-1.13` | 0.850 [0.72, 0.95] | no, gap [+0.05, +0.28] | 0.850 | 0 | 0.27 | 0.047 |
| `cloudflare/clef-flash` | 0.825 [0.70, 0.93] | no, gap [+0.07, +0.30] | 0.825 | 0 | 0.48 | 0.037 |
| `togethercomputer/tev1-4b-experimental` | 0.800 [0.68, 0.93] | no, gap [+0.07, +0.32] | 0.800 | 0 | 0.27 | 0.039 |
| `openai/gpt-6-luna-decisions` | 0.800 [0.68, 0.93] | no, gap [+0.07, +0.32] | 0.800 | 0 | 0.27 | 0.092 |
| `cloudflare/clef-omni` | 0.800 [0.68, 0.93] | no, gap [+0.07, +0.32] | 0.800 | 0 | 0.28 | 0.147 |
| `jaredpalmer/kev-4b` | 0.675 [0.53, 0.82] | no, gap [+0.18, +0.47] | 0.675 | 0 | 0.68 | 0.035 |
| `nace-ai/drex-v1.5` | 0.650 [0.50, 0.80] | no, gap [+0.20, +0.50] | 0.650 | 0 | 0.33 | 0.033 |
| `upstage/solar-decide-flash` | 0.425 [0.28, 0.57] | no, gap [+0.43, +0.72] | 0.425 | 0 | 0.66 | 0.059 |

Winner: `cloudflare/clef`; statistically tied: `inception/mercury-decide`, `anthropic/claude-haiku-5.5`, `perplexity/pplx-decider-v1.1-27b`, `liquid/d1`, `microsoft/microsoft-decision-1`, `upstage/solar-decide`. Recommended: `liquid/d1`.

#### `risk`: MiCode shell-risk Tier-B judge (46 items: critical 15, read 15, routine 1, sensitive 15; majority class 0.326)

| model | accuracy [95% CI] | tied with leader | balanced acc. | unsafe /31 | errors | p50 s | $/1k |
|---|---|---|---|---|---|---|---|
| `perplexity/pplx-decider-v1.1-27b` | 0.826 [0.72, 0.91] | leader | 0.633 | 7 | 0 | 0.34 | 0.007 |
| `anthropic/claude-haiku-5.5` (chat) | 0.783 [0.65, 0.89] | yes | 0.600 | 7 | 0 | 2.28 | 0.201 |
| `typesafe/jev-1.13` | 0.717 [0.59, 0.83] | yes | 0.550 | 11 | 0 | 0.29 | 0.024 |
| `microsoft/microsoft-decision-1` | 0.717 [0.59, 0.85] | yes | 0.550 | 9 | 0 | 0.31 | 0.012 |
| `cloudflare/clef` | 0.717 [0.59, 0.85] | yes | 0.550 | 8 | 0 | 0.51 | 0.095 |
| `inception/mercury-decide` | 0.696 [0.54, 0.83] | yes | 0.533 | 10 | 0 | 0.34 | 0.007 |
| `upstage/solar-decide` **rec.** | 0.696 [0.57, 0.83] | yes | 0.533 | 2 | 0 | 0.74 | 0.029 |
| `openai/gpt-6-luna-decisions` | 0.674 [0.52, 0.80] | yes | 0.517 | 12 | 1 | 0.28 | 0.035 |
| `liquid/d1` | 0.630 [0.48, 0.76] | no, gap [+0.07, +0.33] | 0.483 | 15 | 0 | 0.28 | 0.012 |
| `togethercomputer/tev1-4b-experimental` | 0.609 [0.46, 0.74] | no, gap [+0.04, +0.39] | 0.467 | 18 | 0 | 0.28 | 0.015 |
| `cloudflare/clef-flash` | 0.587 [0.46, 0.72] | no, gap [+0.07, +0.41] | 0.450 | 18 | 0 | 0.39 | 0.015 |
| `cloudflare/clef-omni` | 0.565 [0.43, 0.70] | no, gap [+0.09, +0.43] | 0.433 | 19 | 0 | 0.28 | 0.059 |
| `jaredpalmer/kev-4b` | 0.522 [0.37, 0.65] | no, gap [+0.15, +0.46] | 0.400 | 20 | 0 | 0.70 | 0.011 |
| `nace-ai/drex-v1.5` | 0.413 [0.26, 0.54] | no, gap [+0.26, +0.57] | 0.317 | 27 | 0 | 0.36 | 0.010 |
| `upstage/solar-decide-flash` | 0.370 [0.24, 0.52] | no, gap [+0.28, +0.63] | 0.283 | 0 | 0 | 0.62 | 0.029 |

Unsafe = assigned a LOWER tier than the label.

By item kind (correct/n): `upstage/solar-decide` base64 6/11, hex-printf 9/14, sh -c 17/21; `anthropic/claude-haiku-5.5` base64 9/11, hex-printf 10/14, sh -c 17/21; `perplexity/pplx-decider-v1.1-27b` base64 10/11, hex-printf 9/14, sh -c 19/21.

Winner: `perplexity/pplx-decider-v1.1-27b`; statistically tied: `anthropic/claude-haiku-5.5`, `typesafe/jev-1.13`, `microsoft/microsoft-decision-1`, `cloudflare/clef`, `inception/mercury-decide`, `upstage/solar-decide`, `openai/gpt-6-luna-decisions`. Recommended: `upstage/solar-decide`.

### Recommended model per site (correctness first, then latency, then cost)

| site | code | category | winner | statistical tie | recommended | `claude-haiku-5.5` |
|---|---|---|---|---|---|---|
| Cortex `repair --ranker` (candidate order) | axon-cortex src/rank.rs | `loc` | `anthropic/claude-haiku-5.5` | 7 models | `openai/gpt-6-luna-decisions` | 0.965, tied, p50 3.08 s |
| Cortex post-verification audit (`--audit`) | axon-cortex src/audit.rs | `audit`; `tamper`: leader `cloudflare/clef`, rec. `liquid/d1`, haiku-5.5 tied | `liquid/d1` | 9 models | `liquid/d1` | 1.000, tied, p50 1.46 s, unsafe 0 |
| Cortex cheap-vs-strong advice | derived from the ranker's answer | `route55`; `route`: leader `anthropic/claude-haiku-5.5`, rec. `anthropic/claude-haiku-5.5`, haiku-5.5 tied | `perplexity/pplx-decider-v1.1-27b` | 2 models | `perplexity/pplx-decider-v1.1-27b` | 0.933, tied, p50 2.33 s |
| MiCode shell-command risk tier (Tier-B judge) | micode assembly.rs `ProviderRiskJudge`, tiered_classifier.rs `shadow_observe` | `risk`; `gate`: leader `microsoft/microsoft-decision-1`, rec. `microsoft/microsoft-decision-1`, haiku-5.5 tied | `perplexity/pplx-decider-v1.1-27b` | 8 models | `upstage/solar-decide` | 0.783, tied, p50 2.28 s, unsafe 7 |
| MiCode trust Tier-B judge | micode-verify judge.rs `apply_tier_b`, assembly.rs `ProviderJudge` | `trust` | `liquid/d1` | 14 models | `typesafe/jev-1.13` | 0.722, tied, p50 1.48 s, unsafe 2 |
| Axon `ai_extract::<bool>` | axon-ai lib.rs 1205-1250 (`__axon_ai_extract_bool`) | `axbool` | `anthropic/claude-haiku-5.5` | 6 models | `liquid/d1` | 0.938, tied, p50 2.03 s |
| `axon improve discover --ai` | axon-core improve.rs 509-620 | `improve` | `cloudflare/clef` | 7 models | `liquid/d1` | 1.000, tied, p50 2.13 s |
### Answer stability on the new categories (36-item subset, full run + 3 repeats)

Repeated only for the models that were not 30/30 deterministic in the original stability phase; the other nine answered identically in all four runs there.

| model | items with identical answers in every run | per category | accuracy per run (full, r1, r2, r3) |
|---|---|---|---|
| `perplexity/pplx-decider-v1.1-27b` | 36/36 (1.00) | audit 6/6, route55 6/6, trust 6/6, axbool 6/6, improve 6/6, risk 6/6 | 0.92, 0.92, 0.92, 0.92 |
| `cloudflare/clef` | 33/36 (0.92) | audit 6/6, route55 5/6, trust 6/6, axbool 6/6, improve 6/6, risk 4/6 | 0.86, 0.86, 0.92, 0.86 |
| `typesafe/jev-1.13` | 35/36 (0.97) | audit 6/6, route55 6/6, trust 6/6, axbool 6/6, improve 6/6, risk 5/6 | 0.94, 0.94, 0.92, 0.94 |
| `upstage/solar-decide` | 31/36 (0.86) | audit 6/6, route55 3/6, trust 6/6, axbool 5/6, improve 5/6, risk 6/6 | 0.78, 0.83, 0.83, 0.81 |
| `upstage/solar-decide-flash` | 33/36 (0.92) | audit 6/6, route55 5/6, trust 6/6, axbool 5/6, improve 5/6, risk 6/6 | 0.61, 0.58, 0.61, 0.61 |
| `anthropic/claude-haiku-5.5` (chat) | 34/36 (0.94) | audit 6/6, route55 6/6, trust 6/6, axbool 6/6, improve 5/6, risk 5/6 | 0.97, 0.94, 0.94, 0.97 |


Use-case spend (sum of `usage.cost`): `pilot-usecases` $0.0408, `usecases` $0.3987, `stability-usecases` $0.0834, total $0.5229.

