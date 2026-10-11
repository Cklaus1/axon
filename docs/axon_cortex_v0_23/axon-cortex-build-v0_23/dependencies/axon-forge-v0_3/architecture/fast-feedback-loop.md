# Fast Feedback Loop

Compiler/test latency is part of inference economics.

Total completed-task cost SHOULD model model tokens/cost/latency, compile latency, test latency, execution cost, failed iterations, and human review where applicable.

Forge SHOULD measure cold/incremental compile latency, diagnostic latency, semantic-index update latency, targeted/full test latency, repair iterations, tokens per successful repair, dollar cost per completed task, and cache hit rate.

Axon MAY experiment with incremental compilation, compiler modes, test selection, diagnostic strategies, and escalation policies to minimize completed-task cost.
