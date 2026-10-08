import sys,subprocess
sys.path.insert(0,'scripts')
import v022_resurvey as rs
rc=rs.load("v022_refusal_coverage")
cmd=["cargo","test","--no-fail-fast","-p","axon-fabric","--test","privileged_launcher","--","--skip","a_callers_scheduling_state_never_reaches_the_root_launch","the_root_helper_hands"]
f,state,code=rs.run_tests(cmd)
print(f,state,code)
r=subprocess.run(cmd,capture_output=True,text=True)
print((r.stdout+r.stderr)[-1500:])
