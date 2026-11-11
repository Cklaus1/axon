# Run as a claimed uid (GBE_UID, never a literal: amendment 111), so that "root" and "the builder's own uid" are different numbers
# (as root the two terms of the guard coincide, and removing one is unobservable).
ME = int(os.environ["GBE_UID"]); OTHER = ME + 1
assert os.geteuid() == ME, os.geteuid()
# Service-account discovery points at an absent directory, never the host's deployment: these cases are about the uid
# VALUE, and a host's root-only /etc/axon file (the NH-R1 opt-in is mode 0600 by design) is unreadable to this claimed
# uid, so discovery would fail closed here. Discovery itself is covered by service_ids.py's fixtures.
import tempfile
_NONE = os.path.join(tempfile.mkdtemp(), "absent")
g.SERVICE_ETC, g.SERVICE_UNITS, g.SERVICE_USERS = _NONE, _NONE, ()
g.SERVICE_UNIT_EXTRA_DIRS, g.SERVICE_UNIT_VENDOR_DIRS = (_NONE,), (_NONE,)
def ids(v):
    if v is None:
        os.environ.pop("AXON_GUEST_BUILD_UID", None)
    else:
        os.environ["AXON_GUEST_BUILD_UID"] = v
    return run(g.build_ids)
WHY = "is not an unprivileged uid other than the builder's own"
chk("build_ids default is 65534", ids(None), [65534, 65534])
chk("build_ids control another uid", ids(str(OTHER)), [OTHER, OTHER])
chk("build_ids an empty value falls to the default", ids(""), [65534, 65534])
chk("build_ids nine digits are allowed", ids("123456789"), [123456789, 123456789])
chk("build_ids not a decimal", ids("4abc"), WHY)
chk("build_ids a sign is not a decimal", ids("+" + str(OTHER)), WHY)
chk("build_ids too many digits", ids("1234567890"), WHY)
chk("build_ids root", ids("0"), WHY)
chk("build_ids the builder's own uid", ids(str(ME)), WHY)
chk("require_runner refuses a runner that is not root", run(g.require_runner), "the controlled build runs as root")
