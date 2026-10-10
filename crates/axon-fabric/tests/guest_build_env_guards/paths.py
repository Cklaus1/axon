# Ancestors of a build parent, the recorded ancestors, the build parent, a
# directory the build uid must traverse, and an operator file.
good = S + "/good"
mkdir(good); mkdir(good + "/child")
gw = S + "/gw"
mkdir(gw, 0o775); mkdir(gw + "/child")
u = S + "/u4242"
mkdir(u, 0o755, 4242); mkdir(u + "/child", 0o755, 4242)
os.symlink(good, S + "/link")
anc, why = g.ancestors_of(good + "/child")
chk("ancestors_of control: a chain only root can write", why, "")
chk("ancestors_of lists every directory from /", [a["path"] for a in anc][:2], ["/", "/var"])
chk("ancestors_of relative path", g.ancestors_of("rel/dir")[1], "is not an absolute path free of symlinks")
chk("ancestors_of symlinked path", g.ancestors_of(S + "/link/child")[1], "is not an absolute path free of symlinks")
chk("ancestors_of missing directory", g.ancestors_of(good + "/nope/child")[1], "No such file or directory")
chk("ancestors_of group-writable ancestor", g.ancestors_of(gw + "/child")[1], "can be written by a uid other than root or the builder")
chk("ancestors_of other-uid ancestor", g.ancestors_of(u + "/child")[1], "can be written by a uid other than root or the builder")
chk("ancestors_of the builder's own uid is allowed", g.ancestors_of(u + "/child", 4242)[1], "")
chk("ancestors_of a non-builder uid is not allowed", g.ancestors_of(u + "/child", 4243)[1], "can be written by a uid other than root or the builder")

def mod(i, **kv):
    a = [dict(x) for x in anc]
    a[i].update(kv)
    return a
last = len(anc) - 1
P = good + "/child"
chk("ancestors_problem control", g.ancestors_problem(P, anc, 0), "")
chk("ancestors_problem parent not a string", g.ancestors_problem(None, anc, 0), "it records no build parent")
chk("ancestors_problem ancestors not a list", g.ancestors_problem(P, "x", 0), "it records no build parent")
chk("ancestors_problem ancestors are not the parent's", g.ancestors_problem(P, anc[:-1], 0), "ancestors are not the parent")
chk("ancestors_problem foreign uid", g.ancestors_problem(P, mod(last, uid=4242), 0), "was writable by another uid")
chk("ancestors_problem the builder's uid is allowed", g.ancestors_problem(P, mod(last, uid=4242), 4242), "")
chk("ancestors_problem mode not a string", g.ancestors_problem(P, mod(last, mode=493), 0), "was writable by another uid")
chk("ancestors_problem mode without 0o", g.ancestors_problem(P, mod(last, mode="755"), 0), "was writable by another uid")
chk("ancestors_problem empty mode digits", g.ancestors_problem(P, mod(last, mode="0o"), 0), "was writable by another uid")
chk("ancestors_problem non-octal mode digits", g.ancestors_problem(P, mod(last, mode="0o78"), 0), "was writable by another uid")
chk("ancestors_problem group-writable mode", g.ancestors_problem(P, mod(last, mode="0o775"), 0), "was writable by another uid")
chk("ancestors_problem other-writable mode", g.ancestors_problem(P, mod(last, mode="0o757"), 0), "was writable by another uid")

# build_parent(): refused unless only root or the builder can write any ancestor.
os.environ["AXON_GUEST_BUILD_PARENT"] = good + "/bp"
r = g.build_parent()
chk("build_parent control", r[0], Eq(good + "/bp"))
os.environ["AXON_GUEST_BUILD_PARENT"] = gw + "/bp"
chk("build_parent under a group-writable directory", run(g.build_parent), "not private to the builder")

# reach_problem(path, uid): every directory from / must be traversable by uid.
closed = S + "/closed"
mkdir(closed, 0o700); mkdir(closed + "/in", 0o755)
chk("reach_problem a directory closed to other uids", g.reach_problem(closed + "/in", 4242), "cannot be traversed by uid 4242")
chk("reach_problem the owner may traverse by the owner bit", g.reach_problem(closed + "/in", 0), "")
o1 = S + "/o1"
mkdir(o1, 0o701); mkdir(o1 + "/in", 0o755)
chk("reach_problem other-execute alone is enough", g.reach_problem(o1 + "/in", 4242), "")
g1 = S + "/g1"
mkdir(g1, 0o710); mkdir(g1 + "/in", 0o755)
chk("reach_problem group-execute is not enough for another uid", g.reach_problem(g1 + "/in", 4242), "cannot be traversed by uid 4242")
own = S + "/own"
mkdir(own, 0o700, 4242); mkdir(own + "/in", 0o755, 4242)
chk("reach_problem a directory only its owner may traverse, uid is the owner", g.reach_problem(own + "/in", 4242), "")
chk("reach_problem a directory only its owner may traverse, uid is another", g.reach_problem(own + "/in", 4243), "cannot be traversed by uid 4243")

# operator_file_problem(path): real file, real directories, root-owned, closed to group/other writes.
od = S + "/op"
mkdir(od)
put(od + "/pin", "{}")
os.symlink(od + "/pin", od + "/pinlink")
mkdir(od + "/adir")
put(od + "/gw", "x", 0o664)
put(od + "/ow", "x", 0o646)
put(od + "/u", "x", 0o644, 4242)
mkdir(od + "/wd", 0o775); put(od + "/wd/f", "x")
mkdir(od + "/ud", 0o755, 4242); put(od + "/ud/f", "x")
os.symlink(od, S + "/oplink")
put(od + "/plainfile", "x")
chk("operator_file_problem control", g.operator_file_problem(od + "/pin"), "")
chk("operator_file_problem missing file", g.operator_file_problem(od + "/absent"), "No such file or directory")
chk("operator_file_problem a symlink as the file", g.operator_file_problem(od + "/pinlink"), "is not a real file")
chk("operator_file_problem a directory as the file", g.operator_file_problem(od + "/adir"), "is not a real file")
chk("operator_file_problem a symlinked ancestor", g.operator_file_problem(S + "/oplink/pin"), "is not a real directory")
chk("operator_file_problem a file as an ancestor", g.operator_file_problem(od + "/plainfile/x"), "is not a real directory")
chk("operator_file_problem group-writable file", g.operator_file_problem(od + "/gw"), "is not root-owned and closed to group/other writes")
chk("operator_file_problem other-writable file", g.operator_file_problem(od + "/ow"), "is not root-owned and closed to group/other writes")
chk("operator_file_problem file owned by another uid", g.operator_file_problem(od + "/u"), "is not root-owned and closed to group/other writes")
chk("operator_file_problem group-writable directory above", g.operator_file_problem(od + "/wd/f"), "is not root-owned and closed to group/other writes")
chk("operator_file_problem directory above owned by another uid", g.operator_file_problem(od + "/ud/f"), "is not root-owned and closed to group/other writes")
