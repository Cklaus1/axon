"""Executed model-free adversarial cases; none is a product-runtime gate."""
from __future__ import annotations
import copy
import itertools
import json
from pathlib import Path
import sys
import unittest
sys.dont_write_bytecode = True
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
from research_reference import (ContractError, validate_decision, candidate_set_digest, digest,
    pipeline_binding, cache_key, permutation_average, stable_rank, checklist,
    context_projection, eligible_route, evolution_screen, Cascade, task_ledger, priced_usage)

class DecisionTests(unittest.TestCase):
    def setUp(self):self.r=json.loads((ROOT/'fixtures/research_v021/decision.json').read_text())
    def reject(self):
        with self.assertRaises(ContractError):validate_decision(self.r)
    def test_valid_fixture(self):self.assertEqual(validate_decision(self.r)['chosen_id'],'read')
    def test_wrong_principal(self):
        with self.assertRaises(ContractError):validate_decision(self.r,expected_principal='other')
    def test_wrong_epoch(self):
        with self.assertRaises(ContractError):validate_decision(self.r,expected_epoch=2)
    def test_extra_field(self):self.r['authorize']=True;self.reject()
    def test_bad_fixture_claim(self):self.r['fixture_only']=False;self.reject()
    def test_bad_product_claim(self):self.r['product_result']='PASS';self.reject()
    def test_candidate_duplication(self):self.r['candidates'].append(self.r['candidates'][0]);self.reject()
    def test_candidate_text_revision(self):self.r['candidates'][0]['text_sha256']='e'*64;self.reject()
    def test_candidate_digest(self):self.r['candidate_set_sha256']='0'*64;self.reject()
    def test_presentation_digest(self):self.r['presentation_sha256']='0'*64;self.reject()
    def test_missing_probability(self):self.r['probabilities_ppm'].pop('read');self.reject()
    def test_unknown_probability_label(self):self.r['probabilities_ppm']['alien']=0;self.reject()
    def test_non_normalized_probability(self):self.r['probabilities_ppm']['read']=1;self.reject()
    def test_negative_probability(self):self.r['probabilities_ppm']['read']=-1;self.reject()
    def test_float_probability(self):self.r['probabilities_ppm']['read']=700000.5;self.reject()
    def test_bool_probability(self):self.r['probabilities_ppm']['read']=True;self.reject()
    def test_raw_score_labels(self):self.r['raw_scores_micros'].pop('read');self.reject()
    def test_wrong_winner(self):self.r['chosen_id']='search';self.reject()
    def test_tie_stable_id(self):
        self.r['probabilities_ppm']={'search':500000,'read':500000}
        self.assertEqual(validate_decision(self.r)['chosen_id'],'read')
    def test_order_does_not_change_set(self):
        original=self.r['candidate_set_sha256'];self.r['candidates'].reverse()
        self.r['presentation_sha256']=digest([r['id'] for r in self.r['candidates']])
        self.assertEqual(candidate_set_digest(self.r['candidates']),original)
        validate_decision(self.r)
    def test_unknown_is_not_zero_cost(self):self.r['usage']['status']='UNKNOWN';self.reject()
    def test_unknown_needs_reserved_ceiling(self):self.r['usage']={'status':'UNKNOWN','observed_micro':None,'reserved_micro':0};self.reject()
    def test_unknown_valid(self):
        self.r['usage']={'status':'UNKNOWN','observed_micro':None,'reserved_micro':20};validate_decision(self.r)
    def test_usage_ceiling(self):self.r['usage']['observed_micro']=101;self.reject()
    def test_nondecision_cannot_keep_choice(self):self.r['status']='TRANSPORT_ERROR';self.reject()
    def test_nondecision_typed_empty(self):
        self.r.update(status='REFUSED',chosen_id=None,probabilities_ppm={},raw_scores_micros={});validate_decision(self.r)
    def test_valid_external_calibration_binding(self):
        self.r['calibration']={'pipeline_sha256':pipeline_binding(self.r),'artifact_sha256':'e'*64,'domain_id':self.r['domain_id'],'correctness_ppm':800000};validate_decision(self.r)
    def test_stale_calibration(self):
        self.r['calibration']={'pipeline_sha256':pipeline_binding(self.r),'artifact_sha256':'e'*64,'domain_id':self.r['domain_id'],'correctness_ppm':800000}
        self.r['pipeline']['epoch']+=1;self.reject()
    def test_cache_principal(self):
        a=cache_key(self.r,'f'*64,'state');self.r['principal']='other'
        self.assertNotEqual(a,cache_key(self.r,'f'*64,'state'))
    def test_cache_epoch(self):
        a=cache_key(self.r,'f'*64,'action');self.r['pipeline']['epoch']+=1
        self.assertNotEqual(a,cache_key(self.r,'f'*64,'action'))
    def test_cache_projection(self):
        a=cache_key(self.r,'f'*64,'state');self.r['pipeline']['projection_sha256']='9'*64
        self.assertNotEqual(a,cache_key(self.r,'f'*64,'state'))
    def test_cache_retention(self):
        a=cache_key(self.r,'f'*64,'state');self.r['retention_scope']='other'
        self.assertNotEqual(a,cache_key(self.r,'f'*64,'state'))
    def test_cache_sides(self):self.assertNotEqual(cache_key(self.r,'f'*64,'state'),cache_key(self.r,'f'*64,'action'))

class PermutationTests(unittest.TestCase):
    def test_label_alignment(self):
        a=permutation_average(['a','b'],[['a','b'],['b','a']],[{'a':800000,'b':200000},{'b':400000,'a':600000}])
        self.assertEqual(a,{'a':700000,'b':300000})
    def test_missing_subcall(self):
        with self.assertRaises(ContractError):permutation_average(['a','b'],[['a','b'],['b','a']],[{'a':500000,'b':500000}])
    def test_missing_label(self):
        with self.assertRaises(ContractError):permutation_average(['a','b'],[['a','b']],[{'a':1000000}])
    def test_foreign_schedule(self):
        with self.assertRaises(ContractError):permutation_average(['a','b'],[['a','c']],[{'a':500000,'b':500000}])
    def test_ordinal_rejected(self):
        with self.assertRaises(ContractError):permutation_average(['a','b'],[['a','b']],[{'a':500000,'b':500000}],ordinal=True)
    def test_schedule_budget(self):
        with self.assertRaises(ContractError):permutation_average(['a'],[['a']]*65,[{'a':1000000}]*65)
    def test_rounding_preserves_sum(self):
        a=permutation_average(['a','b','c'],[['a','b','c']]*3,[{'a':1000000,'b':0,'c':0},{'a':0,'b':1000000,'c':0},{'a':0,'b':0,'c':1000000}])
        self.assertEqual(sum(a.values()),1000000);self.assertEqual(a['a'],333334)
    def test_all_fixed_score_permutations(self):
        scores={'c':3,'a':3,'b':1}
        for order in itertools.permutations(scores):
            self.assertEqual(stable_rank({k:scores[k] for k in order}),['a','c','b'])

class ChecklistTests(unittest.TestCase):
    def setUp(self):
        self.r=[{'id':'tests','applicable':True,'required':True},{'id':'style','applicable':True,'required':False}]
        self.a=[{'id':'tests','status':'PASS','evidence_ids':['check-1']},{'id':'style','status':'FAIL','evidence_ids':['human-1']}]
    def test_advisory_cannot_override(self):self.assertEqual(checklist(self.r,self.a),'PASS')
    def test_hard_failure(self):self.a[0]['status']='FAIL';self.assertEqual(checklist(self.r,self.a),'FAIL')
    def test_unknown_blocks(self):self.a[0]['status']='UNKNOWN';self.assertEqual(checklist(self.r,self.a),'UNKNOWN')
    def test_missing_criterion(self):
        with self.assertRaises(ContractError):checklist(self.r,self.a[1:])
    def test_duplicate_criterion(self):
        with self.assertRaises(ContractError):checklist(self.r,self.a+[self.a[0]])
    def test_self_claim_not_evidence(self):
        self.a[0]['evidence_ids']=[]
        with self.assertRaises(ContractError):checklist(self.r,self.a)
    def test_candidate_na_rejected(self):
        self.a[0]['status']='NOT_APPLICABLE'
        with self.assertRaises(ContractError):checklist(self.r,self.a)
    def test_no_required_applicable_not_success(self):
        self.r[0]['applicable']=False;self.a[0]['status']='NOT_APPLICABLE'
        with self.assertRaises(ContractError):checklist(self.r,self.a)

class ContextTests(unittest.TestCase):
    def setUp(self):
        self.events=[{'id':'task','kind':'original','tokens':10,'pinned':True,'depends_on':[]},
         {'id':'call','kind':'original','tokens':10,'pinned':False,'depends_on':[]},
         {'id':'result','kind':'original','tokens':10,'pinned':True,'depends_on':['call']},
         {'id':'chatter','kind':'original','tokens':20,'pinned':False,'depends_on':[]}]
    def test_pair_closure(self):self.assertEqual(context_projection(self.events,30)['event_ids'],['task','call','result'])
    def test_pin_budget_refuses(self):
        with self.assertRaises(ContractError):context_projection(self.events,29)
    def test_no_recursive_summary(self):
        self.events[0]['kind']='summary'
        with self.assertRaises(ContractError):context_projection(self.events,100)
    def test_missing_original(self):
        with self.assertRaises(ContractError):context_projection(self.events[:1]+self.events[2:],100)
    def test_cycle(self):
        self.events[1]['depends_on']=['result']
        with self.assertRaises(ContractError):context_projection(self.events,100)
    def test_original_digest_changes(self):
        a=context_projection(self.events,30)['source_sha256'];self.events[0]['tokens']=9
        self.assertNotEqual(a,context_projection(self.events,30)['source_sha256'])
    def test_full_context(self):self.assertTrue(context_projection(self.events,100)['complete'])
    def test_negative_tokens(self):
        self.events[3]['tokens']=-1
        with self.assertRaises(ContractError):context_projection(self.events,30)

class RouterTests(unittest.TestCase):
    def setUp(self):
        self.c=[{'id':'local','principals':['p'],'healthy':True,'compatible':True,'ceiling_micro':10},
                {'id':'private','principals':['other'],'healthy':True,'compatible':True,'ceiling_micro':1}]
    def test_only_eligible_visible(self):self.assertEqual(eligible_route(self.c,{'local':1},principal='p',remaining_micro=10),'local')
    def test_ineligible_score_rejected(self):
        with self.assertRaises(ContractError):eligible_route(self.c,{'local':1,'private':999},principal='p',remaining_micro=10)
    def test_no_budget(self):
        with self.assertRaises(ContractError):eligible_route(self.c,{},principal='p',remaining_micro=1)
    def test_revoked(self):
        with self.assertRaises(ContractError):eligible_route(self.c,{},principal='p',remaining_micro=10,revoked={'local'})
    def test_unhealthy(self):
        self.c[0]['healthy']=False
        with self.assertRaises(ContractError):eligible_route(self.c,{},principal='p',remaining_micro=10)
    def test_incompatible(self):
        self.c[0]['compatible']=False
        with self.assertRaises(ContractError):eligible_route(self.c,{},principal='p',remaining_micro=10)

class EvolutionTests(unittest.TestCase):
    def setUp(self):
        self.policy={'id':'frozen-1','incumbent_sha256':'a'*64,'seen_attempts':['old-1'],'allowed_components':['prompt','retrieval'],'edit_budget':1,'cost_ceiling_micro':10,'statistical_policy_id':'predeclared-method','min_delta_ppm':100}
        self.p={'policy_id':'frozen-1','incumbent_sha256':'a'*64,'attempt_id':'new-1','intervention':'harness','components':['prompt'],'final_test_exposed':False,'critic':'PASS','total_cost_micro':5,'disposition':'QUALIFIED_COMPARISON','statistical_policy_id':'predeclared-method','quality_delta_lower_ppm':101}
    def test_candidate_not_activation(self):self.assertEqual(evolution_screen(self.p,self.policy),'CANDIDATE_FOR_INDEPENDENT_ADMISSION')
    def test_protected_component(self):
        self.p['components']=['grader']
        with self.assertRaises(ContractError):evolution_screen(self.p,self.policy)
    def test_edit_budget(self):
        self.p['components']=['prompt','retrieval']
        with self.assertRaises(ContractError):evolution_screen(self.p,self.policy)
    def test_attempt_collision(self):
        self.p['attempt_id']='old-1'
        with self.assertRaises(ContractError):evolution_screen(self.p,self.policy)
    def test_holdout_leakage(self):
        self.p['final_test_exposed']=True
        with self.assertRaises(ContractError):evolution_screen(self.p,self.policy)
    def test_small_delta_rejected(self):self.p['quality_delta_lower_ppm']=99;self.assertEqual(evolution_screen(self.p,self.policy),'REJECTED')
    def test_inconclusive_preserved(self):self.p['disposition']='UNKNOWN';self.assertEqual(evolution_screen(self.p,self.policy),'INCONCLUSIVE')
    def test_critic_rejection(self):self.p['critic']='FAIL';self.assertEqual(evolution_screen(self.p,self.policy),'REJECTED')
    def test_policy_drift(self):
        self.p['statistical_policy_id']='tuned-on-test'
        with self.assertRaises(ContractError):evolution_screen(self.p,self.policy)

class CascadeTests(unittest.TestCase):
    def setUp(self):self.c=Cascade(10);self.c.reserve();self.c.draft(b'patch-A')
    def verified(self):self.c.verify(self.c.candidate_sha256,True)
    def test_verified_authorized_commit(self):self.verified();self.c.commit(authorized=True,final_check=True);self.assertEqual(self.c.phase,'COMMITTED')
    def test_unverified_commit(self):
        with self.assertRaises(ContractError):self.c.commit(authorized=True,final_check=True)
    def test_wrong_verifier_digest(self):
        with self.assertRaises(ContractError):self.c.verify('e'*64,True)
    def test_permission_denied(self):
        self.verified()
        with self.assertRaises(ContractError):self.c.commit(authorized=False,final_check=True)
    def test_final_failure(self):
        self.verified()
        with self.assertRaises(ContractError):self.c.commit(authorized=True,final_check=False)
    def test_repair_invalidates_verification(self):
        self.verified();self.c.draft(b'patch-B')
        with self.assertRaises(ContractError):self.c.commit(authorized=True,final_check=True)
    def test_cancel_cannot_commit(self):
        self.verified();self.c.cancel()
        with self.assertRaises(ContractError):self.c.commit(authorized=True,final_check=True)
    def test_cancel_cannot_redraft(self):
        self.c.cancel()
        with self.assertRaises(ContractError):self.c.draft(b'patch')
    def test_duplicate_commit(self):
        self.verified();self.c.commit(authorized=True,final_check=True)
        with self.assertRaises(ContractError):self.c.commit(authorized=True,final_check=True)
    def test_budget(self):
        self.c.charge('draft',6)
        with self.assertRaises(ContractError):self.c.charge('verify',5)
    def test_duplicate_charge(self):
        self.c.charge('draft',6)
        with self.assertRaises(ContractError):self.c.charge('draft',1)
    def test_cancelled_billing_is_retained(self):self.c.cancel();self.c.charge('late-bill',2);self.assertEqual(self.c.spent_micro,2)
    def test_no_spend_before_reservation(self):
        with self.assertRaises(ContractError):Cascade(10).charge('x',1)

class EconomicsTests(unittest.TestCase):
    def setUp(self):
        self.e={'principal':'p','task_id':'t1','attempt_id':'a1','call_id':'c1','usage_status':'COMPLETE','price_revision':'fixture-price','tokens':{'uncached_input':10,'cache_read':20,'cache_write':5,'output':3},'price_micro_per_million':{'uncached_input':1000000,'cache_read':1000000,'cache_write':1000000,'output':1000000},'total_input':35,'observed_micro':38,'reserved_micro':100}
    def ledger(self,entries=None):return task_ledger(entries or [self.e],principal='p',assigned_tasks=['t1','t2'])
    def test_total_cost(self):self.assertEqual(self.ledger()['known_micro'],38)
    def test_denominator_includes_uncompleted_task(self):self.assertEqual(self.ledger()['assigned_tasks'],2)
    def test_duplicate_call(self):
        with self.assertRaises(ContractError):self.ledger([self.e,self.e])
    def test_cross_principal(self):
        self.e['principal']='q'
        with self.assertRaises(ContractError):self.ledger()
    def test_unassigned_task(self):
        self.e['task_id']='t3'
        with self.assertRaises(ContractError):self.ledger()
    def test_overlap_categories(self):
        self.e['total_input']=55
        with self.assertRaises(ContractError):self.ledger()
    def test_unknown_retains_reservation(self):
        self.e.update(usage_status='UNKNOWN',observed_micro=None)
        r=self.ledger();self.assertEqual(r['unknown_reserved_micro'],100);self.assertFalse(r['complete'])
    def test_unknown_not_zero(self):
        self.e.update(usage_status='UNKNOWN',observed_micro=0)
        with self.assertRaises(ContractError):self.ledger()
    def test_negative_usage(self):
        self.e['tokens']['output']=-1
        with self.assertRaises(ContractError):self.ledger()
    def test_wrong_tariff_total(self):
        self.e['observed_micro']=1
        with self.assertRaises(ContractError):self.ledger()
    def test_unpriced(self):
        self.e['price_revision']=''
        with self.assertRaises(ContractError):self.ledger()

if __name__=='__main__':unittest.main()
