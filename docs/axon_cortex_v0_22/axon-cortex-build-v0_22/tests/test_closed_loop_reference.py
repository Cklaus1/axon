"""Executable model-free 0.22 contract examples. Never runtime evidence."""
from copy import deepcopy
import json
from pathlib import Path
import sys
import unittest
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import closed_loop_reference as c


def fixture():return json.loads((ROOT/'fixtures/closed_loop_v022/bundle.json').read_text())
def ref(char):return 'cl22:'+char*64


class ParsingTests(unittest.TestCase):
    def test_valid_json(self):self.assertEqual(c.strict_json(b'{"a":1,"b":false}'),{'a':1,'b':False})
    def test_canonical_key_order(self):self.assertEqual(c.content_ref({'b':2,'a':1}),c.content_ref({'a':1,'b':2}))
    def test_candidate_order_preserved(self):self.assertNotEqual(c.content_ref(['a','b']),c.content_ref(['b','a']))
    def test_unicode_not_silently_normalized(self):self.assertNotEqual(c.content_ref('e\u0301'),c.content_ref('\u00e9'))
    def test_integer_edge(self):self.assertEqual(c.strict_json(str(c.MAX_INTEGER)),c.MAX_INTEGER)
    def test_boolean_not_money(self):
        with self.assertRaises(c.Refusal):c.integer(True)
    def test_nonstring_dictionary_key(self):
        with self.assertRaises(c.Refusal):c.canonical({1:'x'})
    def test_nonjson_object(self):
        with self.assertRaises(c.Refusal):c.canonical(set(['x']))
    def test_quoted_braces_do_not_count_as_depth(self):self.assertEqual(c.strict_json(json.dumps('{'*99)),'{'*99)


def parsing_case(value):
    def test(self):
        with self.assertRaises(c.Refusal):c.strict_json(value)
    return test
for name,value in {
 'duplicate':'{"x":1,"x":2}', 'escaped_duplicate':'{"x":1,"\\u0078":2}',
 'nested_duplicate':'{"a":{"x":1,"x":2}}','float':'{"x":1.0}', 'exponent':'{"x":1e2}',
 'nan':'NaN','infinity':'Infinity','negative_infinity':'-Infinity',
 'unsafe_integer':str(c.MAX_INTEGER+1),'unsafe_negative':str(-c.MAX_INTEGER-1),
 'huge_integer':'9'*5000,'invalid_utf8':b'"\xff"', 'surrogate':'"\\ud800"',
 'trailing':'{}{}','empty':'','oversize':' '* (c.MAX_BYTES+1),
 'deep':'['*(c.MAX_DEPTH+1)+'0'+']'*(c.MAX_DEPTH+1),'malformed':'{"x":}',
}.items():setattr(ParsingTests,'test_reject_'+name,parsing_case(value))


class SchemaTests(unittest.TestCase):
    def setUp(self):self.b=fixture()
    def test_positive_projections(self):
        for name in ['policy','context','episode','transition']:c.validate(name,self.b[name])
    def test_template_shape_not_ready(self):
        p=json.loads((ROOT/'fixtures/closed_loop_v022/pilot-template.json').read_text());c.validate('pilot',p)
        with self.assertRaises(c.Refusal):c.pilot_ready(p)
    def test_unknown_schema_refused(self):
        with self.assertRaises(c.Refusal):c.validate('../../bad',{})
    def test_unknown_episode_cost_cannot_be_zero(self):
        self.b['episode']['usage'].update(state='unknown',cost_micro=0)
        with self.assertRaises(c.Refusal):c.validate('episode',self.b['episode'])
    def test_zero_checks_cannot_pass(self):
        self.b['episode']['verification']['matched_checks']=0
        with self.assertRaises(c.Refusal):c.validate('episode',self.b['episode'])
    def test_unknown_outcome_cannot_pass(self):
        self.b['episode']['status']='outcome_unknown'
        with self.assertRaises(c.Refusal):c.validate('episode',self.b['episode'])
    def test_final_usage_has_no_pending_liability(self):
        self.b['episode']['usage']['unresolved_liability_micro']=2
        with self.assertRaises(c.Refusal):c.validate('episode',self.b['episode'])
    def test_pause_target_must_be_null(self):
        self.b['transition']['kind']='pause'
        with self.assertRaises(c.Refusal):c.validate('transition',self.b['transition'])
    def test_activation_requires_admission_ref(self):
        self.b['transition']['admission_ref']=None
        with self.assertRaises(c.Refusal):c.validate('transition',self.b['transition'])


def missing_field_case(name,field):
    def test(self):
        value=deepcopy(self.b[name]);del value[field]
        with self.assertRaises(c.Refusal):c.validate(name,value)
    return test
for name,fields in {
 'policy':['scope','controls_ref','candidate_set_ref','parent_policy_ref','shortlist'],
 'context':['identity','observed','observed_issuer_ref','expires_ms','authority_epoch'],
 'episode':['context_ref','acf_receipt_ref','identity','usage','verification','corpus_role'],
 'transition':['expected_policy_ref','expected_epoch','next_epoch','issuer_ref','scope'],
}.items():
    for field in fields:setattr(SchemaTests,f'test_{name}_requires_{field}',missing_field_case(name,field))


def unknown_field_case(name):
    def test(self):
        value=deepcopy(self.b[name]);value['unnegotiated']=True
        with self.assertRaises(c.Refusal):c.validate(name,value)
    return test
for name in ['policy','context','episode','transition']:setattr(SchemaTests,'test_'+name+'_closed_object',unknown_field_case(name))


class BoundaryTests(unittest.TestCase):
    def setUp(self):self.b=fixture()
    def preflight(self):c.preflight(self.b['context'],now_ms=150,current_epoch=7,trusted_observers={'fixture:observer'})
    def bind(self):c.bind_episode(self.b['episode'],self.b['policy'],self.b['context'],current_epoch=7,trusted_verifiers={'fixture:independent-verifier'},subject_issuers={'fixture:worker'})
    def test_positive_composed_boundary(self):
        self.preflight();self.bind();c.bind_acf(self.b['episode'],self.b['acf_request'],self.b['acf_receipt'],self.b['policy_projection'])
        self.assertTrue(self.b['fixture_only']);self.assertFalse(self.b['source_runtime_executed'])
    def test_parent_echo_refused(self):
        self.b['context']['observed_issuer_ref']=self.b['context']['expected_issuer_ref']
        with self.assertRaises(c.Refusal):self.preflight()
    def test_untrusted_observer(self):
        self.b['context']['observed_issuer_ref']='worker:untrusted'
        with self.assertRaises(c.Refusal):self.preflight()
    def test_expired_context(self):
        self.b['context']['expires_ms']=149
        with self.assertRaises(c.Refusal):self.preflight()
    def test_future_context(self):
        self.b['context']['created_ms']=151
        with self.assertRaises(c.Refusal):self.preflight()
    def test_primary_worktree(self):
        for k in ['expected','observed']:self.b['context'][k]['is_primary_worktree']=True
        with self.assertRaises(c.Refusal):self.preflight()
    def test_readonly_role_no_writes(self):
        for k in ['expected','observed']:
            self.b['context'][k]['role']='critic';self.b['context'][k]['write_paths']=[]
        self.preflight()
    def test_readonly_role_with_writes(self):
        for k in ['expected','observed']:self.b['context'][k]['role']='critic'
        with self.assertRaises(c.Refusal):self.preflight()
    def test_implementation_empty_writes(self):
        for k in ['expected','observed']:self.b['context'][k]['write_paths']=[]
        with self.assertRaises(c.Refusal):self.preflight()
    def test_stale_context_epoch(self):
        self.b['context']['authority_epoch']=6
        with self.assertRaises(c.Refusal):self.preflight()
    def test_forged_verifier(self):
        self.b['episode']['verification']['issuer_ref']='fixture:worker'
        with self.assertRaises(c.Refusal):self.bind()
    def test_changed_output_after_verification(self):
        self.b['episode']['output_workspace_ref']='acf1:'+'9'*64
        with self.assertRaises(c.Refusal):self.bind()
    def test_changed_context_bytes(self):
        self.b['context']['context_id']='substituted'
        with self.assertRaises(c.Refusal):self.bind()
    def test_changed_policy_bytes(self):
        self.b['policy']['shortlist']=['read']
        with self.assertRaises(c.Refusal):self.bind()
    def test_cross_tenant(self):
        self.b['episode']['scope']['tenant_id']='other'
        with self.assertRaises(c.Refusal):self.bind()
    def test_policy_projection_not_scheme_replace(self):
        self.b['policy_projection']['acf_policy_digest']=ref('3')
        with self.assertRaises(c.Refusal):c.bind_acf(self.b['episode'],self.b['acf_request'],self.b['acf_receipt'],self.b['policy_projection'])
    def test_fabric_receipt_substitution(self):
        self.b['acf_receipt']['execution_id']='different'
        self.b['episode']['acf_receipt_ref']=c.content_ref(self.b['acf_receipt'])
        with self.assertRaises(c.Refusal):c.bind_acf(self.b['episode'],self.b['acf_request'],self.b['acf_receipt'],self.b['policy_projection'])
    def test_fabric_status_loss(self):
        self.b['acf_receipt']['status']='failed'
        self.b['episode']['acf_receipt_ref']=c.content_ref(self.b['acf_receipt'])
        with self.assertRaises(c.Refusal):c.bind_acf(self.b['episode'],self.b['acf_request'],self.b['acf_receipt'],self.b['policy_projection'])
    def test_shortlist_is_not_permission(self):
        p=self.b['policy'];p['shortlist']=['unapproved-shell']
        with self.assertRaises(c.Refusal):c.shortlist(p,eligible={'read','search'},candidate_set_ref=ref('1'),controls_ref=ref('2'),scope=p['scope'])
    def test_shortlist_control_drift(self):
        p=self.b['policy']
        with self.assertRaises(c.Refusal):c.shortlist(p,eligible={'read','search'},candidate_set_ref=ref('1'),controls_ref=ref('3'),scope=p['scope'])
    def test_shortlist_positive(self):
        p=self.b['policy'];self.assertEqual(c.shortlist(p,eligible={'read','search'},candidate_set_ref=ref('1'),controls_ref=ref('2'),scope=p['scope']),('read','search'))
    def test_effect_narrowing(self):c.narrowed_effects(['read'],['src/main.rs'],['read','search'],['src/main.rs','src/lib.rs'])
    def test_path_scope_expansion(self):
        with self.assertRaises(c.Refusal):c.narrowed_effects(['read'],['secret.txt'],['read'],['src/main.rs'])
    def test_tool_scope_expansion(self):
        with self.assertRaises(c.Refusal):c.narrowed_effects(['shell'],[],['read'],[])
    def test_mechanism_test_not_learning(self):self.assertFalse(c.learning_eligible(self.b['episode']))
    def test_confirmation_not_learning(self):
        self.b['episode']['corpus_role']='confirmation';self.assertFalse(c.learning_eligible(self.b['episode']))
    def test_reporting_not_learning(self):
        self.b['episode']['corpus_role']='reporting';self.assertFalse(c.learning_eligible(self.b['episode']))
    def test_discovery_eligible_after_independent_outcome(self):
        self.b['episode']['corpus_role']='discovery';self.assertTrue(c.learning_eligible(self.b['episode']))


def context_drift_case(field):
    def test(self):
        self.b['context']['observed'][field] = ref('8') if field in ['model_ref','namespace_ref'] else ('acf1:'+'8'*64 if field=='workspace_ref' else 'mismatch')
        with self.assertRaises(c.Refusal):self.preflight()
    return test
for field in ['repo_id','base_commit','workspace_ref','branch','worktree_id','working_directory','build_namespace','model_ref','namespace_ref']:
    setattr(BoundaryTests,'test_context_drift_'+field,context_drift_case(field))


def identity_drift_case(field):
    def test(self):
        self.b['episode']['identity'][field]='other'
        with self.assertRaises(c.Refusal):self.bind()
    return test
for field in ['task_id','arm_id','trial_id','attempt_id','operation_id','execution_id']:
    setattr(BoundaryTests,'test_identity_drift_'+field,identity_drift_case(field))


def path_case(path):
    def test(self):
        with self.assertRaises(c.Refusal):c.concrete_path(path)
    return test
for name,path in {'parent':'../secret','absolute':'/etc/passwd','drive':'C:/secret','backslash':'src\\main.rs','glob':'src/**','empty_part':'a//b','dot':'a/./b','nul':'a\x00b'}.items():
    setattr(BoundaryTests,'test_unproved_path_'+name,path_case(path))


class LifecycleTests(unittest.TestCase):
    def test_duplicate_identical_operation(self):
        j=c.OperationJournal();self.assertEqual(j.submit('op',{'task':'a'}),j.submit('op',{'task':'a'}))
    def test_conflicting_operation(self):
        j=c.OperationJournal();j.submit('op',{'task':'a'})
        with self.assertRaises(c.Refusal):j.submit('op',{'task':'b'})
    def test_new_trial_is_not_deduplicated(self):
        j=c.OperationJournal();j.submit('one',{'task':'same'});j.submit('two',{'task':'same'});self.assertEqual(len(j.rows),2)
    def test_cancel_not_cleanup(self):
        j=c.OperationJournal();j.submit('op',{});j.start('op');j.cancel('op')
        self.assertEqual(j.rows['op']['state'],'cancel_requested');self.assertFalse(j.rows['op']['cleanup_complete'])
    def test_possible_effect_timeout_not_retry(self):
        j=c.OperationJournal();j.submit('op',{});j.start('op');j.timeout('op')
        self.assertEqual(j.rows['op']['state'],'outcome_unknown')
        with self.assertRaises(c.Refusal):j.start('op')
    def test_unknown_cannot_be_called_clean(self):
        j=c.OperationJournal();j.submit('op',{});j.start('op')
        with self.assertRaises(c.Refusal):j.reconcile('op',observed_state='outcome_unknown',cleanup_complete=True)
    def test_terminal_cannot_be_overwritten(self):
        j=c.OperationJournal();j.submit('op',{});j.start('op');j.reconcile('op',observed_state='completed',cleanup_complete=True)
        with self.assertRaises(c.Refusal):j.reconcile('op',observed_state='failed',cleanup_complete=True)
    def test_aggregate_reservations(self):
        b=c.ReservationBook(100);b.reserve('a',60)
        with self.assertRaises(c.Refusal):b.reserve('b',60)
    def test_changed_duplicate_reservation(self):
        b=c.ReservationBook(100);b.reserve('a',50)
        with self.assertRaises(c.Refusal):b.reserve('a',51)
    def test_duplicate_reservation_is_idempotent(self):
        b=c.ReservationBook(100);b.reserve('a',50);b.reserve('a',50);self.assertEqual(b.exposure(),50)
    def test_unknown_holds_budget(self):
        b=c.ReservationBook(100);b.reserve('a',100);b.observe('a',sequence=1,cumulative_cost=20,liability=40,final=False,cleanup_complete=False)
        self.assertEqual(b.exposure(),100)
        with self.assertRaises(c.Refusal):b.reserve('b',1)
    def test_final_releases_unused_not_spent(self):
        b=c.ReservationBook(100);b.reserve('a',100);b.observe('a',sequence=1,cumulative_cost=25,liability=0,final=True,cleanup_complete=True)
        self.assertEqual(b.exposure(),25);b.reserve('b',75)
    def test_observed_overspend_retained(self):
        b=c.ReservationBook(100);b.reserve('a',50);b.observe('a',sequence=1,cumulative_cost=120,liability=0,final=True,cleanup_complete=True)
        self.assertEqual(b.exposure(),120)
        with self.assertRaises(c.Refusal):b.reserve('b',1)
    def test_no_final_before_cleanup(self):
        b=c.ReservationBook(100);b.reserve('a',50)
        with self.assertRaises(c.Refusal):b.observe('a',sequence=1,cumulative_cost=25,liability=0,final=True,cleanup_complete=False)
    def test_duplicate_cost_not_double_charged(self):
        b=c.ReservationBook(100);b.reserve('a',100)
        for _ in range(2):b.observe('a',sequence=1,cumulative_cost=25,liability=0,final=True,cleanup_complete=True)
        self.assertEqual(b.exposure(),25)
    def test_conflicting_cost_duplicate(self):
        b=c.ReservationBook(100);b.reserve('a',100);b.observe('a',sequence=1,cumulative_cost=25,liability=0,final=True,cleanup_complete=True)
        with self.assertRaises(c.Refusal):b.observe('a',sequence=1,cumulative_cost=10,liability=0,final=True,cleanup_complete=True)
    def test_missing_usage_sequence(self):
        b=c.ReservationBook(100);b.reserve('a',100)
        with self.assertRaises(c.Refusal):b.observe('a',sequence=2,cumulative_cost=25,liability=0,final=True,cleanup_complete=True)


class PromotionTests(unittest.TestCase):
    def setUp(self):
        self.b=fixture();self.t=self.b['transition'];self.p=c.PolicyPointer(self.t['scope'],self.t['expected_policy_ref'],7,ref('2'))
        self.admissions={self.t['admission_ref']:{'target_ref':self.t['target_policy_ref'],'scope':self.t['scope'],'controls_ref':ref('2'),'current':True,'revoked':False,'mechanism_test':True}}
    def apply(self):return self.p.apply(self.t,admissions=self.admissions,trusted_admitters={'fixture:admission-authority'})
    def test_activation_and_later_task_pin(self):
        before=self.p.start_task();r=self.apply();after=self.p.start_task()
        self.assertEqual(before,(ref('0'),7));self.assertEqual(after,(self.t['target_policy_ref'],8));self.assertFalse(r['runtime_qualified']);self.assertFalse(r['measured_improvement_supported'])
    def test_same_transition_idempotent(self):self.assertEqual(self.apply(),self.apply())
    def test_changed_transition_id_conflict(self):
        self.apply();self.t['reason_ref']=ref('1')
        with self.assertRaises(c.Refusal):self.apply()
    def test_competing_stale_cas(self):
        self.apply();self.t['transition_id']='competing'
        with self.assertRaises(c.Refusal):self.apply()
    def test_revoked_target(self):
        self.admissions[self.t['admission_ref']]['revoked']=True
        with self.assertRaises(c.Refusal):self.apply()
    def test_wrong_admission_subject(self):
        self.admissions[self.t['admission_ref']]['target_ref']=ref('f')
        with self.assertRaises(c.Refusal):self.apply()
    def test_wrong_control_profile(self):
        self.admissions[self.t['admission_ref']]['controls_ref']=ref('f')
        with self.assertRaises(c.Refusal):self.apply()
    def test_unauthorized_admitter(self):
        self.t['issuer_ref']='fixture:worker'
        with self.assertRaises(c.Refusal):self.apply()
    def test_epoch_skip(self):
        self.t['next_epoch']=9
        with self.assertRaises(c.Refusal):self.apply()
    def test_fixture_cannot_become_measured_claim(self):
        self.t['mechanism_test']=False
        with self.assertRaises(c.Refusal):self.apply()
    def test_rollback_revalidates_predecessor(self):
        self.apply();new=self.p.active_ref;self.t.update(transition_id='rollback',kind='rollback',expected_policy_ref=new,target_policy_ref=ref('0'),expected_epoch=8,next_epoch=9)
        self.admissions[self.t['admission_ref']].update(target_ref=ref('0'),revoked=True)
        with self.assertRaises(c.Refusal):self.apply()
        self.assertEqual(self.p.active_ref,new)
    def test_valid_rollback_is_new_epoch(self):
        self.apply();self.t.update(transition_id='rollback',kind='rollback',expected_policy_ref=self.p.active_ref,target_policy_ref=ref('0'),expected_epoch=8,next_epoch=9)
        self.admissions[self.t['admission_ref']]['target_ref']=ref('0');self.apply()
        self.assertEqual(self.p.start_task(),(ref('0'),9))
    def test_pause_when_no_safe_predecessor(self):
        self.t.update(kind='pause',target_policy_ref=None,admission_ref=None);self.apply()
        with self.assertRaises(c.Refusal):self.p.start_task()


class QualificationPremiseTests(unittest.TestCase):
    def setUp(self):
        self.b=fixture();self.required=self.b['acf_request']['required']
        p=json.loads((ROOT/'integration/compute-fabric-v0_1-reference/contracts/examples/profile_source_inspected.json').read_text())
        p.update(engine='native_process',enclosure='kvm_microvm',guest_kind='linux_init',support_status='verified',evidence_refs=['fixture:physical-evidence'],functionality=['registered_check','logical_workspace'],enforcement=['host_filesystem_boundary','host_network_deny','descendant_resource_limits','current_epoch_rebind','hardware_isolation','output_limit'])
        self.profile=p
    def eligible(self):return c.eligible_profile(self.profile,self.required,expected_config=self.profile['configuration_digest'],independently_qualified_refs={'fixture:physical-evidence'})
    def test_positive_only_under_explicit_fixture_premises(self):self.assertTrue(self.eligible())
    def test_unverified_flag_refused(self):self.profile['support_status']='experimental';self.assertFalse(self.eligible())
    def test_worker_claimed_evidence_refused(self):self.profile['evidence_refs']=['worker:claims'];self.assertFalse(self.eligible())
    def test_process_not_hardware(self):self.profile['enclosure']='process_scoped';self.assertFalse(self.eligible())
    def test_network_broker_out_of_scope(self):self.required['network_mode']='brokered';self.assertFalse(self.eligible())
    def test_ram_checkpoint_out_of_scope(self):self.required['checkpoint_kind']='machine_state';self.assertFalse(self.eligible())
    def test_remote_out_of_scope(self):self.profile['placement']='remote';self.assertFalse(self.eligible())
    def test_custom_demo_not_linux(self):self.profile['guest_kind']='axon_kernel_demo';self.assertFalse(self.eligible())
    def test_config_change_requalifies(self):self.assertFalse(c.eligible_profile(self.profile,self.required,expected_config=ref('2'),independently_qualified_refs={'fixture:physical-evidence'}))
    def test_paired_same_trial_invalid(self):
        with self.assertRaises(c.Refusal):c.comparable({'arm_id':'A','trial_id':'x','attempt_id':'1'},{'arm_id':'B','trial_id':'x','attempt_id':'2'},controls=[])
    def test_paired_model_change_invalid(self):
        with self.assertRaises(c.Refusal):c.comparable({'arm_id':'A','trial_id':'x','attempt_id':'1','model':'one'},{'arm_id':'B','trial_id':'y','attempt_id':'2','model':'two'},controls=['model'])
    def test_real_task_comparability_controls(self):c.comparable({'arm_id':'A','trial_id':'x','attempt_id':'1','base':'same'},{'arm_id':'B','trial_id':'y','attempt_id':'2','base':'same'},controls=['base'])

if __name__=='__main__':unittest.main()
