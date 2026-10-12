# The operator's builder pin, the per-build proof key and the proof over a record.
NOT_PIN = "naming a uid, an absolute parent and an unprivileged build_uid"
pd = S + "/pins"
mkdir(pd)
def pin(name, body):
    p = pd + "/" + name
    put(p, body if isinstance(body, str) else json.dumps(body))
    return p
pgood = {"schema": g.BUILDER_PIN_SCHEMA, "uid": 0, "parent": "/var/lib/x", "build_uid": 65534}
def variant(**kv):
    d = dict(pgood)
    d.update(kv)
    return d
chk("builder_pin control", g.builder_pin(pin("ok", pgood)), [[0, "/var/lib/x", 65534], ""])
os.symlink(pd + "/ok", pd + "/lnk")
chk("builder_pin a pin that is not an operator file", g.builder_pin(pd + "/lnk")[1], "is not the operator's")
chk("builder_pin an unreadable pin", g.builder_pin(pin("junk", "{not json"))[1], "is unreadable")
chk("builder_pin not an object", g.builder_pin(pin("list", []))[1], NOT_PIN)
chk("builder_pin another schema", g.builder_pin(pin("sch", variant(schema="x")))[1], NOT_PIN)
chk("builder_pin uid is a string", g.builder_pin(pin("u1", variant(uid="0")))[1], NOT_PIN)
chk("builder_pin uid is a bool", g.builder_pin(pin("u2", variant(uid=True)))[1], NOT_PIN)
chk("builder_pin uid is negative", g.builder_pin(pin("u3", variant(uid=-1)))[1], NOT_PIN)
chk("builder_pin build_uid is a string", g.builder_pin(pin("b1", variant(build_uid="1")))[1], NOT_PIN)
chk("builder_pin build_uid is a bool", g.builder_pin(pin("b2", variant(build_uid=True)))[1], NOT_PIN)
chk("builder_pin build_uid is negative", g.builder_pin(pin("b3", variant(build_uid=-5)))[1], NOT_PIN)
chk("builder_pin build_uid is root", g.builder_pin(pin("b4", variant(build_uid=0, uid=7)))[1], NOT_PIN)
chk("builder_pin build_uid is the builder's own", g.builder_pin(pin("b5", variant(build_uid=7, uid=7)))[1], NOT_PIN)
chk("builder_pin parent is not a string", g.builder_pin(pin("p1", variant(parent=5)))[1], NOT_PIN)
chk("builder_pin parent is relative", g.builder_pin(pin("p2", variant(parent="a/b")))[1], NOT_PIN)
chk("builder_pin parent is not normalised", g.builder_pin(pin("p3", variant(parent="/a/../b")))[1], NOT_PIN)
chk("builder_pin parent has a trailing slash", g.builder_pin(pin("p4", variant(parent="/a/b/")))[1], NOT_PIN)

# proof_key(parent, pid, builder_uid, judging): the key `new_proof` stored.
bp = S + "/bp"
mkdir(bp, 0o711)
PID = "axon-guest-build-abc"
def keyed(pid=PID, keydir_mode=0o700, key_mode=0o400, key=None, keydir_uid=0, key_uid=0, parent=bp):
    mkdir(parent + "/keys", keydir_mode, keydir_uid)
    put(parent + "/keys/" + pid + ".key", key if key is not None else "k" * 64, key_mode, key_uid)
keyed()
chk("proof_key control", g.proof_key(bp, PID, 0)[1], "")
chk("proof_key control returns the key", g.proof_key(bp, PID, 0)[0], b"k" * 64)
chk("proof_key the parent is not a string", g.proof_key(None, PID, 0)[1], "names no usable proof id or build parent")
chk("proof_key the id is not a string", g.proof_key(bp, 5, 0)[1], "names no usable proof id or build parent")
chk("proof_key the id is not a proof id", g.proof_key(bp, "../x", 0)[1], "names no usable proof id or build parent")
gwp = S + "/gwp"
mkdir(gwp, 0o775)
keyed(parent=gwp + "/bp")
chk("proof_key judged under a parent another uid can write", g.proof_key(gwp + "/bp", PID, 0)[1], "the proof's build parent is not private")
chk("proof_key signing is not judged on the parent's ancestors", g.proof_key(gwp + "/bp", PID, 0, judging=False)[1], "")
chk("proof_key no key at the pinned place", g.proof_key(bp, "axon-guest-build-none", 0)[1], "the build's proof key is not at")
mkdir(S + "/nokd/bp", 0o711)
chk("proof_key no keys directory", g.proof_key(S + "/nokd/bp", PID, 0)[1], "the build's proof key is not at")
# the key's directory
keyed(parent=S + "/kd1")
os.rename(S + "/kd1/keys", S + "/kd1/real"); os.symlink(S + "/kd1/real", S + "/kd1/keys")
chk("proof_key the keys directory is a symlink", g.proof_key(S + "/kd1", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kd2", keydir_uid=4242)
chk("proof_key the keys directory is another uid's", g.proof_key(S + "/kd2", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kd3", keydir_mode=0o750)
chk("proof_key the keys directory is open to its group", g.proof_key(S + "/kd3", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kd4", keydir_mode=0o707)
chk("proof_key the keys directory is open to others", g.proof_key(S + "/kd4", PID, 0)[1], "is not a file only the builder")
# the key file
keyed(parent=S + "/kf1", key_uid=4242)
chk("proof_key the key is another uid's", g.proof_key(S + "/kf1", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kf2", key_mode=0o440)
chk("proof_key the key is group-readable", g.proof_key(S + "/kf2", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kf3", key_mode=0o404)
chk("proof_key the key is other-readable", g.proof_key(S + "/kf3", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kf4", key_mode=0o700)
chk("proof_key the key is executable", g.proof_key(S + "/kf4", PID, 0)[1], "is not a file only the builder")
keyed(parent=S + "/kf5", key="short")
chk("proof_key a key too short to be one", g.proof_key(S + "/kf5", PID, 0)[1], "holds no key")
keyed(parent=S + "/kf6")
os.remove(S + "/kf6/keys/" + PID + ".key"); mkdir(S + "/kf6/keys/" + PID + ".key", 0o400)
chk("proof_key the key is a directory", g.proof_key(S + "/kf6", PID, 0)[1], "is not a file only the builder")

# proof_problems(rec, what, builder): the pinned builder's runner wrote this record.
def signed(**kv):
    rec = {"build_uid": 65534, "x": 1, "proof": {"schema": g.PROOF_SCHEMA, "id": PID, "hmac": ""}}
    rec.update(kv)
    key = g.proof_key(bp, PID, 0)[0]
    rec["proof"]["hmac"] = hmac.new(key, g.proof_payload(rec), "sha256").hexdigest()
    return rec
rec = signed()
B3 = (0, bp, 65534)
NOPROOF = "carries no builder proof"
chk("proof_problems control, a pin of three", g.proof_problems(rec, "t", B3), "")
chk("proof_problems control, a pin of two", g.proof_problems(rec, "t", (0, bp)), "")
chk("proof_problems a record that is not an object", g.proof_problems([], "t", B3), NOPROOF)
chk("proof_problems no proof", g.proof_problems({"x": 1}, "t", B3), NOPROOF)
chk("proof_problems another proof schema", g.proof_problems(dict(rec, proof=dict(rec["proof"], schema="x")), "t", B3), NOPROOF)
chk("proof_problems an hmac that is not a string", g.proof_problems(dict(rec, proof=dict(rec["proof"], hmac=5)), "t", B3), NOPROOF)
chk("proof_problems an hmac that is not 64 hex", g.proof_problems(dict(rec, proof=dict(rec["proof"], hmac="ab")), "t", B3), NOPROOF)
chk("proof_problems an upper-case hmac", g.proof_problems(dict(rec, proof=dict(rec["proof"], hmac=rec["proof"]["hmac"].upper())), "t", B3), NOPROOF)
NOB = "no operator builder identity"
chk("proof_problems no builder", g.proof_problems(rec, "t", None), NOB)
chk("proof_problems a builder that is a list", g.proof_problems(rec, "t", [0, bp, 65534]), NOB)
chk("proof_problems a builder of one", g.proof_problems(rec, "t", (0,)), NOB)
chk("proof_problems a builder of four", g.proof_problems(rec, "t", (0, bp, 65534, 1)), NOB)
chk("proof_problems another build uid than the pin's", g.proof_problems(rec, "t", (0, bp, 65533)), "build processes ran as uid")
chk("proof_problems a pin of two does not judge the build uid", g.proof_problems(signed(build_uid=7), "t", (0, bp)), "")
chk("proof_problems no key at the pinned parent", g.proof_problems(rec, "t", (0, S + "/nokd/bp", 65534)), "proof cannot be checked")
chk("proof_problems another uid owns the key", g.proof_problems(rec, "t", (5, bp, 65534)), "proof cannot be checked")
chk("proof_problems a record edited after it was signed", g.proof_problems(dict(rec, x=2), "t", B3), "does not hold")
chk("proof_problems a proof signed under another key", g.proof_problems(dict(rec, proof=dict(rec["proof"], hmac="0" * 64)), "t", B3), "does not hold")

# write(path, rec): signs only a record whose tools are unchanged and whose key is the builder's.
wp = S + "/rec.json"
chk("write measure_problem: the tools changed since begin", run(g.write, wp, {"proof": {"id": PID}, "measured": {"cargo": "x"}, "toolchain": {"cargo": S + "/missing/cargo", "rustc": S + "/missing/rustc"}}), "cannot re-measure the build's tools")
chk("write a record whose key is not usable", run(g.write, wp, {"proof": {"id": "../x"}, "build_parent": bp, "builder_uid": 0}), "names no usable proof id")
r2 = {"build_uid": 65534, "build_parent": bp, "builder_uid": 0, "proof": {"schema": g.PROOF_SCHEMA, "id": PID, "hmac": ""}}
g.write(wp, r2)
signed_rec = json.load(open(wp))
chk("write signs a record the pinned builder can sign", g.proof_problems(signed_rec, "t", B3), "")
chk("measure_problem a record with no measurement", g.measure_problem({}), "")
chk("measure_problem a measurement whose tools are gone", g.measure_problem({"measured": {"cargo": "x"}, "toolchain": {"cargo": S + "/missing/cargo", "rustc": "r"}}), "cannot re-measure the build's tools")
