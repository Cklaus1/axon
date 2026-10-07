# Amendment 101: service-account discovery fails CLOSED. Each case plants ONE shape of a service's identity in a
# fixture /etc/axon or unit directory and requires that uid (or gid) refused as the build uid; the controls
# are the shapes that name no service and are accepted. Fixtures only: g.SERVICE_ETC / SERVICE_UNITS / SERVICE_USERS
# point at scratch directories, so the host's own deployment is never read.
import shutil
ETC, UNITS = os.path.join(S, "svc-etc"), os.path.join(S, "svc-units")
g.SERVICE_ETC, g.SERVICE_UNITS, g.SERVICE_USERS = ETC, UNITS, ()
def reset():
    for d in (ETC, UNITS):
        shutil.rmtree(d, ignore_errors=True)
        os.makedirs(d)
def bu(uid):
    return run(g.build_uid_problem, uid, 1, ())
def plant_etc(rel, text):
    put(os.path.join(ETC, rel), text)
def plant_unit(rel, text):
    put(os.path.join(UNITS, rel), text)
def case(planter, rel, text):
    reset()
    planter(rel, text)
SVC = "service uid"
reset()
chk("service ids control: no deployment, a dedicated uid is accepted", bu(4300), "")
case(plant_etc, "a.json", '{"allowed_uids": [4301, 4302]}')
chk("service ids a plural uid list (the first)", bu(4301), SVC)
chk("service ids a plural uid list (the last)", bu(4302), SVC)
case(plant_etc, "a.json", '{"uid": "4303"}')
chk("service ids a uid given as a decimal string", bu(4303), SVC)
case(plant_etc, "a.json", '{"fabric_uids": ["4304"]}')
chk("service ids a plural list of decimal strings", bu(4304), SVC)
case(plant_etc, "a.json", '{"UID": 4305}')
chk("service ids the key in another letter case", bu(4305), SVC)
case(plant_etc, "sub/deeper/a.json", '{"fabric_uid": 4306}')
chk("service ids a config in a subdirectory of /etc/axon", bu(4306), SVC)
case(plant_etc, "a.conf", '{"uid": 4307}')
chk("service ids a config whose name does not end in .json", bu(4307), SVC)
case(plant_etc, "a.json", '{"x": {"y": [{"observer_uid": 4308}]}}')
chk("service ids a uid nested in lists and objects", bu(4308), SVC)
case(plant_unit, "axon-a.service", '[Service]\nUser="4309"\n')
chk("service ids a double-quoted User=", bu(4309), SVC)
case(plant_unit, "axon-a.service", "[Service]\nUser='4310'\n")
chk("service ids a single-quoted User=", bu(4310), SVC)
case(plant_unit, "axon-a.service.d/override.conf", '[Service]\nUser=4311\n')
chk("service ids a drop-in of an axon unit", bu(4311), SVC)
case(plant_unit, "custody.service", '[Service]\nExecStart=/usr/local/libexec/axon/axon-custodian\nUser=4312\n')
chk("service ids a unit not named axon-* that runs an axon binary", bu(4312), SVC)
case(plant_unit, "other.service", '[Service]\nExecStart=/usr/bin/other\nUser=4313\n')
chk("service ids control: a unit that runs no axon binary names no service", bu(4313), "")
case(plant_unit, "axon-a.service", '[Service]\nDynamicUser=yes\n')
chk("service ids DynamicUser allocates the uid at start: cannot be determined", bu(4314), "cannot be determined")
case(plant_unit, "other.service", '[Service]\nExecStart=/usr/bin/other\nDynamicUser=yes\n')
chk("service ids control: DynamicUser of an unrelated unit", bu(4314), "")
case(plant_etc, "big.json", '{"pad": "' + "x" * 70000 + '"}')
chk("service ids a JSON config over the size bound refuses (it is not skipped)", bu(4315), "cannot be determined")
case(plant_etc, "bad.json", '{"fabric_uid": 4316')
chk("service ids an unparsable JSON config refuses (it is not skipped)", bu(4316), "cannot be determined")
case(plant_etc, "a.json", '{"uid": "alice"}')
chk("service ids a uid field that is not a number refuses", bu(4317), "cannot be determined")
reset()
os.symlink("/nonexistent-target", os.path.join(ETC, "dangling.json"))
chk("service ids a .json that cannot be opened refuses (it is not skipped)", bu(4323), "cannot be determined")
reset()
shutil.rmtree(ETC); put(ETC, "not a directory")
chk("service ids an /etc/axon that cannot be listed refuses", bu(4324), "cannot be determined")
reset()
shutil.rmtree(UNITS); put(UNITS, "not a directory")
chk("service ids a unit directory that cannot be listed refuses", bu(4325), "cannot be determined")
case(plant_etc, "a.json", '{"fabric_gid": 4318}')
chk("service ids a service gid: the build gid equals the build uid", bu(4318), "service gid")
case(plant_unit, "axon-a.service", '[Service]\nGroup=4319\n')
chk("service ids a Group= of an axon unit is a service gid", bu(4319), "service gid")
case(plant_etc, "a.json", '{"count": 4320, "name": "x", "uid_note": "y"}')
chk("service ids control: fields that are not identities name no service", bu(4320), "")
case(plant_etc, "keys/a.key", "not json at all")
chk("service ids control: a file that is not a config (a key) is not an error", bu(4321), "")
# a build uid that already owns running processes
reset()
import subprocess as sp
child = sp.Popen(["/usr/bin/setpriv", "--reuid=4322", "--regid=4322", "--clear-groups", "/usr/bin/sleep", "30"])
import time
for _ in range(100):
    if g.build_uid_pids(4322):
        break
    time.sleep(0.05)
chk("foreign processes: a build uid that already owns processes is refused", g.foreign_process_problem(4322), "already owns running processes")
g.LOCK_DIR = os.path.join(S, "svc-locks")
os.environ["AXON_GUEST_BUILD_UID"] = "4322"
def lock_refused():
    r = run(g.build_uid_lock)
    if isinstance(r, int):
        os.close(r); return "locked"
    return r
chk("foreign processes: the per-uid lock refuses a build uid that already owns processes", lock_refused(), "already owns running processes")
child.kill(); child.wait()
for _ in range(100):
    if not g.build_uid_pids(4322):
        break
    time.sleep(0.05)
chk("foreign processes: control, none left", g.foreign_process_problem(4322), "")
chk("foreign processes: control, the per-uid lock is taken once the uid owns none", lock_refused(), "locked")
os.environ.pop("AXON_GUEST_BUILD_UID", None)
