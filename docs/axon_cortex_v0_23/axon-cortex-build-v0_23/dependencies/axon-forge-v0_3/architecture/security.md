# Security and anti-Goodhart contract

Forge-generated transforms, compiler plugins, meta-programs, migrations and build programs are untrusted candidates.

They MUST NOT mint capabilities, rewrite the evaluator judging themselves, mutate hidden references, falsify metrics, disable required security checks, delete tests to obtain fitness, or expand physical resource authority.

Forge requests capabilities through the external Axon authority seam; execution occurs through the external isolated runtime seam. Hard semantic/security constraints dominate optimization metrics.
