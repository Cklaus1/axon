from __future__ import annotations

import copy
import unittest

from harness.contracts import ROOT, load
from harness.metrics import brier_score, experience_key, reliability_bins, risk_coverage, total_stage_cost


class MetricTests(unittest.TestCase):
    def test_perfect_brier_score(self):
        self.assertEqual(brier_score([0, 1], [0, 1]), 0)

    def test_constant_uncertain_brier_score(self):
        self.assertEqual(brier_score([0.5, 0.5], [0, 1]), 0.25)

    def test_empty_data_is_not_zero_error(self):
        with self.assertRaises(ValueError):
            brier_score([], [])

    def test_length_mismatch(self):
        with self.assertRaises(ValueError):
            brier_score([0.5], [0, 1])

    def test_invalid_probability(self):
        for value in (-0.1, 1.1, float('nan'), float('inf'), True):
            with self.subTest(value=value), self.assertRaises(ValueError):
                brier_score([value], [1])

    def test_invalid_label(self):
        with self.assertRaises(ValueError):
            brier_score([0.5], [2])

    def test_risk_coverage_exposes_abstention(self):
        result = risk_coverage([0.9, 0.6, 0.2, 0.1], [1, 0, 1, 0], 0.8)
        self.assertEqual(result['accepted'], 3)
        self.assertEqual(result['coverage'], 0.75)
        self.assertAlmostEqual(result['error_among_accepted'], 1/3)

    def test_no_accepted_cases_error_is_unknown(self):
        result = risk_coverage([0.5, 0.5], [0, 1], 0.9)
        self.assertIsNone(result['error_among_accepted'])
        self.assertEqual(result['coverage'], 0)

    def test_threshold_validation(self):
        with self.assertRaises(ValueError):
            risk_coverage([0.5], [0], 0.3)

    def test_bins_keep_endpoint_one(self):
        bins = reliability_bins([0, 0.25, 0.75, 1], [0, 0, 1, 1], 2)
        self.assertEqual([b['n'] for b in bins], [2, 2])
        self.assertEqual(bins[1]['observed_positive_rate'], 1)

    def test_empty_bins_not_fabricated_zero(self):
        bins = reliability_bins([0.1], [0], 10)
        self.assertIsNone(bins[-1]['observed_positive_rate'])

    def test_distinct_repeated_trial_identity(self):
        record = load(ROOT / 'examples/experience.json')
        another = copy.deepcopy(record)
        another['trial_id'] += 1
        another['run_id'] = 'fixture-run-0002'
        self.assertNotEqual(experience_key(record), experience_key(another))
        self.assertEqual(record['task_id'], another['task_id'])
        self.assertEqual(record['arm_id'], another['arm_id'])

    def test_identical_record_is_idempotent_key(self):
        record = load(ROOT / 'examples/experience.json')
        self.assertEqual(experience_key(record), experience_key(copy.deepcopy(record)))

    def test_failed_attempts_subset_not_double_counted(self):
        record = load(ROOT / 'examples/experience.json')
        record['costs'] = {'compile_microusd': 100, 'execute_microusd': 200, 'evaluate_microusd': 40,
                           'repair_microusd': 60, 'failed_attempts_microusd': 30}
        self.assertEqual(total_stage_cost(record), 400)

    def test_unknown_stage_cost_not_zero(self):
        record = load(ROOT / 'examples/experience.json')
        record['costs']['execute_microusd'] = None
        self.assertIsNone(total_stage_cost(record))


if __name__ == '__main__':
    unittest.main()
