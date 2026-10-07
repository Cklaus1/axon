# Run as uid 4242, so that "root" and "the builder's own uid" are different numbers
# (as root the two terms of the guard coincide, and removing one is unobservable).
assert os.geteuid() == 4242, os.geteuid()
def ids(v):
    if v is None:
        os.environ.pop("AXON_GUEST_BUILD_UID", None)
    else:
        os.environ["AXON_GUEST_BUILD_UID"] = v
    return run(g.build_ids)
WHY = "is not an unprivileged uid other than the builder's own"
chk("build_ids default is 65534", ids(None), [65534, 65534])
chk("build_ids control 4243", ids("4243"), [4243, 4243])
chk("build_ids an empty value falls to the default", ids(""), [65534, 65534])
chk("build_ids nine digits are allowed", ids("123456789"), [123456789, 123456789])
chk("build_ids not a decimal", ids("4abc"), WHY)
chk("build_ids a sign is not a decimal", ids("+4243"), WHY)
chk("build_ids too many digits", ids("1234567890"), WHY)
chk("build_ids root", ids("0"), WHY)
chk("build_ids the builder's own uid", ids("4242"), WHY)
chk("require_runner refuses a runner that is not root", run(g.require_runner), "the controlled build runs as root")
