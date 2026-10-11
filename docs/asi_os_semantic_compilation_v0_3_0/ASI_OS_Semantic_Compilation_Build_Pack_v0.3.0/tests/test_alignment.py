from __future__ import annotations

import json
import unittest
from pathlib import Path

from harness.contracts import ROOT, Schemas, load, lint_bundle, bundle


class CortexAlignmentTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.schemas = Schemas()

    def test_alignment_examples_validate(self):
        cases = [
            ('owner_profile', 'owner-profile'),
            ('strategy_artifact', 'strategy-artifact'),
            ('experiment_profile', 'experiment-profile'),
            ('evidence_projection', 'evidence-projection'),
            ('resource_projection', 'resource-projection'),
            ('forge_binding', 'forge-binding'),
            ('compilation_record', 'compilation-record'),
        ]
        for filename, schema in cases:
            with self.subTest(filename=filename):
                self.schemas.require(schema, load(ROOT / f'examples/{filename}.json'))

    def test_cortex_owner_profile_has_no_semantic_compilation_global_authority(self):
        profile = load(ROOT / 'integration/CORTEX_V023_OWNER_PROFILE.json')
        owners = profile['owners']
        forbidden = {'semantic-compilation', 'asi-semantic-compilation', 'local'}
        for concern, owner in owners.items():
            with self.subTest(concern=concern):
                self.assertNotIn(owner.lower(), forbidden)
        self.assertEqual(owners['intent'], 'CX-19 IntentIR')
        self.assertEqual(owners['execution_semantics'], 'CX-04 AIR')
        self.assertEqual(owners['software_semantics'], 'Forge v0.3 SIR')
        self.assertEqual(owners['evidence'], 'CX-32 UEG')
        self.assertEqual(owners['experiments'], 'CX-33')

    def test_dependency_hash_locks_match_owner_profile(self):
        profile = load(ROOT / 'integration/CORTEX_V023_OWNER_PROFILE.json')
        locks = {x['name']: x for x in profile['dependency_locks']}
        cortex = load(ROOT / 'integration/CORTEX_V023_INPUT_LOCK.json')
        forge = load(ROOT / 'integration/FORGE_V03_INPUT_LOCK.json')
        self.assertEqual(locks['axon-cortex']['sha256'], cortex['sha256'])
        self.assertEqual(locks['axon-cortex']['version'], cortex['version'])
        self.assertEqual(locks['axon-forge']['sha256'], forge['sha256'])
        self.assertEqual(locks['axon-forge']['version'], forge['version'])

    def test_source_owner_snapshot_preserves_no_new_authority(self):
        source = load(ROOT / 'source_context/cortex_v023/V023_OWNER_MAP.json')
        self.assertTrue(source['rows'])
        self.assertTrue(all(row['new_authority'] is False for row in source['rows']))

    def test_semantic_and_mechanism_axes_are_distinct(self):
        plan, registry, contract, config, intent = bundle('document')
        self.assertEqual(lint_bundle(plan, registry, contract, config, intent, self.schemas), [])
        draft = next(n for n in plan['nodes'] if n['id'] == 'draft')
        self.assertEqual(draft['semantic_op'], 'generate')
        self.assertEqual(draft['mechanism'], 'model.call')
        self.assertNotEqual(draft['semantic_op'], draft['mechanism'])

    def test_capability_cannot_semantically_launder_mechanism(self):
        plan, registry, contract, config, intent = bundle('document')
        draft = next(n for n in plan['nodes'] if n['id'] == 'draft')
        draft['semantic_op'] = 'rule'
        findings = lint_bundle(plan, registry, contract, config, intent, self.schemas)
        self.assertIn('SEMANTIC_OP_MISMATCH', {f.code for f in findings})

    def test_crosswalk_covers_every_spec_requirement_exactly_once(self):
        import re
        spec_ids = re.findall(r'\*\*([A-Z]+-\d{3})\.', (ROOT / 'SPEC.md').read_text())
        crosswalk = load(ROOT / 'integration/CORTEX_V023_REQUIREMENT_CROSSWALK.json')
        row_ids = [r['requirement_id'] for r in crosswalk['rows']]
        self.assertEqual(len(row_ids), len(set(row_ids)))
        self.assertEqual(set(row_ids), set(spec_ids))
        self.assertEqual(sum(crosswalk['counts'].values()), len(spec_ids))

    def test_hard_gate_before_pareto_is_documented(self):
        text = (ROOT / 'docs/STRATEGY_EVIDENCE_EXPERIMENTS.md').read_text()
        self.assertIn('hard semantic / correctness / security / authority gates', text)
        self.assertIn('No scalar reward rescues an invalid candidate.', text)

    def test_resource_projection_cannot_mint_grant_or_reservation(self):
        import copy
        value = load(ROOT / 'examples/resource_projection.json')
        value = copy.deepcopy(value)
        value['grant'] = 'forged'
        self.assertTrue(self.schemas.errors('resource-projection', value))

    def test_evidence_projection_requires_canonical_identity(self):
        import copy
        value = copy.deepcopy(load(ROOT / 'examples/evidence_projection.json'))
        value.pop('canonical_evidence_ref')
        self.assertTrue(self.schemas.errors('evidence-projection', value))

    def test_experiment_profile_requires_falsifier_and_hard_gates(self):
        import copy
        value = copy.deepcopy(load(ROOT / 'examples/experiment_profile.json'))
        value.pop('falsifier')
        value['hard_gate_refs'] = []
        self.assertTrue(self.schemas.errors('experiment-profile', value))

    def test_strategy_artifact_rejects_unversioned_shape(self):
        import copy
        value = copy.deepcopy(load(ROOT / 'examples/strategy_artifact.json'))
        value.pop('revision')
        self.assertTrue(self.schemas.errors('strategy-artifact', value))

    def test_forge_binding_requires_pinned_package_hash(self):
        import copy
        value = copy.deepcopy(load(ROOT / 'examples/forge_binding.json'))
        value['forge_package_sha256'] = 'latest'
        self.assertTrue(self.schemas.errors('forge-binding', value))

    def test_forge_binding_requires_air_lineage(self):
        import copy
        value = copy.deepcopy(load(ROOT / 'examples/forge_binding.json'))
        value.pop('air_ref')
        self.assertTrue(self.schemas.errors('forge-binding', value))


if __name__ == '__main__':
    unittest.main()
