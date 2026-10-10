from __future__ import annotations
import sys, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from rlcd_contract_reference import *

class RLCDContractTests(unittest.TestCase):
    def test_pairwise_and_two_way_luce_agree_for_same_utilities(self):
        bt=bradley_terry(2.0,0.5); pl=luce_probabilities([2.0,0.5])[0]
        self.assertAlmostEqual(bt,pl,places=12)
    def test_proper_scores_reward_better_calibration(self):
        y=[1,1,0,0]
        self.assertLess(brier_binary([.9,.8,.2,.1],y),brier_binary([.6,.6,.4,.4],y))
        self.assertLess(nll_binary([.9,.8,.2,.1],y),nll_binary([.6,.6,.4,.4],y))
    def test_calibration_key_changes_with_candidate_policy(self):
        base=dict(producer='p',model='m',score_origin='native',question='q',candidate_policy='c1',order_policy='stable',effective_input_policy='w',domain='d',calibrator='v1')
        a=calibration_key(base);base['candidate_policy']='c2';self.assertNotEqual(a,calibration_key(base))
    def test_calibration_key_rejects_missing_domain(self):
        with self.assertRaises(ValueError): calibration_key(dict(producer='p'))
    def test_stage1_score_cannot_be_called_full_joint_probability(self):
        with self.assertRaises(ValueError): staged_choice_record('a','b','p','joint_probability','choice_on_shortlist')
    def test_stage_lineage_is_explicit(self):
        r=staged_choice_record('a','b','p','independent_score','choice_on_shortlist')
        self.assertEqual(r['stage1_semantics'],'independent_score')
    def test_iia_is_not_encoded_as_universal_gate(self):
        p=luce_probabilities([1.0,0.0,-2.0]); self.assertAlmostEqual(pairwise_odds(p,0,1),math.e,places=10)

if __name__=='__main__': unittest.main()
