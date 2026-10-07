# The pinned channel, the rustup that resolves it, and the toolchain it resolves to.
# g.ROOT is the scratch tree (this copy of the script lives in <ROOT>/scripts).
assert g.ROOT == S, (g.ROOT, S)
TOML = S + "/rust-toolchain.toml"
chk("pinned_channel no rust-toolchain.toml", run(g.pinned_channel), "names no channel")
put(TOML, '[toolchain]\ncomponents = ["rustfmt"]\n')
chk("pinned_channel a rust-toolchain.toml without a channel", run(g.pinned_channel), "names no channel")
put(TOML, '[toolchain]\nchannel = "nightly-2099-01-01"\n')
chk("pinned_channel control", g.pinned_channel(), Eq("nightly-2099-01-01"))
chk("pinned_channel_or_none control", g.pinned_channel_or_none(), "nightly-2099-01-01")
put(TOML, '[toolchain]\nchannel "nightly-2099-01-01"\n')
chk("pinned_channel a channel line without an equals sign", run(g.pinned_channel), "names no channel")
put(TOML, '[toolchain]\nchannel = "nightly-2099-01-01"\n')

home = S + "/home"
mkdir(home)
g.builder_home = lambda: home
chk("rustup none in the builder's home", run(g.rustup), "no rustup at")
put(home + "/.cargo/bin/rustup", "#!/bin/sh\n", 0o755)
chk("rustup control", g.rustup(), [home + "/.cargo/bin/rustup", home])

# toolchain(): `rustup which --toolchain CHAN TOOL` under a cleared environment.
tcd = S + "/tc/bin"
mkdir(tcd)
for t in ("cargo", "rustc"):
    put(tcd + "/" + t, "#!/bin/sh\n", 0o755)
def stub(body):
    p = S + "/stub-rustup"
    put(p, "#!/bin/sh\n" + body + "\n", 0o755)
    g.rustup = lambda: (p, home)
stub('echo "%s/$4"' % tcd)
chk("toolchain control", g.toolchain(), ["nightly-2099-01-01", tcd + "/cargo", tcd + "/rustc"])
stub('echo "%s/$4"; exit 1' % tcd)
chk("toolchain rustup fails but prints a path", run(g.toolchain), "rustup cannot resolve")
stub('echo "tc/$4"')
chk("toolchain rustup prints a relative path", run(g.toolchain), "rustup cannot resolve")
stub('echo "%s/missing-$4"' % tcd)
chk("toolchain rustup names a file that is not there", run(g.toolchain), "rustup cannot resolve")
stub('echo "%s"' % (S + "/tc"))
chk("toolchain rustup names a directory, not a file", run(g.toolchain), "rustup cannot resolve")
