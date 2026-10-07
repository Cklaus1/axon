chk("require_runner lets root run", run(g.require_runner), "")
os.environ.pop("AXON_GUEST_BUILD_UID", None)
chk("as_build_uid runs the build as the unprivileged uid, with no new privileges and no groups", g.as_build_uid(["x", "y"]),
    ["/usr/bin/unshare", "--pid", "--fork", "--mount-proc", "--kill-child", "--",
     "/usr/bin/setpriv", "--reuid=65534", "--regid=65534", "--clear-groups", "--no-new-privs", "--", "x", "y"])
g.UNSHARE = "/nonexistent/unshare"
chk("as_build_uid refuses to start a step when the PID-namespace tool is missing", run(g.as_build_uid, ["x"]), "is not available")
