# The environment the build constructs: every VALUE is a decision (what cargo and make may see).
base = S + "/b"
os.environ["PATH"] = "/opt/evil/bin:/usr/bin:/bin"
os.environ["HOME"] = "/home/evil"
os.environ["LC_ALL"] = "en_US.UTF-8"
env = g.constructed_env(base, "/tc/cargo", "/tc/rustc", {})
chk("constructed_env holds exactly the allow-list's variables", sorted(env), sorted(g.ENV_ALLOWLIST))
chk("constructed_env CARGO_HOME is fresh under the base", env["CARGO_HOME"], Eq(base + "/cargo-home"))
chk("constructed_env CARGO_TARGET_DIR is fresh under the base", env["CARGO_TARGET_DIR"], Eq(base + "/target"))
chk("constructed_env GIT_CEILING_DIRECTORIES stops an enclosing repository", env["GIT_CEILING_DIRECTORIES"], Eq(base))
chk("constructed_env HOME is the base", env["HOME"], Eq(base))
chk("constructed_env LC_ALL is C", env["LC_ALL"], Eq("C"))
chk("constructed_env PATH is the fixed system directories", env["PATH"], Eq("/usr/bin:/bin"))
chk("constructed_env RUSTC is the pinned compiler", env["RUSTC"], Eq("/tc/rustc"))
chk("constructed_env passes the proxy variables on", g.constructed_env(base, "c", "r", {"https_proxy": "http://p:1"})["https_proxy"], Eq("http://p:1"))
chk("constructed_env takes nothing from the caller's environment", "ZZ_CALLER_VAR" in g.constructed_env(base, "c", "r", {}), False)
chk("kernel_env takes nothing from the caller's environment", [g.kernel_env(base)[k] for k in ("HOME", "LC_ALL", "PATH")], [base, "C", "/usr/bin:/bin"])
chk("kernel_env is exactly the make environment", g.kernel_env(base),
    {"HOME": base, "LC_ALL": "C", "PATH": "/usr/bin:/bin", "KBUILD_BUILD_TIMESTAMP": "1970-01-01",
     "KBUILD_BUILD_USER": "axon", "KBUILD_BUILD_HOST": "b263", "KBUILD_BUILD_VERSION": "1"})
chk("TOOL_PATH is the fixed system directories", g.TOOL_PATH, Eq("/usr/bin:/bin"))
decoy = S + "/decoy"
put(decoy + "/axon-decoy-tool", "#!/bin/sh\n", 0o755)
os.environ["PATH"] = decoy + ":/usr/bin:/bin"
chk("host_tool never finds a tool outside the fixed directories", g.host_tool("axon-decoy-tool"), None)
sh = g.host_tool("sh")
chk("host_tool finds a system tool by name", sh in ("/usr/bin/sh", "/bin/sh"), True)
chk("host_tool_path is the first fixed directory", g.host_tool_path("make"), Eq("/usr/bin/make"))
chk("host_tools skips a tool that is not there", g.host_tools(["axon-decoy-tool", "axon-nonesuch"]), {})
ht = g.host_tools(["sh"])["sh"]
chk("host_tools records the path, the realpath and the digest", [ht["path"], ht["realpath"] == os.path.realpath(sh), ht["sha256"] == g.sha256(sh)], [sh, True, True])
chk("host_tools records a version string", bool(ht["version"]), True)
chk("host_tools resolves cc1 through gcc, or leaves it out", ("cc1" in g.host_tools(["cc1"])) == bool(g.host_tool("gcc")), True)
