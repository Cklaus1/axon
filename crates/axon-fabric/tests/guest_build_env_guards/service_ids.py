# Amendment 101: service-account discovery fails CLOSED. Each case plants ONE shape of a service's identity in a
# fixture /etc/axon or unit directory and requires that uid (or gid) refused as the build uid; the controls
# are the shapes that name no service and are accepted. Fixtures only: g.SERVICE_ETC / SERVICE_UNITS / SERVICE_USERS
# point at scratch directories, so the host's own deployment is never read.
import shutil
ETC, UNITS = os.path.join(S, "svc-etc"), os.path.join(S, "svc-units")
EXTRA, VENDOR = os.path.join(S, "svc-extra"), os.path.join(S, "svc-vendor")
g.SERVICE_ETC, g.SERVICE_UNITS, g.SERVICE_USERS = ETC, UNITS, ()
g.SERVICE_UNIT_EXTRA_DIRS, g.SERVICE_UNIT_VENDOR_DIRS = (EXTRA,), (VENDOR,)
# two accounts that exist only here, so an account NAME in a config resolves without touching the host's user database
import types
def _getpwnam(n):
    if n == "svc-acct":
        return types.SimpleNamespace(pw_uid=4401, pw_gid=4402)
    raise KeyError(n)
def _getgrnam(n):
    if n == "svc-grp":
        return types.SimpleNamespace(gr_gid=4403)
    raise KeyError(n)
_pwd, _grp = g.pwd, g.grp
g.pwd, g.grp = types.SimpleNamespace(getpwnam=_getpwnam, getpwuid=_pwd.getpwuid), types.SimpleNamespace(getgrnam=_getgrnam)
def reset():
    for d in (ETC, UNITS, EXTRA, VENDOR):
        if os.path.isdir(d) and not os.path.islink(d):
            shutil.rmtree(d)
        elif os.path.lexists(d):
            os.unlink(d)
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
chk("service ids a unit that is not named axon-* and runs no axon binary still names an account (any unit)", bu(4313), SVC)
case(plant_unit, "axon-a.service", '[Service]\nDynamicUser=yes\n')
chk("service ids DynamicUser allocates the uid at start: cannot be determined", bu(4314), "cannot be determined")
case(plant_unit, "other.service", '[Service]\nExecStart=/usr/bin/other\nDynamicUser=yes\n')
chk("service ids DynamicUser of ANY unit cannot be determined", bu(4314), "cannot be determined")
case(plant_etc, "big.json", '{"fabric_uid": 4315}' + " " * 70000)   # complete within the bound: only the size refusal stops a partial read
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

# ── amendment 105: the shapes outside the shipped deployment (round-11 FIELD-ORIGIN) ──────────────────────────────
case(plant_etc, "a.toml", 'name = "x"\nuid = 4410\n')
chk("service ids a TOML config", bu(4410), SVC)
case(plant_etc, "a.yaml", 'name: x\nfabric_uid: 4411\n')
chk("service ids a YAML config", bu(4411), SVC)
case(plant_etc, "service.env", 'FOO=1\nAXON_FABRIC_UID=4412\n')
chk("service ids an env file", bu(4412), SVC)
case(plant_etc, "a.yaml", 'allowed_uids:\n  - 4413\n  - 4414\n')
chk("service ids a YAML list item (the first)", bu(4413), SVC)
chk("service ids a YAML list item (the last)", bu(4414), SVC)
case(plant_etc, "a.toml", 'gid = 4415\n')
chk("service ids a gid in a TOML config", bu(4415), "service gid")
case(plant_etc, "a.toml", '[service]\nrun_as = "svc-acct"\n')
chk("service ids a TOML account NAME resolves to its uid", bu(4401), SVC)
case(plant_etc, "a.json", '{"user": "svc-acct"}')
chk("service ids a JSON key `user` holding an account name", bu(4401), SVC)
case(plant_etc, "a.json", '{"owner": 4416}')
chk("service ids a JSON key `owner`", bu(4416), SVC)
case(plant_etc, "a.json", '{"run_as": "4417"}')
chk("service ids a JSON key `run_as` holding a decimal string", bu(4417), SVC)
case(plant_etc, "a.json", '{"euid": 4418}')
chk("service ids a JSON key `euid`", bu(4418), SVC)
case(plant_etc, "a.json", '{"id": 4419}')
chk("service ids a JSON key `id`", bu(4419), SVC)
case(plant_etc, "a.json", '{"principal": "4420"}')
chk("service ids a JSON key `principal`", bu(4420), SVC)
case(plant_etc, "a.json", '{"group": "svc-grp"}')
chk("service ids a JSON key `group` holding a group NAME is a service gid", bu(4403), "service gid")
case(plant_etc, "a.json", '{"supplementary_groups": [4421]}')
chk("service ids a JSON `supplementary_groups` list is service gids", bu(4421), "service gid")
case(plant_etc, "a.json", '{"user": "no-such-account", "owner": {"name": "x"}}')
chk("service ids control: an account name that does not exist yet names no uid", bu(4422), "")
case(plant_etc, "a.yaml", 'uid: alice\n')
chk("service ids a strict uid key in a text config that is not a number refuses", bu(4423), "which is not a uid or gid")
case(plant_etc, "notes.txt", 'the uid of the service is set elsewhere\n')
chk("service ids a text file that mentions uid and yields no identity refuses (fail closed)", bu(4424), "in a form that cannot be read")
case(plant_etc, "notes.txt", 'nothing about identities here\n')
chk("service ids control: a text file with no identity word is not an error", bu(4424), "")
reset()
put(os.path.join(S, "elsewhere", "b.json"), '{"fabric_uid": 4425}')
os.symlink(os.path.join(S, "elsewhere"), os.path.join(ETC, "linkdir"))
chk("service ids a symlinked sub-directory of /etc/axon is followed", bu(4425), SVC)
reset()
os.makedirs(os.path.join(ETC, "loop")); os.symlink(ETC, os.path.join(ETC, "loop", "back"))
chk("service ids control: a symlink loop is read once and does not hang", bu(4426), "")
reset()
os.mkfifo(os.path.join(ETC, "pipe.conf"))
import signal
class _Blocked(Exception):                 # not an OSError: the reader under test catches those and would turn it into a refusal
    pass
def _blocked(*a):
    raise _Blocked("blocked reading a FIFO under /etc/axon")
signal.signal(signal.SIGALRM, _blocked); signal.alarm(10)        # a guard that is gone BLOCKS here; that is the case failing, not a hang
chk("service ids a FIFO under /etc/axon refuses (it is not read, it would block)", bu(4427), "is not a regular file")
signal.alarm(0)
case(plant_unit, "axon-a.service", '[Service]\nSupplementaryGroups=4428 -svc-grp 4429\n')
chk("service ids SupplementaryGroups= (a number)", bu(4428), "service gid")
chk("service ids SupplementaryGroups= (the last)", bu(4429), "service gid")
chk("service ids SupplementaryGroups= (a name, with the ignore-missing dash)", bu(4403), "service gid")
case(plant_unit, "axon-a.service", '[Service]\nUser=%U\n')
chk("service ids User= with a specifier refuses (the account cannot be determined)", bu(4430), "specifier or variable")
case(plant_unit, "axon-a.service", '[Service]\nUser=$SVC\n')
chk("service ids User= with a variable refuses", bu(4430), "specifier or variable")
case(plant_unit, "axon-a.service", '[Service]\nUser=we ird\n')
chk("service ids User= that is not a name or an id refuses", bu(4430), "is not an account name or id")
case(plant_unit, "axon-a.socket", '[Socket]\nSocketUser=4431\nSocketGroup=4432\n')
chk("service ids a .socket unit's SocketUser=", bu(4431), SVC)
chk("service ids a .socket unit's SocketGroup=", bu(4432), "service gid")
case(plant_unit, "axon-a.service", '[Service]\nUser=\\\n4433\n')
chk("service ids a line continuation in a unit", bu(4433), SVC)
case(plant_unit, "axon-a.service", '[Service]\nUser=4434 # the service\n')
chk("service ids an inline comment after User=", bu(4434), SVC)
case(plant_unit, "axon-a.service", '[Service]\nUSER=4435\n')
chk("service ids the key in another letter case", bu(4435), SVC)
case(plant_unit, "ghost.service.d/o.conf", '[Service]\nUser=4436\n')
chk("service ids a drop-in of a unit that has no file here", bu(4436), SVC)
case(plant_unit, "service.d/o.conf", '[Service]\nUser=4437\n')
chk("service ids the drop-in every service reads (service.d)", bu(4437), SVC)
reset()
put(os.path.join(EXTRA, "mine.service"), '[Service]\nExecStart=/usr/bin/other\nUser=4438\n')
chk("service ids a unit in /run/systemd/system or /usr/local/lib/systemd/system", bu(4438), SVC)
reset()
put(os.path.join(VENDOR, "axon-v.service"), '[Service]\nUser=4439\n')
chk("service ids an axon unit in a distribution unit directory", bu(4439), SVC)
reset()
put(os.path.join(VENDOR, "capsule@.service"), '[Service]\nUser=c-%i\nExecStart=/usr/bin/capsule\n')
put(os.path.join(VENDOR, "other.service"), '[Service]\nUser=4440\nExecStart=/usr/bin/other\n')
chk("service ids control: a distribution unit that is not axon's (a specifier, an id) is not read", bu(4440), "")
reset()
put(os.path.join(VENDOR, "custody.service"), '[Service]\nExecStart=/usr/local/libexec/axon/axon-custodian\nUser=4441\n')
chk("service ids a distribution-directory unit not named axon-* that runs an axon binary", bu(4441), SVC)
g.pwd, g.grp = _pwd, _grp
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
