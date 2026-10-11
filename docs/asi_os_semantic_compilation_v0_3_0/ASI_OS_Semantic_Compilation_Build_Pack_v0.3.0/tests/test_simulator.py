from __future__ import annotations

import copy
import unittest

from harness.contracts import ROOT, ContractError, accept_results, bundle, digest, load
from harness.simulate import demonstrate, evaluate, execute


class SimulationTests(unittest.TestCase):
    def setUp(self):
        self.plan, self.registry, self.contract, self.config, self.intent = bundle()
        self.inputs = load(ROOT / 'examples/document_inputs.json')
        self.captures = load(ROOT / 'examples/document_captures.json')['cases']

    def good_run(self):
        return execute(self.plan, self.registry, self.inputs, self.captures['repaired'])

    def evaluation(self):
        run = self.good_run()
        result = evaluate(self.contract, self.registry, run['values'])
        expected = {c['id']: digest(run['values'][c['target']['node']][c['target']['port']]) for c in self.contract['criteria']}
        return result['results'], expected

    def test_complete_demo_rejects_then_repairs(self):
        report = demonstrate()
        self.assertFalse(report['original_accepted'])
        self.assertTrue(report['repaired_accepted'])
        self.assertEqual(report['reused_nodes'], ['normalize'])
        self.assertEqual(report['live_provider_calls'], 0)
        self.assertEqual(report['external_effects'], 0)
        self.assertNotEqual(report['original_plan_ref'], report['repaired_plan_ref'])

    def test_work_completion_is_not_acceptance(self):
        run = execute(self.plan, self.registry, self.inputs, self.captures['flawed'])
        self.assertTrue(run['work_completed'])
        self.assertFalse(evaluate(self.contract, self.registry, run['values'])['accepted'])

    def test_missing_capture_does_not_invoke_provider(self):
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, {})

    def test_malformed_capture_rejected(self):
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, {'draft': {'text': 7}})

    def test_wrong_input_type_rejected(self):
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, {'text': True}, self.captures['repaired'])

    def test_false_assert_stops_work(self):
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, {'draft': {'text': ''}})

    def test_stale_pure_reuse_rejected(self):
        run = self.good_run()
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, {'text': 'changed'}, self.captures['repaired'],
                    {'normalize': run['stored']['normalize']})

    def test_model_result_not_reused_as_pure(self):
        run = self.good_run()
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, self.captures['repaired'], {'draft': run['stored']['draft']})

    def test_registry_change_rejects_reuse(self):
        run = self.good_run()
        self.plan['registry_ref'] = 'sha256:' + '0'*64
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, self.captures['repaired'], {'normalize': run['stored']['normalize']})

    def test_effectful_capability_refused_even_with_capture(self):
        self.plan['nodes'][1].update({'op': 'tool.call', 'capability': 'messages.send@1', 'outputs': {'receipt': 'string'}})
        with self.assertRaises(ContractError):
            execute(self.plan, self.registry, self.inputs, {'draft': {'receipt': 'pretend-receipt'}})

    def test_missing_required_result_blocks_acceptance(self):
        results, expected = self.evaluation()
        self.assertFalse(accept_results(self.contract, results[:1], expected)[0])

    def test_stale_target_blocks_acceptance(self):
        results, expected = self.evaluation()
        results[0]['target_digest'] = 'sha256:' + '0'*64
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_missing_evidence_blocks_acceptance(self):
        results, expected = self.evaluation()
        results[0]['evidence_refs'] = []
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_evaluator_error_blocks_acceptance(self):
        results, expected = self.evaluation()
        results[0]['status'] = 'error'
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_abstain_blocks_acceptance(self):
        results, expected = self.evaluation()
        results[0]['status'] = 'abstain'
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_false_check_cannot_claim_pass(self):
        results, expected = self.evaluation()
        results[0]['decision']['passed'] = False
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_wrong_evaluator_blocks_acceptance(self):
        results, expected = self.evaluation()
        results[0]['evaluator'] = 'eval.contains@1'
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_duplicate_evaluation_blocks_acceptance(self):
        results, expected = self.evaluation()
        results.append(copy.deepcopy(results[0]))
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_malformed_evaluation_blocks_acceptance(self):
        results, expected = self.evaluation()
        del results[0]['status']
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_empty_required_contract_cannot_accept(self):
        results, expected = self.evaluation()
        for criterion in self.contract['criteria']:
            criterion['required'] = False
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_fake_calibration_references_cannot_authorize_acceptance(self):
        results, expected = self.evaluation()
        self.contract['learned_acceptance'] = 'requires_calibration'
        self.contract['criteria'][0]['decision_kind'] = 'probability'
        self.contract['criteria'][0]['decision_policy_ref'] = 'sha256:' + '1'*64
        results[0]['decision'] = {'kind': 'probability', 'probability': 0.999}
        results[0]['calibration_ref'] = 'sha256:' + '2'*64
        self.assertFalse(accept_results(self.contract, results, expected)[0])

    def test_high_probability_is_not_unconditional_acceptance(self):
        results, expected = self.evaluation()
        self.contract['criteria'][0]['decision_kind'] = 'probability'
        results[0]['decision'] = {'kind': 'probability', 'probability': 0.999}
        self.assertFalse(accept_results(self.contract, results, expected)[0])


if __name__ == '__main__':
    unittest.main()
