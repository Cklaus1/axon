"""Inert owner-profile tests; no live Axon, MiCode, models or APIs."""
import copy, json, sys, unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import ace_contract_reference as c

class ACEProjectionTests(unittest.TestCase):
    def setUp(self):self.f=json.loads((ROOT/'fixtures/ace/records.json').read_text())
    def reject(self,fn,*args,**kw):
        with self.assertRaises(c.ContractError):fn(*args,**kw)
    def test_three_result_families(self):
        for f in ['choice','binary','ordinal']:
            with self.subTest(family=f):self.assertEqual(c.validate_result(self.f[f+'_request'],self.f[f+'_result'],self.f['physical_profile']),'ProposedDecision')
    def test_native_head_does_not_require_kv(self):
        c.validate_profile(self.f['physical_profile'])
        c.require_features(self.f['features'],['Choice','dynamic_candidates'],backend_ref='fixture:backend',deployment='LocalSidecar')
    def test_tuple_axis_corruption(self):
        for key,val in [('air_node_kind','Rule'),('cognitive_class','RULE'),('operation_kind','PROJECT'),('capability_class','RULE'),('physical_mechanism','validated_text'),('mapping_id','unknown')]:
            with self.subTest(axis=key):
                p=copy.deepcopy(self.f['physical_profile']);p[key]=val;self.reject(c.validate_profile,p)
    def test_np_is_generate_proposal_with_explicit_artifact(self):
        p=self.f['physical_profile'];p.update(mapping_id='neural.transform',air_node_kind='Generate',cognitive_class='GENERATE',operation_kind='TRANSFORM',capability_class='NEURAL_PROGRAM',physical_mechanism='learned_function',artifact_ref='fixture:np')
        c.validate_profile(p);p['artifact_ref']=None;self.reject(c.validate_profile,p)
    def test_unknown_required_feature(self):
        self.reject(c.require_features,self.f['features'],['native_state'],backend_ref='fixture:backend',deployment='LocalSidecar')
    def test_feature_requires_exact_deployment(self):self.reject(c.require_features,self.f['features'],['Choice'],backend_ref='fixture:backend',deployment='RemoteService')
    def test_support_evidence_and_explicit_emulation(self):
        f=self.f['features'];f['features']['Choice']['evidence_ref']=None
        self.reject(c.require_features,f,['Choice'],backend_ref='fixture:backend',deployment='LocalSidecar')
        f['features']['Choice']={'status':'Emulated','evidence_ref':'fixture:evidence','emulation_ref':'fixture:emulation'}
        self.reject(c.require_features,f,['Choice'],backend_ref='fixture:backend',deployment='LocalSidecar')
        c.require_features(f,['Choice'],backend_ref='fixture:backend',deployment='LocalSidecar',allowed_emulations=['fixture:emulation'])
    def test_parser_duplicate_nonfinite_invalid_utf8_size_depth(self):
        for raw in ['{"a":1,"a":2}','{"x":NaN}','{"x":1e999}',b'"\xff"']:
            with self.subTest(raw=repr(raw)):self.reject(c.parse_json,raw)
        self.reject(c.parse_json,'"'+'x'*30+'"',max_bytes=8)
        self.reject(c.parse_json,'[[[[0]]]]',max_depth=2)
    def test_unknown_fields_and_variants(self):
        r=self.f['choice_result'];r['confidence']=1;self.reject(c.validate_record,r);del r['confidence'];r['outcome']='VerifiedComplete';self.reject(c.validate_record,r)
    def test_candidate_content_and_order_hashes(self):
        q=self.f['choice_request'];q['candidates'][0]['description']='changed';self.reject(c.validate_request,q)
        q=self.f['binary_request'];q['candidate_order_digest']='0'*64;self.reject(c.validate_request,q)
    def test_duplicate_candidates(self):
        q=self.f['choice_request'];q['candidates'][1]=q['candidates'][0];self.reject(c.validate_request,q)
    def test_all_request_bindings_checked(self):
        for key in ['request_id','operation_id','question_id','observation_ref','effective_input_ref','candidate_set_digest','candidate_order_digest']:
            r=copy.deepcopy(self.f['choice_result']);r[key]='a'*64 if key.endswith('digest') else 'fixture:changed'
            with self.subTest(key=key):self.reject(c.validate_result,self.f['choice_request'],r)
    def test_top_k_is_not_complete(self):
        r=self.f['choice_result'];r['distribution'].pop();self.reject(c.validate_result,self.f['choice_request'],r)
    def test_partial_scores_cannot_satisfy_required(self):
        r=self.f['choice_result'];r['score_completeness']='partial';self.reject(c.validate_result,self.f['choice_request'],r)
    def test_non_normalized_duplicate_and_unknown_scores(self):
        for dist in [[{'id':'a','p':.9},{'id':'b','p':.3},{'id':'c','p':.1}],[{'id':'a','p':1},{'id':'a','p':0}],[{'id':'z','p':1}]]:
            r=copy.deepcopy(self.f['choice_result']);r['distribution']=dist
            with self.subTest(dist=dist):self.reject(c.validate_result,self.f['choice_request'],r)
    def test_argmax_and_tie_rule(self):
        q=self.f['choice_request'];r=self.f['choice_result'];r['selected_id']='b';self.reject(c.validate_result,q,r)
        r['distribution']=[{'id':'c','p':0},{'id':'b','p':.5},{'id':'a','p':.5}];r['selected_id']='a';c.validate_result(q,r)
        r['selected_id']='b';self.reject(c.validate_result,q,r)
    def test_probability_is_not_boolean(self):
        r=self.f['binary_result'];r['probability']=True;self.reject(c.validate_result,self.f['binary_request'],r)
    def test_binary_event_is_bound(self):
        r=self.f['binary_result'];r['event_ref']='fixture:wrong-event';self.reject(c.validate_result,self.f['binary_request'],r)
    def test_ordinal_distribution_not_expectation_only(self):
        r=self.f['ordinal_result'];r['distribution']=None;self.reject(c.validate_result,self.f['ordinal_request'],r)
    def test_ordinal_expected_value_and_order(self):
        r=self.f['ordinal_result'];r['expectation']=2;self.reject(c.validate_result,self.f['ordinal_request'],r)
        r['expectation']=1.1;r['levels']=list(reversed(r['levels']));self.reject(c.validate_result,self.f['ordinal_request'],r)
    def test_label_only_has_no_invented_probability(self):
        q=self.f['choice_request'];r=self.f['choice_result'];q.update(selection_policy='provider_choice',tie_rule='not_applicable',requires_complete_scores=False);r.update(distribution=None,score_provenance=None,score_completeness='unavailable');self.assertEqual(c.validate_result(q,r),'ProposedLabel')
    def test_generated_estimate_not_native_argmax(self):
        q=self.f['choice_request'];r=self.f['choice_result'];r['score_provenance']['origin']='GeneratedEstimate';self.reject(c.validate_result,q,r)
        q.update(selection_policy='provider_choice',tie_rule='not_applicable');c.validate_result(q,r)
    def test_origin_must_match_physical_mechanism(self):
        r=self.f['choice_result'];r['score_provenance']['origin']='NativeOptionLogit';self.reject(c.validate_result,self.f['choice_request'],r,self.f['physical_profile'])
    def test_correctness_names_event_and_domain(self):
        r=self.f['choice_result'];r['correctness']={'status':'Estimated','value':.9,'event_ref':None,'domain_ref':'fixture:domain','estimator_ref':'fixture:estimator','evidence_ref':'fixture:calibration'};self.reject(c.validate_result,self.f['choice_request'],r)
        r['correctness']['event_ref']='fixture:argmax-correct';c.validate_result(self.f['choice_request'],r)
        r['correctness']['status']='Inapplicable';self.reject(c.validate_result,self.f['choice_request'],r)
    def test_all_absence_variants_preserved_without_values(self):
        for outcome in ['Abstained','Refused','Failed','Canceled','DeadlineExceeded','OutcomeUnknown','Unsupported']:
            r=copy.deepcopy(self.f['choice_result']);r.update(outcome=outcome,absence_reason='fixture:reason',selected_id=None,distribution=None,score_provenance=None,score_completeness='unavailable')
            with self.subTest(outcome=outcome):self.assertEqual(c.validate_result(self.f['choice_request'],r),'NoValue')
    def test_no_fabricated_uniform_failure(self):
        r=self.f['choice_result'];r.update(outcome='Failed',absence_reason='fixture:error');self.reject(c.validate_result,self.f['choice_request'],r)
    def test_state_requires_authorization_and_live_lease_flags(self):
        s=self.f['native_state'];self.assertFalse(c.state_reusable(s,s));self.assertTrue(c.state_reusable(s,s,authorization_current=True,expired=False))
    def test_all_state_bindings_participate(self):
        s=self.f['native_state']
        for key in s['bindings']:
            t=copy.deepcopy(s);t['bindings'][key]+='-changed'
            with self.subTest(binding=key):self.assertFalse(c.state_reusable(s,t,authorization_current=True,expired=False))
    def test_observation_or_rematerialization_not_native(self):
        s=self.f['native_state'];t=copy.deepcopy(s);t['kind']='observation';self.assertFalse(c.state_reusable(s,t,authorization_current=True,expired=False));t=copy.deepcopy(s);t['reuse_mode']='rematerialized';self.assertFalse(c.state_reusable(s,t,authorization_current=True,expired=False))
    def test_branch_order_and_unused_failure(self):
        self.assertEqual(c.join(self.f['branch_plan'],list(reversed(self.f['branch_results'])),active_branch='click',allow_unused_failure=True),'ProposedComposition')
        self.reject(c.join,self.f['branch_plan'],self.f['branch_results'],active_branch='click')
    def test_missing_duplicate_unknown_active_result(self):
        p=self.f['branch_plan'];r=self.f['branch_results']
        for bad in [[r[0],r[2]],r+[r[0]],r+[dict(r[0],id='other')]]:
            self.reject(c.join,p,bad,active_branch='click',allow_unused_failure=True)
    def test_answer_dependencies_are_committed_and_not_cyclic(self):
        p=self.f['branch_plan'];r=self.f['branch_results'];p[1].update(dependency='AnswerDependent',depends_on=['op']);self.reject(c.join,p,r,active_branch='click',allow_unused_failure=True)
        r[1]['committed_dependencies']={'op':'fixture:op-result'};c.join(p,r,active_branch='click',allow_unused_failure=True)
        p[0].update(dependency='AnswerDependent',depends_on=['click']);self.reject(c.join,p,r,active_branch='click',allow_unused_failure=True)
    def test_independent_cannot_consume_sibling(self):
        p=self.f['branch_plan'];p[1].update(dependency='Independent',depends_on=['op']);self.reject(c.join,p,self.f['branch_results'],active_branch='click',allow_unused_failure=True)
    def test_attempt_success_and_failed_observed_producer(self):
        es=self.f['attempts'];c.validate_attempt_stream(es,root_id='fixture:root',budget=2)
        es[1].update(outcome='Failed',accepted_producer_ref=None,committed=False);c.validate_attempt_stream(es,root_id='fixture:root',budget=2)
    def test_duplicate_terminal_and_canceled_late_result(self):
        es=self.f['attempts'];self.reject(c.validate_attempt_stream,es+[es[1]],root_id='fixture:root',budget=2)
        es[1].update(outcome='Canceled',accepted_producer_ref=None,committed=False,remote_status='Unknown');late=copy.deepcopy(es[1]);late.update(kind='LateResult',outcome='Value',result_ref='fixture:late');c.validate_attempt_stream(es+[late],root_id='fixture:root',budget=2)
        late['committed']=True;self.reject(c.validate_attempt_stream,es+[late],root_id='fixture:root',budget=2)
    def test_unknown_cost_does_not_reset_budget(self):
        es=self.f['attempts'];es[1]['actual_cost']=None;c.validate_attempt_stream(es,root_id='fixture:root',budget=2)
        extra=copy.deepcopy(es[0]);extra.update(attempt_id='fixture:retry',parent_attempt_id='fixture:attempt');self.reject(c.validate_attempt_stream,es+[extra],root_id='fixture:root',budget=2)
    def test_unknown_reservation_root_switch_and_hidden_producer(self):
        for mut in ['unknown','root','producer']:
            es=copy.deepcopy(self.f['attempts'])
            if mut=='unknown':es[0]['reservation_upper_bound']=None
            if mut=='root':es[1]['root_id']='fixture:other-root'
            if mut=='producer':es[1]['accepted_producer_ref']='fixture:hidden'
            with self.subTest(mutation=mut):self.reject(c.validate_attempt_stream,es,root_id='fixture:root',budget=2)

if __name__=='__main__':unittest.main()
