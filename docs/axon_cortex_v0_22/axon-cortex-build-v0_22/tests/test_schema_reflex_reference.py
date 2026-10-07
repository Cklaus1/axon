"""Synthetic fail-closed schema, provenance and statistical-policy tests only."""
from __future__ import annotations
import copy, json, math, sys, unittest
from pathlib import Path
sys.dont_write_bytecode=True
ROOT=Path(__file__).resolve().parents[1];sys.path.insert(0,str(ROOT/'tools'))
import schema_reflex_reference as r

class SchemaReferenceTests(unittest.TestCase):
    def setUp(self):
        self.f=json.loads((ROOT/'fixtures/schema_reflex/records.json').read_text())
    def compile(self):
        return r.compile_plan(self.f['tools'],self.f['snapshot'],self.f['source_text'],self.f['span_catalogs'])
    def decode(self):
        return r.decode(self.f['plan'],self.f['response'],snapshot=self.f['snapshot'],source=self.f['source_text'],registry=self.f['registry'])
    def set_answer(self,qid,value=None,none=False):
        self.f['response']['answers'][qid]=r.answer_for(self.f['plan'],qid,value,none=none)
    def test_reproducible_catalog_and_expected_proposal(self):
        self.assertEqual(self.compile(),self.f['plan']);self.assertEqual(self.decode(),self.f['expected'])
        result=self.decode();self.assertFalse(result['executed']);self.assertFalse(result['execution_authorized']);self.assertFalse(result['verified_complete']);self.assertIsNone(result['joint_correctness'])
    def test_false_boolean_is_actual_answer_not_missing(self):
        self.assertIs(self.decode()['args']['verify'],False)
    def test_optional_false_omits_without_default(self):
        self.assertNotIn('limit',self.decode()['args'])
    def test_optional_true_requires_active_value(self):
        self.set_answer('inspect.limit.present',True)
        with self.assertRaises(r.Refusal):self.decode()
        self.set_answer('inspect.limit',2);self.assertIs(type(self.decode()['args']['limit']),int)
    def test_no_tool_fits_is_not_an_action(self):
        self.set_answer('route',none=True);self.assertEqual(self.decode()['status'],'NoSuitableCandidate')
    def test_missing_span_is_explicit_absence(self):
        self.set_answer('inspect.target',none=True);result=self.decode();self.assertEqual(result['status'],'NoSuitableCandidate');self.assertFalse(result['executed'])
    def test_inactive_bad_answers_do_not_execute(self):
        self.set_answer('route','status');self.f['response']['answers']['inspect.verify']={}
        result=self.decode();self.assertEqual(result['args'],{});self.assertIn('inspect.verify',result['unused_question_ids'])
    def test_unknown_question_refuses(self):
        self.f['response']['answers']['invented']={}
        with self.assertRaises(r.Refusal):self.decode()
    def test_complete_invocation_cross_field_failure(self):
        self.set_answer('inspect.mode','scan')
        with self.assertRaisesRegex(r.Refusal,'complete invocation'):self.decode()
    def test_unicode_codepoint_projection_is_exact(self):
        result=self.decode();self.assertEqual(result['args']['target'],'src/α.ts')
        projection=result['provenance']['target'];self.assertEqual(projection['offset_unit'],'unicode_codepoint')
        self.assertEqual(self.f['source_text'][projection['start']:projection['end']],'src/α.ts')
    def test_untrusted_readonly_hint_never_grants_permission(self):
        self.assertTrue(self.f['tools'][0]['annotations']['readOnlyHint']);self.assertFalse(self.decode()['execution_authorized'])
    def test_changed_descriptor_even_same_schema_refuses(self):
        self.f['registry']['inspect']['descriptor_digest']='new-description'
        with self.assertRaises(r.Refusal):self.decode()
    def test_const_is_bound_to_declared_type(self):
        self.f['tools'][0]['schema']['properties']['mode']={'type':'string','const':'inspect'}
        plan=self.compile();self.assertEqual(plan['questions']['inspect.mode']['candidates'][0]['value'],'inspect')
    def test_no_span_catalog_exposes_only_absence(self):
        self.f['span_catalogs']={};plan=self.compile();c=plan['questions']['inspect.target']['candidates'];self.assertEqual(len(c),1);self.assertEqual(c[0]['kind'],'none')
    def test_duplicate_keys_and_unsafe_json(self):
        for text in ['{"x":1,"x":2}','{"__proto__":{}}','{"x":NaN}']:
            with self.subTest(text=text),self.assertRaises(r.Refusal):r.parse_json(text)
    def test_depth_limit(self):
        with self.assertRaises(r.Refusal):r.parse_json('['*20+'0'+']'*20)
    def test_manifest_budget(self):
        self.f['tools'][0]['description']='x'*4097
        with self.assertRaises(r.Refusal):self.compile()
    def test_duplicate_tool(self):
        self.f['tools'].append(copy.deepcopy(self.f['tools'][0]))
        with self.assertRaises(r.Refusal):self.compile()
    def test_source_budget(self):
        self.f['source_text']='x'*65537
        with self.assertRaises(r.Refusal):self.compile()
    def test_span_bounds(self):
        self.f['span_catalogs']['inspect']['target']=[{'start':-1,'end':4}]
        with self.assertRaises(r.Refusal):self.compile()
    def test_integer_range_budget(self):
        self.f['tools'][0]['schema']['properties']['limit']['maximum']=1000000
        with self.assertRaises(r.Refusal):self.compile()
    def test_typed_enum_refuses_bool_as_integer(self):
        self.f['tools'][0]['schema']['properties']['limit']={'type':'integer','enum':[True,2]}
        with self.assertRaises(r.Refusal):self.compile()
    def test_invalid_constraint_cannot_install_code(self):
        self.f['registry']['inspect']['constraints']=[{'kind':'eval','code':'no execution'}]
        with self.assertRaises(r.Refusal):self.decode()

class ExtraBoundaryTests(unittest.TestCase):
    setUp = SchemaReferenceTests.setUp
    compile = SchemaReferenceTests.compile
    def test_span_unit_must_be_declared(self):
        self.f['span_catalogs']['inspect']['target'][0]['offset_unit']='utf16_codeunit'
        with self.assertRaisesRegex(r.Refusal,'source/unit'):self.compile()
    def test_span_source_version_must_match(self):
        self.f['span_catalogs']['inspect']['target'][0]['source_version']='stale'
        with self.assertRaises(r.Refusal):self.compile()
    def test_span_digest_must_match(self):
        self.f['span_catalogs']['inspect']['target'][0]['source_digest']='stale'
        with self.assertRaises(r.Refusal):self.compile()
    def test_empty_list_is_not_absent_span_map(self):
        self.f['span_catalogs']=[]
        with self.assertRaises(r.Refusal):self.compile()
    def test_unknown_span_field_refuses(self):
        self.f['span_catalogs']['inspect']['unknown']=[]
        with self.assertRaises(r.Refusal):self.compile()
    def test_extreme_integer_is_explicit_refusal(self):
        self.f['tools'][0]['schema']['properties']['limit']['maximum']=10**1000
        with self.assertRaises(r.Refusal):self.compile()
    def test_root_annotation_type_refuses(self):
        self.f['tools'][0]['schema']['description']=False
        with self.assertRaises(r.Refusal):self.compile()

for qid in ['route','inspect.mode','inspect.verify','inspect.target','inspect.limit.present']:
    def test(self,qid=qid):
        self.f['response']['answers'].pop(qid)
        with self.assertRaisesRegex(r.Refusal,'active answer'):self.decode()
    setattr(SchemaReferenceTests,'test_missing_active_'+qid.replace('.','_'),test)
for label,change in {
    'array':lambda s:s['properties'].update(extra={'type':'array','items':{'enum':['x','y']}}),
    'nested':lambda s:s['properties'].update(extra={'type':'object','properties':{}}),
    'remote_ref':lambda s:s.update({'$ref':'https://example.invalid/not-fetched'}),
    'union':lambda s:s.update(oneOf=[{},{}]),
    'default':lambda s:s['properties']['verify'].update(default=False),
    'unsafe_key':lambda s:s['properties'].update({'__proto__':{'type':'boolean'}}),
    'unknown_keyword':lambda s:s.update(unrecognized=True),
    'open_properties':lambda s:s.update(additionalProperties=True),
    'float_bounds':lambda s:s['properties']['limit'].update(minimum=1.5),
    'missing_required_name':lambda s:s['required'].append('unknown'),
    'duplicate_enum':lambda s:s['properties']['mode'].update(enum=['x','x']),
    'unbounded_number':lambda s:s['properties'].update(extra={'type':'number'}),
}.items():
    def test(self,change=change):
        change(self.f['tools'][0]['schema'])
        with self.assertRaises(r.Refusal):self.compile()
    setattr(SchemaReferenceTests,'test_unsupported_'+label,test)
for label,change in {
    'snapshot':lambda f:f.update(snapshot='changed'),
    'source':lambda f:f.update(source_text=f['source_text']+' '),
    'schema':lambda f:f['registry']['inspect'].update(schema_digest='changed'),
    'unregistered':lambda f:f['registry'].pop('inspect'),
    'plan':lambda f:f['plan']['tools']['inspect'].update(schema_digest='tampered'),
    'envelope':lambda f:f['response'].update(catalog_digest='wrong'),
    'probability_nan':lambda f:f['response']['answers']['inspect.verify']['probabilities'].update(c0000=float('nan')),
    'probability_bool':lambda f:f['response']['answers']['inspect.verify']['probabilities'].update(c0000=True),
    'distribution_missing':lambda f:f['response']['answers']['inspect.verify']['probabilities'].pop('c0001'),
    'wrong_choice':lambda f:f['response']['answers']['inspect.verify'].update(selected_id='invented'),
}.items():
    def test(self,change=change):
        change(self.f)
        with self.assertRaises(r.Refusal):self.decode()
    setattr(SchemaReferenceTests,'test_identity_or_result_'+label,test)

class LearningReferenceTests(unittest.TestCase):
    def setUp(self):
        self.f=json.loads((ROOT/'fixtures/schema_reflex/records.json').read_text())
    def rows(self,n,correct=True):
        return [{'id':f'{i:05}', 'group':f'g{i:05}', 'correct':correct,'probability':.98} for i in range(n)]
    def data(self):
        return [{'id':role,'input_digest':'input-'+role,'group':'group-'+role,'partition':role,'label_digest':'label','label_kind':'teacher','protected':role=='test'} for role in sorted(r.ROLES)]
    def test_unchanged_qualification_only_proposes(self):
        b=self.f['binding'];self.assertEqual(r.route_recommendation(1,{'status':'ready','threshold':.95},b,b,ood='qualified_in_domain',revoked=False),'ELIGIBLE_PROPOSAL')
    def test_null_cutoff_never_becomes_zero(self):
        b=self.f['binding'];self.assertEqual(r.route_recommendation(1,{'status':'ready','threshold':None},b,b,ood='qualified_in_domain',revoked=False),'FALLBACK')
    def test_ood_and_unknown_and_revocation_fallback(self):
        b=self.f['binding']
        for ood,revoked in [('ood',False),('unknown',False),('qualified_in_domain',True)]:
            self.assertEqual(r.route_recommendation(1,{'status':'ready','threshold':.9},b,b,ood=ood,revoked=revoked),'FALLBACK')
    def test_five_disjoint_roles(self):
        result=r.validate_partitions(self.data());self.assertTrue(all(x==1 for x in result['unique_rows_by_role'].values()));self.assertFalse(result['independence_proved'])
    def test_same_partition_duplicate_deduplicated(self):
        rows=self.data();duplicate=copy.deepcopy(rows[0]);duplicate['id']='duplicate';rows.append(duplicate)
        self.assertEqual(sum(r.validate_partitions(rows)['unique_rows_by_role'].values()),5)
    def test_conflicting_labels_refuse(self):
        rows=self.data();duplicate=copy.deepcopy(rows[0]);duplicate.update(id='duplicate',label_digest='different');rows.append(duplicate)
        with self.assertRaisesRegex(r.Refusal,'adjudication'):r.validate_partitions(rows)
    def test_teacher_key_does_not_cross_partition(self):
        job=self.f['teacher_job'];a=r.teacher_key(job,'state');changed=dict(job,partition='test');self.assertNotEqual(a,r.teacher_key(changed,'state'))
    def test_teacher_key_changes_with_all_scope_members(self):
        job=self.f['teacher_job'];a=r.teacher_key(job,'state')
        for key in r.JOB_KEYS-{'partition'}:
            with self.subTest(key=key):self.assertNotEqual(a,r.teacher_key(dict(job,**{key:job[key]+'-changed'}),'state'))
    def test_credential_cannot_be_added_to_job(self):
        job=dict(self.f['teacher_job'],api_key='never-persist-this')
        with self.assertRaises(r.Refusal):r.teacher_key(job,'state')
    def test_torn_log_tail_not_committed(self):
        row={'job_key':'job','input_digest':'state','label_digest':'label'}
        self.assertEqual(r.committed_labels(json.dumps(row)+'\n{"partial":',job_key='job'),{'state':'label'})
        self.assertEqual(r.committed_labels(json.dumps(row),job_key='job'),{})
    def test_duplicate_or_wrong_job_log_refuses(self):
        row=json.dumps({'job_key':'job','input_digest':'state','label_digest':'label'})+'\n'
        for log,key in [(row+row,'job'),(row,'different')]:
            with self.assertRaises(r.Refusal):r.committed_labels(log,job_key=key)
    def test_binomial_all_successes_closed_form(self):
        self.assertAlmostEqual(r.lower_bound(120,120,.05/13),(.05/13)**(1/120),places=12)
    def test_binomial_nontrivial_analytic_case(self):
        # P[Binomial(2,p)>=1] = 2p-p^2; lower endpoint solves tail=alpha.
        self.assertAlmostEqual(r.lower_bound(1,2,.05),1-math.sqrt(.95),places=12)
    def test_minimum_support_is_not_a_guarantee(self):
        result=r.recommend(self.rows(30));self.assertIsNone(result['threshold']);self.assertEqual(result['status'],'target_not_met')
    def test_small_dataset_reports_insufficient(self):
        result=r.recommend(self.rows(29));self.assertIsNone(result['threshold']);self.assertEqual(result['status'],'insufficient_data')
    def test_high_support_selects_highest_coverage_lowest_tie(self):
        result=r.recommend(self.rows(120));self.assertEqual(result['status'],'ready');self.assertEqual(result['threshold'],0)
    def test_duplicated_groups_do_not_fake_independent_support(self):
        rows=self.rows(300)
        for row in rows:row['group']='one-episode'
        result=r.recommend(rows);self.assertEqual(result['representatives'],1);self.assertIsNone(result['threshold'])
    def test_representative_selection_not_by_accuracy(self):
        rows=self.rows(120)
        for row in rows:row['correct']=False
        extra=[dict(row,id='z'+row['id'],correct=True,probability=1) for row in rows]
        result=r.recommend(rows+extra);self.assertEqual(result['candidates'][0]['correct'],0)
    def test_corrected_grid_budget_cannot_be_understated(self):
        with self.assertRaises(r.Refusal):r.recommend(self.rows(120),comparisons=1)
    def test_extra_comparisons_cannot_strengthen_bound(self):
        rows=self.rows(120);a=r.recommend(rows);b=r.recommend(rows,comparisons=130)
        self.assertLessEqual(b['candidates'][0]['lower_bound'],a['candidates'][0]['lower_bound'])
    def test_reference_has_no_trained_evidence(self):
        draft=json.loads((ROOT/'fixtures/schema_reflex/experiment-template.json').read_text());self.assertFalse(draft['deployment_enabled']);self.assertFalse(draft['live_evidence']);self.assertEqual(draft['product_result'],'NOT_RUN')

for key in sorted(r.BINDING_KEYS):
    def test(self,key=key):
        b=self.f['binding'];altered=dict(b,**{key:'changed'})
        with self.assertRaises(r.Refusal):r.validate_binding(b,altered)
    setattr(LearningReferenceTests,'test_binding_mismatch_'+key,test)
for name,mutate in {
    'groups':lambda rows:rows[1].update(group=rows[0]['group']),
    'inputs':lambda rows:rows[1].update(input_digest=rows[0]['input_digest']),
    'protected':lambda rows:rows[0].update(protected=True),
    'id':lambda rows:rows[1].update(id=rows[0]['id']),
    'approval_not_label':lambda rows:rows[0].update(label_kind='user_approved_tool'),
}.items():
    def test(self,mutate=mutate):
        rows=self.data();mutate(rows)
        with self.assertRaises(r.Refusal):r.validate_partitions(rows)
    setattr(LearningReferenceTests,'test_data_refuses_'+name,test)

if __name__=='__main__':unittest.main()
