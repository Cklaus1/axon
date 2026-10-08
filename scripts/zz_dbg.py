import sys,subprocess,os
sys.path.insert(0,'scripts')
import v022_resurvey as rs
rc=rs.load("v022_refusal_coverage")
for e in rc.VALUE_EXEMPT:
    if e[1]=='run_checks' and e[3]=='"--check-registry"': print(e[5])
f='crates/axon-cortex/src/runner.rs'
t=open(f).read()
assert t.count('"--check-registry"')==1
open(f,'w').write(t.replace('"--check-registry"','"--check-registryx"'))
cmd=["cargo","test","--no-fail-fast","-p","axon-fabric","--test","cortex_via_fabric"]
r=subprocess.run(cmd,capture_output=True,text=True)
open(f,'w').write(t)
print((r.stdout+r.stderr)[-2500:])
