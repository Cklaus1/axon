from __future__ import annotations

import copy
import unittest

from jsonschema import Draft202012Validator
from referencing.exceptions import Unresolvable

from harness.contracts import (ROOT, ContractError, Schemas, affected_nodes, bundle, canonical_bytes,
                               compatible, digest, infer_type, lint_bundle, load, loads,
                               topological, validate_decision)


class ContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schemas = Schemas()

    def setUp(self):
        self.plan, self.registry, self.contract, self.config, self.intent = bundle()

    def findings(self):
        return lint_bundle(self.plan, self.registry, self.contract, self.config, self.intent, self.schemas)

    def has_code(self, code):
        self.assertIn(code, {f.code for f in self.findings()})

    def test_all_schemas_are_metavalid(self):
        self.assertEqual(len(self.schemas.documents), 17)
        for schema in self.schemas.documents.values():
            Draft202012Validator.check_schema(schema)

    def test_three_valid_bundles(self):
        for name in ('document', 'coding', 'research'):
            with self.subTest(name=name):
                self.assertEqual(lint_bundle(*bundle(name), schemas=self.schemas), [])

    def test_nonplan_envelopes(self):
        for filename, kind in [('evaluation_result', 'evaluation-result'), ('experience', 'experience'),
                               ('event', 'event'), ('repair_request', 'repair'), ('owner_profile', 'owner-profile'),
                               ('strategy_artifact', 'strategy-artifact'), ('experiment_profile', 'experiment-profile'),
                               ('evidence_projection', 'evidence-projection'), ('resource_projection', 'resource-projection'),
                               ('forge_binding', 'forge-binding'), ('compilation_record', 'compilation-record')]:
            self.schemas.require(kind, load(ROOT / f'examples/{filename}.json'))

    def test_remote_reference_is_not_fetched(self):
        validator = Draft202012Validator({'$ref': 'https://not-a-real-provider.invalid/unsafe-schema'},
                                         registry=self.schemas.registry)
        with self.assertRaises(Unresolvable):
            validator.validate({'value': 1})

    def test_duplicate_capability_rejected(self):
        self.registry['capabilities'].append(copy.deepcopy(self.registry['capabilities'][0]))
        self.plan['registry_ref'] = digest(self.registry)
        self.has_code('DUPLICATE_CAPABILITY')

    def test_effect_cannot_be_laundered_as_pure(self):
        self.registry['capabilities'][0]['effects'] = ['write']
        self.plan['registry_ref'] = digest(self.registry)
        self.has_code('INVALID_EFFECT_CLASS')

    def test_idempotent_claim_requires_contract(self):
        cap = next(c for c in self.registry['capabilities'] if c['id'] == 'patch.apply@1')
        cap['supports_idempotency'] = False
        self.plan['registry_ref'] = digest(self.registry)
        self.has_code('INVALID_EFFECT_CLASS')

    def test_duplicate_criterion_rejected(self):
        self.contract['criteria'].append(copy.deepcopy(self.contract['criteria'][0]))
        self.plan['evaluation_ref'] = digest(self.contract)
        self.has_code('DUPLICATE_CRITERION')

    def test_no_required_criteria_rejected(self):
        for c in self.contract['criteria']:
            c['required'] = False
        self.plan['evaluation_ref'] = digest(self.contract)
        self.has_code('NO_REQUIRED_CRITERIA')

    def test_evaluation_target_must_exist(self):
        self.contract['criteria'][0]['target']['port'] = 'absent'
        self.plan['evaluation_ref'] = digest(self.contract)
        self.has_code('EVALUATION_TARGET')

    def test_evaluator_argument_type_checked(self):
        self.contract['criteria'][1]['arguments']['required_terms'] = 12
        self.plan['evaluation_ref'] = digest(self.contract)
        self.has_code('TYPE_MISMATCH')

    def test_evaluation_target_binding_not_ambiguous(self):
        self.contract['criteria'][1]['arguments'] = {}
        self.plan['evaluation_ref'] = digest(self.contract)
        self.has_code('EVALUATOR_ARGUMENTS')

    def test_changed_registry_digest_rejected(self):
        self.registry['name'] = 'changed-registry'
        self.has_code('DIGEST_MISMATCH')

    def test_changed_intent_digest_rejected(self):
        self.intent['goal'] = 'Changed goal'
        self.has_code('DIGEST_MISMATCH')

    def test_task_class_mismatch(self):
        self.plan['task_class'] = 'another.task'
        self.has_code('TASK_MISMATCH')

    def test_unknown_input(self):
        self.plan['nodes'][0]['args']['text'] = {'input': 'missing'}
        self.has_code('UNKNOWN_INPUT')

    def test_unknown_node_reference(self):
        self.plan['nodes'][1]['args']['text'] = {'node': 'missing', 'port': 'text'}
        self.has_code('UNKNOWN_REFERENCE')

    def test_output_types_cannot_override_registry(self):
        self.plan['nodes'][0]['outputs']['text'] = 'boolean'
        self.has_code('OUTPUT_MISMATCH')

    def test_operation_must_match_capability(self):
        self.plan['nodes'][0]['mechanism'] = 'model.call'
        self.has_code('MECHANISM_MISMATCH')

    def test_semantic_operation_must_match_capability(self):
        self.plan['nodes'][0]['semantic_op'] = 'act'
        self.has_code('SEMANTIC_OP_MISMATCH')

    def test_assert_must_receive_boolean(self):
        self.plan['nodes'][3]['args']['condition'] = {'literal': 'true'}
        self.has_code('TYPE_MISMATCH')

    def test_assert_cannot_retry_into_pass(self):
        self.plan['nodes'][3]['retry']['max_attempts'] = 2
        self.has_code('UNSAFE_RETRY')

    def test_return_matches_intent(self):
        self.intent['output_type'] = 'boolean'
        self.plan['intent_ref'] = digest(self.intent)
        self.has_code('TYPE_MISMATCH')

    def test_return_must_be_terminal(self):
        extra = copy.deepcopy(self.plan['nodes'][0])
        extra['id'] = 'after_return'
        extra['depends_on'] = ['finish']
        self.plan['nodes'].append(extra)
        self.has_code('NONTERMINAL_RETURN')

    def test_only_one_return(self):
        extra = copy.deepcopy(self.plan['nodes'][-1])
        extra['id'] = 'second_return'
        self.plan['nodes'].append(extra)
        self.has_code('RETURN_COUNT')

    def test_retry_bound(self):
        self.plan['nodes'][0]['retry']['max_attempts'] = 3
        self.has_code('LIMIT')

    def test_node_timeout_bound(self):
        self.plan['nodes'][0]['limits']['timeout_ms'] += 1
        self.has_code('LIMIT')

    def test_run_budget_bound(self):
        self.plan['limits']['max_cost_microusd'] += 1
        self.has_code('LIMIT')

    def test_captured_mode_cannot_enable_live_effects(self):
        self.config['allow_external_effects'] = True
        self.plan['config_ref'] = digest(self.config)
        self.has_code('UNSAFE_CONFIG')

    def test_unknown_config_key_rejected(self):
        self.config['mysterious_override'] = True
        self.has_code('SCHEMA')

    def test_topological_dependencies_precede_consumers(self):
        order = topological(self.plan)
        for node in self.plan['nodes']:
            for dep in node['depends_on']:
                self.assertLess(order.index(dep), order.index(node['id']))

    def test_repair_invalidation_is_transitive(self):
        self.assertEqual(affected_nodes(self.plan, {'draft'}), {'draft', 'check', 'guard', 'finish'})

    def test_unknown_repair_target_rejected(self):
        with self.assertRaises(ContractError):
            affected_nodes(self.plan, {'absent'})

    def test_repair_preserves_unaffected_parallel_peer(self):
        extra = copy.deepcopy(self.plan['nodes'][0])
        extra['id'] = 'independent'
        self.plan['nodes'].insert(0, extra)
        self.plan['nodes'][-1]['depends_on'].append('independent')
        self.assertNotIn('independent', affected_nodes(self.plan, {'draft'}))


# One independently named test per adversarial plan fixture.
def invalid_test(case):
    def test(self):
        bad = load(ROOT / case['path'])
        findings = lint_bundle(bad, self.registry, self.contract, self.config, self.intent, self.schemas)
        self.assertIn(case['expected_code'], {f.code for f in findings})
    return test

for _case in load(ROOT / 'examples/invalid_cases.json'):
    _name = _case['path'].rsplit('/', 1)[-1].replace('.json', '')
    setattr(ContractTests, 'test_invalid_fixture_' + _name, invalid_test(_case))


class ParserIdentityTests(unittest.TestCase):
    def test_duplicate_json_keys_rejected(self):
        with self.assertRaises(ContractError):
            loads('{"x":1,"x":2}')

    def test_nonfinite_json_rejected(self):
        for source in ('NaN', 'Infinity', '-Infinity', '1e999'):
            with self.subTest(source=source), self.assertRaises(ContractError):
                loads(source)

    def test_byte_limit(self):
        with self.assertRaises(ContractError):
            loads('"too long"', max_bytes=4)

    def test_depth_limit(self):
        with self.assertRaises(ContractError):
            loads('[[[[0]]]]', max_depth=2)

    def test_unpaired_surrogate_rejected(self):
        with self.assertRaises(ContractError):
            loads('"\\ud800"')

    def test_identity_stable_for_object_order(self):
        self.assertEqual(digest({'b': 2, 'a': 1}), digest({'a': 1, 'b': 2}))

    def test_identity_preserves_array_order(self):
        self.assertNotEqual(digest([1, 2]), digest([2, 1]))

    def test_identity_float_rejected(self):
        with self.assertRaises(ContractError):
            canonical_bytes({'p': 0.95})

    def test_identity_unsafe_integer_rejected(self):
        with self.assertRaises(ContractError):
            canonical_bytes(2**53)

    def test_identity_keeps_unicode(self):
        self.assertIn('café'.encode(), canonical_bytes({'text': 'café'}))

    def test_boolean_not_integer(self):
        self.assertEqual(infer_type(True), 'boolean')
        self.assertFalse(compatible(infer_type(True), 'integer'))

    def test_only_integer_number_widening(self):
        self.assertTrue(compatible('integer', 'number'))
        self.assertFalse(compatible('number', 'integer'))
        self.assertFalse(compatible('string', 'number'))

    def test_artifact_descriptor_not_arbitrary_path(self):
        artifact = {'digest': 'sha256:' + 'a'*64, 'tenant': 'fixture', 'media_type': 'text/plain', 'size_bytes': 1}
        self.assertEqual(infer_type(artifact), 'artifact')
        self.assertEqual(infer_type({'path': '/tmp/a'}), 'object')


class DecisionTests(unittest.TestCase):
    def test_valid_choice(self):
        validate_decision({'kind': 'choice', 'selected': 'a', 'probabilities': {'a': 0.8, 'b': 0.2}, 'provider_confidence': 0.6})

    def test_choice_sum_is_checked(self):
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'choice', 'selected': 'a', 'probabilities': {'a': 0.8, 'b': 0.8}})

    def test_selected_option_must_exist(self):
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'choice', 'selected': 'other', 'probabilities': {'a': 0.8, 'b': 0.2}})

    def test_selected_option_must_be_maximum(self):
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'choice', 'selected': 'b', 'probabilities': {'a': 0.8, 'b': 0.2}})

    def test_probability_has_no_generic_confidence(self):
        validate_decision({'kind': 'probability', 'probability': 0.9})
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'probability', 'probability': 0.9, 'provider_confidence': 0.99})

    def test_score_is_not_probability(self):
        validate_decision({'kind': 'score', 'score': 8.5, 'rubric_ref': 'sha256:' + 'b'*64})
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'probability', 'probability': 8.5})

    def test_score_can_retain_full_distribution(self):
        validate_decision({'kind': 'score', 'score': 0.8, 'rubric_ref': 'sha256:' + 'b'*64,
                           'probabilities': {'low': 0.2, 'high': 0.8}, 'provider_confidence': 0.6})

    def test_score_distribution_must_sum_to_one(self):
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'score', 'score': 0.8, 'rubric_ref': 'sha256:' + 'b'*64,
                               'probabilities': {'low': 0.8, 'high': 0.8}})

    def test_decision_nan_rejected(self):
        with self.assertRaises(ContractError):
            validate_decision({'kind': 'probability', 'probability': float('nan')})


if __name__ == '__main__':
    unittest.main()
