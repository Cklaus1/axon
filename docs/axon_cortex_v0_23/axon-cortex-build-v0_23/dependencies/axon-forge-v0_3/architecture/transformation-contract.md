# Transformation Contract

A Transform is a versioned semantic mutation.

```text
Transform {
  id
  version
  description
  hypothesis
  accepts
  produces
  preconditions[]
  postconditions[]
  invariants_preserved[]
  permitted_changes[]
  parameters_schema
  verification_plan
  expected_cost
}

Transform.apply(input_ir, context) -> TransformResult

TransformResult {
  status
  output_ir
  semantic_diff
  provenance
  diagnostics
  verification_requirements
  observed_inputs
  metrics
}
```

AI-generated transforms MUST pass schema validation, static safety checks, sandboxed execution, SIR validation, compilation, required correctness verification, and Axon evaluation before promotion.
