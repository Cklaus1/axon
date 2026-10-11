"""Small tested arithmetic utilities, not a fitted evaluator or promotion policy."""
from __future__ import annotations

import math
from typing import Any, Iterable


def _pairs(probabilities: Iterable[float], labels: Iterable[int]) -> list[tuple[float, int]]:
    p = list(probabilities)
    y = list(labels)
    if not p or len(p) != len(y):
        raise ValueError("Nonempty predictions and equal-length labels are required")
    if any(not isinstance(x, (int, float)) or isinstance(x, bool) or not math.isfinite(x) or x < 0 or x > 1 for x in p):
        raise ValueError("Predictions must be finite probabilities in [0, 1]")
    if any(type(x) not in (int, bool) or x not in (0, 1) for x in y):
        raise ValueError("Labels must be independently supplied binary values")
    return [(float(a), int(b)) for a, b in zip(p, y)]


def brier_score(probabilities: Iterable[float], labels: Iterable[int]) -> float:
    pairs = _pairs(probabilities, labels)
    return math.fsum((p - y)**2 for p, y in pairs) / len(pairs)


def risk_coverage(probabilities: Iterable[float], labels: Iterable[int], threshold: float) -> dict[str, Any]:
    if not math.isfinite(threshold) or not 0.5 <= threshold <= 1:
        raise ValueError("Binary symmetric acceptance threshold must lie in [0.5, 1]")
    pairs = _pairs(probabilities, labels)
    selected = [(p, y) for p, y in pairs if max(p, 1 - p) >= threshold]
    errors = sum((p >= 0.5) != bool(y) for p, y in selected)
    return {"n": len(pairs), "accepted": len(selected), "coverage": len(selected) / len(pairs),
            "error_among_accepted": errors / len(selected) if selected else None}


def reliability_bins(probabilities: Iterable[float], labels: Iterable[int], bins: int = 10) -> list[dict[str, Any]]:
    if type(bins) is not int or bins <= 0:
        raise ValueError("bins must be a positive integer")
    buckets: list[list[tuple[float, int]]] = [[] for _ in range(bins)]
    for p, y in _pairs(probabilities, labels):
        buckets[min(int(p * bins), bins - 1)].append((p, y))
    result = []
    for index, bucket in enumerate(buckets):
        result.append({"bin": index, "n": len(bucket),
                       "mean_probability": math.fsum(p for p, _ in bucket) / len(bucket) if bucket else None,
                       "observed_positive_rate": math.fsum(y for _, y in bucket) / len(bucket) if bucket else None})
    return result


def experience_key(record: dict[str, Any]) -> tuple[Any, ...]:
    return tuple(record[k] for k in ("experiment_id", "task_id", "trial_id", "arm_id", "run_id"))


def total_stage_cost(record: dict[str, Any]) -> int | None:
    """Stages are disjoint; failed_attempts is an informational subset, not additive."""
    values = [record["costs"][k] for k in ("compile_microusd", "execute_microusd", "evaluate_microusd", "repair_microusd")]
    if any(value is None for value in values):
        return None
    if any(type(value) is not int or value < 0 for value in values):
        raise ValueError("Stage costs must be nonnegative integer micro-USD or unknown")
    return sum(values)
