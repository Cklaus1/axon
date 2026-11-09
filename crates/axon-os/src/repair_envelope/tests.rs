//! NH-R1 §7 S1-S4, S7, S9 (and the axon-os half of S5) for the repair envelope.
//! Keys here are TEST keys derived from a fixed, public byte string.

use super::*;
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

/// A fixed TEST operator key (public, never a real secret).
const OP_KEY: &[u8] = b"nh-r1 test operator key, not a secret";
const NOW: u64 = 1_800_000_000;

fn mint() -> MintKey {
    MintKey::derive(OP_KEY).expect("test key")
}

fn sample() -> RepairEnvelope {
    RepairEnvelope {
        schema: ENVELOPE_SCHEMA.into(),
        envelope_id: "0123456789abcdef0123456789abcdef".into(),
        spent_store_id: "fedcba9876543210fedcba9876543210".into(),
        run_ledger_id: "00112233445566778899aabbccddeeff".into(),
        issued_by: "operator".into(),
        issued_unix: NOW - 10,
        expires_unix: NOW + 3_600,
        task: Task {
            workspace: "/srv/ws".into(),
            file: "src/main.axon".into(),
            symbol: "parse".into(),
            pristine_sha256: "a".repeat(64),
            imports_digest: format!("axsha256:{}", "b".repeat(64)),
        },
        write_prefixes: vec!["src".into()],
        scopes: Scopes {
            fs_read: vec!["/srv/data".into()],
            fs_write: vec!["/srv/out".into()],
            net: vec!["api.example.com".into()],
        },
        capabilities: Capabilities {
            fence_sha256: "c".repeat(64),
            target_grant: json!({"symbol": "parse", "effects": ["fs_read"]}),
        },
        check: Check {
            name: "test_parse".into(),
            digest: "d".repeat(64),
        },
        budget: Budget {
            max_proposals: 4,
            deadline_ms: 60_000,
            check_timeout_ms: 10_000,
            candidates: 3,
            max_steps: 8,
        },
        generator: Generator {
            kind: "ai".into(),
            model: "claude-x".into(),
            provider: "anthropic".into(),
            endpoint_host: "api.anthropic.com:443".into(),
        },
        decision: DECISION.into(),
        sig: String::new(),
    }
}

fn signed() -> String {
    mint().sign(&sample()).expect("sign")
}

/// Sign an arbitrary JSON value the way `MintKey::sign` does, with no typed
/// or intrinsic checks: a correctly-signed document that is NOT a valid envelope.
fn sign_raw(mut v: Value) -> String {
    v.as_object_mut().unwrap().remove("sig");
    let msg = signed_message(&v).unwrap();
    let sig = format!("{SIG_PREFIX}{}", hex(mint().0.sign(&msg).as_ref()));
    v.as_object_mut()
        .unwrap()
        .insert("sig".into(), Value::String(sig));
    String::from_utf8(axon_canonical::canonical_bytes(&v).unwrap()).unwrap()
}

fn me() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "axon-os-env-{tag}-{}-{}",
        std::process::id(),
        new_id().unwrap()
    ));
    std::fs::create_dir(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

// ── S1: known answers ────────────────────────────────────────────────────────

#[test]
fn s1_seed_public_key_and_signature_known_answers() {
    let seed = axon_attest::hmac_sha256(OP_KEY, b"axon-approval repair-envelope v1");
    assert_eq!(
        hex(&seed),
        KAT_SEED,
        "signing KAT repair-envelope seed (pins ENVELOPE_SEED_DOMAIN)"
    );
    let kp = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    use ring::signature::KeyPair;
    assert_eq!(hex(kp.public_key().as_ref()), KAT_PUBLIC);
    assert_eq!(
        mint().verify_key().to_hex(),
        KAT_PUBLIC,
        "signing KAT the mint key is not the G5-derived Ed25519 key"
    );
    let env = signed();
    let v: Value = serde_json::from_str(&env).unwrap();
    assert_eq!(
        v["sig"].as_str().unwrap(),
        KAT_SIG,
        "signing KAT the envelope signature changed (domain, canonical form or key)"
    );
    let ok = verify(&env, &mint().verify_key(), NOW).expect("the KAT envelope verifies");
    assert_eq!(
        ok.digest, KAT_DIGEST,
        "the envelope_digest is cl22 over the signed envelope"
    );
    assert_eq!(
        ok.digest,
        axon_canonical::digest_value(&v).unwrap(),
        "envelope_digest = cl22(canonical bytes incl. sig)"
    );
    assert_eq!(format!("{:?}", mint()), "MintKey(<redacted>)");
}

// Computed independently (Python `hmac` + `cryptography` Ed25519 + `json.dumps(sort_keys=True,
// separators=(',', ':'), ensure_ascii=False)`) from the same fixture (absolute pins since security F4;
// regenerated for EG-1: schema `axon-approval/5`, endpoint `api.anthropic.com:443`).
const KAT_SEED: &str = "b73c9e09b4c8941b5d5a34cc772396ee467235a588b81bf6556c72e153edbbcd";
const KAT_PUBLIC: &str = "7817267170d04cbb2259f5a63ee290e1d6d494a28a0c9bf75f0e7f270aec4aed";
const KAT_SIG: &str = "axenv1:f4f97dfccb1ba0a2a65de1b4d46bdfcd72c7e394edc5709dec5938cfc002f2aa38a32a0c59598fe5fa49ac8f7271b93584668a7dd45e3eb65616ed940c94610c";
const KAT_DIGEST: &str = "cl22:7de43154067dad1bdcbbbeb34293fe9e5b08dc37a81bc9af7e1577d2fd45288a";

#[test]
fn s1_the_envelope_is_written_in_its_canonical_form() {
    let env = signed();
    let v = axon_canonical::parse_value(&env).unwrap();
    assert_eq!(
        env.as_bytes(),
        axon_canonical::canonical_bytes(&v).unwrap().as_slice(),
        "the minted envelope is its own cl22 canonical bytes"
    );
}

// ── S2: every field is bound ─────────────────────────────────────────────────

/// Every JSON pointer to a leaf or an array of `v`.
fn pointers(v: &Value, at: String, out: &mut Vec<String>) {
    match v {
        Value::Object(o) => {
            if !at.is_empty() {
                out.push(at.clone());
            }
            for (k, x) in o {
                pointers(x, format!("{at}/{k}"), out);
            }
        }
        Value::Array(a) => {
            out.push(at.clone());
            for (i, x) in a.iter().enumerate() {
                pointers(x, format!("{at}/{i}"), out);
            }
        }
        _ => out.push(at),
    }
}

fn altered(v: &Value) -> Value {
    match v {
        Value::String(s) => Value::String(format!("{s}0")),
        Value::Number(n) => json!(n.as_u64().unwrap() + 1),
        Value::Bool(b) => json!(!b),
        Value::Array(a) => {
            let mut a = a.clone();
            a.push(json!("zz"));
            Value::Array(a)
        }
        Value::Object(o) => {
            let mut o = o.clone();
            o.insert("zz".into(), json!(1));
            Value::Object(o)
        }
        Value::Null => json!(0),
    }
}

#[test]
fn s2_altering_any_field_is_refused() {
    let good: Value = serde_json::from_str(&signed()).unwrap();
    let mut ptrs = Vec::new();
    pointers(&good, String::new(), &mut ptrs);
    ptrs.retain(|p| p != "/sig");
    assert!(
        ptrs.len() > 30,
        "the table covers every nested field ({})",
        ptrs.len()
    );
    for p in &ptrs {
        let mut v = good.clone();
        let slot = v.pointer_mut(p).unwrap();
        *slot = altered(slot);
        let text = serde_json::to_string(&v).unwrap();
        let e = verify(&text, &mint().verify_key(), NOW)
            .expect_err(&format!("ATTACK: an envelope with {p} altered verified"));
        assert!(
            e.contains("signature does not verify") || p == "/schema",
            "{p}: refused for the wrong reason: {e}"
        );
    }
}

#[test]
fn s2_duplicate_key_float_unknown_and_absent_fields_are_refused() {
    let env = signed();
    let key = mint().verify_key();
    let dup = env.replacen('{', "{\"decision\":\"authorize-operate\",", 1);
    let e = verify(&dup, &key, NOW).expect_err("ATTACK: a duplicate key was read");
    assert!(e.contains("duplicate key"), "{e}");
    let float = env.replace(
        &format!("\"issued_unix\":{}", NOW - 10),
        &format!("\"issued_unix\":{}.0", NOW - 10),
    );
    assert_ne!(float, env);
    verify(&float, &key, NOW).expect_err("ATTACK: a float was read");

    // Correctly SIGNED but not a valid envelope: the typed layer refuses.
    let mut v: Value = serde_json::from_str(&env).unwrap();
    v["task"]["extra"] = json!("x");
    let e =
        verify(&sign_raw(v), &key, NOW).expect_err("ATTACK: an unknown nested field was accepted");
    assert!(e.contains("unknown field"), "{e}");
    let mut v: Value = serde_json::from_str(&env).unwrap();
    v["extra"] = json!("x");
    let e = verify(&sign_raw(v), &key, NOW).expect_err("ATTACK: an unknown field was accepted");
    assert!(e.contains("unknown field"), "{e}");
    let mut v: Value = serde_json::from_str(&env).unwrap();
    v["budget"].as_object_mut().unwrap().remove("max_steps");
    let e = verify(&sign_raw(v), &key, NOW).expect_err("ATTACK: an absent field was accepted");
    assert!(e.contains("missing field"), "{e}");
    let mut v: Value = serde_json::from_str(&env).unwrap();
    v["budget"]["max_steps"] = Value::Null;
    verify(&sign_raw(v), &key, NOW).expect_err("ATTACK: a null limit was accepted");
}

#[test]
fn s2_signed_but_intrinsically_invalid_envelopes_are_refused() {
    let key = mint().verify_key();
    let cases: Vec<(&str, Value)> = vec![
        ("decision", json!({"decision": "approve"})),
        (
            "generator kind",
            json!({"generator": {"kind": "cmd", "model": "m", "provider": "p", "endpoint_host": "h:443"}}),
        ),
        ("envelope id", json!({"envelope_id": "ABC"})),
        ("store id", json!({"spent_store_id": "x"})),
        ("run ledger id", json!({"run_ledger_id": "../ledger"})),
        ("relative workspace", json!({"task": {"workspace": "ws"}})),
        (
            "dotdot workspace",
            json!({"task": {"workspace": "/srv/../etc"}}),
        ),
        ("dotdot file", json!({"task": {"file": "../etc/passwd"}})),
        ("absolute file", json!({"task": {"file": "/etc/passwd"}})),
        ("empty symbol", json!({"task": {"symbol": ""}})),
        ("star prefix", json!({"write_prefixes": ["*"]})),
        ("no prefix", json!({"write_prefixes": []})),
        ("dotdot prefix", json!({"write_prefixes": ["src/../.."]})),
        (
            "fs_write over the file",
            json!({"scopes": {"fs_write": ["/srv/ws/src"]}}),
        ),
        (
            "fs_write absolute over the file",
            json!({"scopes": {"fs_write": ["/srv/ws/src/main.axon"]}}),
        ),
        (
            "fs_write under the file",
            json!({"scopes": {"fs_write": ["/srv/ws/src/main.axon/x"]}}),
        ),
        ("net star", json!({"scopes": {"net": ["*"]}})),
        ("zero budget", json!({"budget": {"max_proposals": 0}})),
        (
            "ttl above max",
            json!({"expires_unix": NOW - 10 + MAX_TTL_S + 1}),
        ),
        // Inside the clock window (issued within ISSUED_SKEW_S, not yet
        // expired), so only the order check can refuse it.
        (
            "expires before issued",
            json!({"issued_unix": NOW + 30, "expires_unix": NOW + 20}),
        ),
        (
            "bad imports digest",
            json!({"task": {"imports_digest": "b".repeat(64)}}),
        ),
        ("empty issuer", json!({"issued_by": " "})),
        (
            "pristine digest",
            json!({"task": {"pristine_sha256": "A".repeat(64)}}),
        ),
        (
            "fence digest",
            json!({"capabilities": {"fence_sha256": "f"}}),
        ),
        (
            "target grant",
            json!({"capabilities": {"target_grant": []}}),
        ),
        ("check name", json!({"check": {"name": ""}})),
        ("check digest", json!({"check": {"digest": "d"}})),
        ("generator model", json!({"generator": {"model": ""}})),
        ("net empty", json!({"scopes": {"net": [""]}})),
        ("fs_read dotdot", json!({"scopes": {"fs_read": ["../x"]}})),
    ];
    for (what, patch) in cases {
        let mut v: Value = serde_json::from_str(&signed()).unwrap();
        merge(&mut v, &patch);
        let e = verify(&sign_raw(v), &key, NOW).expect_err(&format!(
            "ATTACK: a signed envelope with an invalid {what} verified"
        ));
        assert!(!e.contains("signature"), "{what}: {e}");
    }
    // A `..` workspace is refused as a workspace, not only later by the
    // fs_write overlap guard that a non-normalized path also trips.
    let mut v: Value = serde_json::from_str(&signed()).unwrap();
    merge(&mut v, &json!({"task": {"workspace": "/srv/../etc"}}));
    let e = verify(&sign_raw(v), &key, NOW).unwrap_err();
    assert!(
        e.contains("task.workspace"),
        "ATTACK: a `..` workspace was admitted as a workspace: {e}"
    );
    // After the `..` workspace: an absolute pin's normalization (`/srv/*`) is
    // the same `normalized_abs` the workspace uses, so checked first it would
    // take that guard's attack (g01 M4288 vs M4269).
    let mut v: Value = serde_json::from_str(&signed()).unwrap();
    merge(&mut v, &json!({"scopes": {"fs_write": ["/srv/*"]}}));
    let e = verify(&sign_raw(v), &key, NOW)
        .expect_err("ATTACK: a signed envelope with an invalid fs_write star verified");
    assert!(!e.contains("signature"), "fs_write star: {e}");
}

/// Security F4: a relative `fs_read`/`fs_write` pin resolves against the
/// check child's cwd (the workspace) and skips the phase-A symlink walk and
/// policy check, which only see absolute pins. It is refused at mint (sign)
/// and at verify, whatever it names.
#[test]
fn f4_a_relative_pin_is_refused_at_mint_and_at_verify() {
    let key = mint().verify_key();
    for (kind, pin) in [
        ("fs_read", "data"),
        ("fs_write", "out"),
        ("fs_read", "src/lib"),
        ("fs_write", "keys/verify.pub"),
    ] {
        let mut e = sample();
        match kind {
            "fs_read" => e.scopes.fs_read = vec![pin.into()],
            _ => e.scopes.fs_write = vec![pin.into()],
        }
        let err = mint().sign(&e).expect_err(&format!(
            "ATTACK: an envelope with a relative {kind} pin {pin:?} was minted"
        ));
        assert!(err.contains("is relative"), "{kind} {pin}: {err}");
        let mut v: Value = serde_json::from_str(&signed()).unwrap();
        merge(&mut v, &json!({"scopes": {kind: [pin]}}));
        let err = verify(&sign_raw(v), &key, NOW).expect_err(&format!(
            "ATTACK: a signed envelope with a relative {kind} pin {pin:?} verified"
        ));
        assert!(err.contains("is relative"), "{kind} {pin}: {err}");
    }
}

fn merge(v: &mut Value, patch: &Value) {
    match (v, patch) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, x) in b {
                match a.get_mut(k) {
                    Some(slot) if x.is_object() && slot.is_object() => merge(slot, x),
                    _ => {
                        a.insert(k.clone(), x.clone());
                    }
                }
            }
        }
        (v, p) => *v = p.clone(),
    }
}

#[test]
fn s2_the_minter_refuses_an_invalid_envelope() {
    let mut e = sample();
    e.decision = "approve".into();
    mint()
        .sign(&e)
        .expect_err("ATTACK: the minter signed an invalid envelope");
}

#[test]
fn s2_a_malformed_sig_or_schema_is_refused() {
    let key = mint().verify_key();
    let good: Value = serde_json::from_str(&signed()).unwrap();
    let sig = good["sig"].as_str().unwrap().to_string();
    let hexpart = sig.strip_prefix(SIG_PREFIX).unwrap().to_string();
    let cases: Vec<(&str, Option<Value>, &str)> = vec![
        ("a numeric sig", Some(json!(7)), "is not a string"),
        ("no sig", None, "has no `sig`"),
        (
            "an unprefixed sig",
            Some(json!(hexpart)),
            "does not start with",
        ),
        (
            "a short sig",
            Some(json!(&sig[..sig.len() - 2])),
            "128 lowercase hex",
        ),
        (
            "an uppercase sig",
            Some(json!(format!("{SIG_PREFIX}{}", hexpart.to_uppercase()))),
            "128 lowercase hex",
        ),
    ];
    for (what, s, want) in cases {
        let mut v = good.clone();
        match s {
            Some(s) => v["sig"] = s,
            None => {
                v.as_object_mut().unwrap().remove("sig");
            }
        }
        let e = verify(&v.to_string(), &key, NOW)
            .expect_err(&format!("ATTACK: an envelope with {what} verified"));
        assert!(
            e.contains(want),
            "ATTACK: {what} was refused only by a later check: {e}"
        );
    }
    let mut v = good.clone();
    v["schema"] = json!("axon-envelope/4");
    let e = verify(&v.to_string(), &key, NOW).expect_err("ATTACK: another schema verified");
    assert!(
        e.contains("not axon-approval/5"),
        "ATTACK: another schema was refused only by a later check: {e}"
    );
    // The minter judges the schema itself (verify judged it before validate).
    let mut e = sample();
    e.schema = "axon-approval/3".into();
    let r = mint()
        .sign(&e)
        .expect_err("ATTACK: the minter signed another schema");
    assert!(r.contains("schema"), "{r}");
}

#[test]
fn the_envelope_constants_are_the_documented_ones() {
    // Literal pins (NH-R1 §4, §5.5): a consistent edit of a constant is
    // invisible to every test that reads it through the const.
    assert_eq!(
        ENVELOPE_SCHEMA, "axon-approval/5",
        "ATTACK: ENVELOPE_SCHEMA changed"
    );
    assert_eq!(SIG_PREFIX, "axenv1:", "ATTACK: SIG_PREFIX changed");
    assert_eq!(MAX_TTL_S, 86_400, "ATTACK: MAX_TTL_S changed");
    assert_eq!(ISSUED_SKEW_S, 60, "ATTACK: ISSUED_SKEW_S changed");
    assert_eq!(DECISION, "authorize-operate", "ATTACK: DECISION changed");
    assert_eq!(GENERATOR_KIND, "ai", "ATTACK: GENERATOR_KIND changed");
    assert_eq!(STORE_ID_FILE, "store-id", "ATTACK: STORE_ID_FILE changed");
    // MintKey::from_env reads the operator key under this exact name.
    assert_eq!(
        crate::approval::APPROVAL_KEY_ENV,
        "AXON_APPROVAL_KEY",
        "ATTACK: APPROVAL_KEY_ENV changed"
    );
    assert_eq!(
        (VERIFY_KEY.dir, SPENT_STORE.dir),
        (false, true),
        "ATTACK: the verify key is not a file or the spent store not a directory"
    );
    assert_eq!(
        (VERIFY_KEY.what, SPENT_STORE.what),
        ("repair-envelope verify key", "repair-envelope spent store"),
        "ATTACK: a private-path label changed"
    );
}

// ── S3: keys ─────────────────────────────────────────────────────────────────

#[test]
fn s3_wrong_key_hmac_and_token_shaped_envelopes_are_refused() {
    let env = signed();
    let other = MintKey::derive(b"another operator key, also a test key").unwrap();
    let e = verify(&env, &other.verify_key(), NOW).expect_err("ATTACK: verified under another key");
    assert!(e.contains("signature does not verify"), "{e}");
    // An HMAC "envelope": a MAC under the G5 seed instead of a signature.
    let mut v: Value = serde_json::from_str(&env).unwrap();
    v.as_object_mut().unwrap().remove("sig");
    let mac = axon_attest::hmac_sha256(OP_KEY, &signed_message(&v).unwrap());
    v["sig"] = json!(format!("axmac1:{}", hex(&mac)));
    verify(&v.to_string(), &mint().verify_key(), NOW)
        .expect_err("ATTACK: an HMAC envelope verified");
    v["sig"] = json!(format!("{SIG_PREFIX}{}{}", hex(&mac), hex(&mac)));
    let e = verify(&v.to_string(), &mint().verify_key(), NOW)
        .expect_err("ATTACK: an HMAC padded to signature length verified");
    assert!(e.contains("signature does not verify"), "{e}");
}

#[test]
fn s3_the_runner_key_is_a_public_key_only() {
    let k = mint().verify_key();
    assert_eq!(k.to_hex().len(), 64);
    assert_eq!(VerifyKey::from_hex(&k.to_hex()).unwrap(), k);
    VerifyKey::from_hex(&"A".repeat(64)).expect_err("ATTACK: an uppercase-hex verify key loaded");
    VerifyKey::from_hex("00").expect_err("ATTACK: a short verify key loaded");
    VerifyKey::from_hex(&"g".repeat(64)).expect_err("ATTACK: a non-hex verify key loaded");
    MintKey::derive(&[7u8; 15]).expect_err("ATTACK: a 15-byte operator key minted");
}

// ── S4: time ─────────────────────────────────────────────────────────────────

#[test]
fn s4_expired_not_yet_valid_and_deadline_outliving_expiry_are_refused() {
    let env = signed();
    let k = mint().verify_key();
    let exp = NOW + 3_600;
    let e = verify(&env, &k, exp).expect_err("ATTACK: an envelope verified at its expiry");
    assert!(e.contains("expired"), "{e}");
    verify(&env, &k, exp - 1).expect("one second before expiry it verifies");
    let issued = NOW - 10;
    verify(&env, &k, issued - ISSUED_SKEW_S).expect("within the skew it verifies");
    let e = verify(&env, &k, issued - ISSUED_SKEW_S - 1)
        .expect_err("ATTACK: an envelope verified before it was issued");
    assert!(e.contains("not yet valid"), "{e}");
    let v = verify(&env, &k, NOW).unwrap().envelope;
    // now + deadline + grace must be <= expires
    v.fits_before_expiry(exp - 60, 58_000, 2_000)
        .expect("ends exactly at expiry");
    let e = v
        .fits_before_expiry(exp - 60, 58_000, 2_001)
        .expect_err("ATTACK: a run outliving its envelope was admitted");
    assert!(e.contains("expires at"), "{e}");
    v.fits_before_expiry(exp - 60, 58_001, 2_000)
        .expect_err("ATTACK: a deadline outliving the envelope was admitted");
}

#[test]
fn s4_ttl_above_max_is_refused_at_mint_and_at_verify() {
    let mut e = sample();
    e.expires_unix = e.issued_unix + MAX_TTL_S;
    mint().sign(&e).expect("exactly MAX_TTL_S is allowed");
    e.expires_unix += 1;
    let err = mint()
        .sign(&e)
        .expect_err("ATTACK: a TTL above the max was minted");
    assert!(err.contains("MAX_TTL_S"), "{err}");
}

// ── S7: cross-protocol ───────────────────────────────────────────────────────

#[test]
fn s7_a_program_token_is_not_an_envelope_and_the_reverse() {
    let k = mint().verify_key();
    for schema in ["axon-approval/3", "axon-approval/1"] {
        let tok = json!({"schema": schema, "program_digest": "x", "grant_digest": "y",
            "approved_by": "a", "decision": "approved", "risk": "low", "token_digest": "z"});
        let e =
            verify(&tok.to_string(), &k, NOW).expect_err("ATTACK: a token verified as an envelope");
        assert_eq!(
            e,
            format!("an {schema} approval token is not a repair envelope (want axon-approval/5)"),
            "ATTACK: a program token was read as an envelope (refused only by a later check)"
        );
    }
    // The other direction (`/4` handed to `verify_approval`) is
    // `approval::tests::a_repair_envelope_is_not_a_program_approval`.
}

// ── S9: key and store ownership ──────────────────────────────────────────────

#[test]
fn s9_the_verify_key_file_must_be_private_real_and_outside_the_workspace() {
    let d = tmpdir("key");
    let kf = d.join("verify.key");
    std::fs::write(&kf, format!("{}\n", mint().verify_key().to_hex())).unwrap();
    std::fs::set_permissions(&kf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let real = std::fs::canonicalize(&d).unwrap();
    assert_eq!(
        VerifyKey::load(&kf, me(), &[]).unwrap(),
        mint().verify_key()
    );
    let e = VerifyKey::load(&kf, me(), std::slice::from_ref(&real))
        .expect_err("ATTACK: a key inside the workspace loaded");
    assert!(e.contains("lies inside"), "{e}");
    let e = VerifyKey::load(&kf, me().wrapping_add(1), &[])
        .expect_err("ATTACK: a foreign-owned key loaded");
    assert!(e.contains("is owned by uid"), "{e}");
    std::fs::set_permissions(&kf, std::fs::Permissions::from_mode(0o644)).unwrap();
    let e = VerifyKey::load(&kf, me(), &[]).expect_err("ATTACK: a group/other-readable key loaded");
    assert!(e.contains("accessible to group or other"), "{e}");
    std::fs::set_permissions(&kf, std::fs::Permissions::from_mode(0o600)).unwrap();
    let link = d.join("link.key");
    std::os::unix::fs::symlink(&kf, &link).unwrap();
    let e = VerifyKey::load(&link, me(), &[]).expect_err("ATTACK: a symlinked key loaded");
    assert!(e.contains("symlink is never followed"), "{e}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn s9_the_spent_store_must_be_private_and_real() {
    let d = tmpdir("store");
    let id = init_store(&d, me()).unwrap();
    assert_eq!(store_id(&d, me()).unwrap(), id);
    init_store(&d, me()).expect_err("a store is initialised once");
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o770)).unwrap();
    let e = store_id(&d, me()).expect_err("ATTACK: a group-writable spent store was used");
    assert!(e.contains("accessible to group or other"), "{e}");
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    let e = store_id(&d, me().wrapping_add(1)).expect_err("ATTACK: a foreign spent store was used");
    assert!(e.contains("is owned by uid"), "{e}");
    let link = d.with_extension("link");
    std::os::unix::fs::symlink(&d, &link).unwrap();
    let e = store_id(&link, me()).expect_err("ATTACK: a symlinked spent store was used");
    assert!(e.contains("symlink is never followed"), "{e}");
    std::fs::remove_file(&link).unwrap();
    // store-id: a regular file holding 32 lowercase hex, mode 0600
    let sid = d.join(STORE_ID_FILE);
    assert_eq!(
        std::fs::metadata(&sid).unwrap().permissions().mode() & 0o777,
        0o600,
        "ATTACK: the store id was written readable by others"
    );
    std::fs::write(&sid, "XYZ\n").unwrap();
    let e = store_id(&d, me()).expect_err("ATTACK: a malformed store id was read");
    assert!(e.contains("32 lowercase hex"), "{e}");
    std::fs::remove_file(&sid).unwrap();
    // A FIFO: a read would take whatever another process writes into it (a
    // directory is refused by the read itself; a FIFO only by the file check).
    {
        use std::os::unix::ffi::OsStrExt;
        use std::os::unix::fs::OpenOptionsExt;
        use std::sync::atomic::{AtomicBool, Ordering};
        let c = std::ffi::CString::new(sid.as_os_str().as_bytes()).unwrap();
        // SAFETY: `c` is a valid NUL-terminated path.
        assert_eq!(
            unsafe { libc::mkfifo(c.as_ptr(), 0o600) },
            0,
            "setup: mkfifo"
        );
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let (p, s) = (sid.clone(), stop.clone());
        // A writer that answers a reader (if one ever opens the FIFO) with a
        // well-formed id, then closes; it gives up once the call returned.
        let writer = std::thread::spawn(move || {
            while !s.load(Ordering::SeqCst) {
                match std::fs::OpenOptions::new()
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(&p)
                {
                    Ok(mut f) => {
                        use std::io::Write;
                        let _ = f.write_all(b"0123456789abcdef0123456789abcdef\n");
                        return;
                    }
                    Err(_) => std::thread::sleep(std::time::Duration::from_millis(5)),
                }
            }
        });
        let got = store_id(&d, me());
        stop.store(true, Ordering::SeqCst);
        writer.join().unwrap();
        let e = got.expect_err("ATTACK: a FIFO store id was read");
        assert!(e.contains("not a regular file"), "{e}");
    }
    std::fs::remove_file(&sid).unwrap();
    std::fs::create_dir(&sid).unwrap();
    let e = store_id(&d, me()).expect_err("ATTACK: a directory store id was read");
    assert!(e.contains("not a regular file"), "{e}");
    std::fs::remove_dir_all(&d).unwrap();
}

// ── single use (the axon-os half of S5) ──────────────────────────────────────

fn issued_store(env: &mut RepairEnvelope) -> PathBuf {
    let d = tmpdir("spent");
    env.spent_store_id = init_store(&d, me()).unwrap();
    env.envelope_id = new_id().unwrap();
    issue(&d, me(), &env.spent_store_id, &env.envelope_id, NOW).unwrap();
    d
}

fn verified(env: &RepairEnvelope) -> VerifiedEnvelope {
    verify(&mint().sign(env).unwrap(), &mint().verify_key(), NOW).unwrap()
}

#[test]
fn s5_an_envelope_spends_once() {
    let mut env = sample();
    let d = issued_store(&mut env);
    let v = verified(&env);
    issue(&d, me(), &env.spent_store_id, &env.envelope_id, NOW)
        .expect_err("ATTACK: an issued envelope was issued again");
    init_store(&d, me()).expect_err("ATTACK: an initialised store was initialised again");
    consume(&d, me(), &v, NOW).expect("first use");
    let rec: Value = serde_json::from_str(
        &std::fs::read_to_string(d.join(format!("{}.used", env.envelope_id))).unwrap(),
    )
    .unwrap();
    assert_eq!(rec["envelope_digest"], json!(v.digest));
    assert_eq!(rec["spent_unix"], json!(NOW));
    let e = consume(&d, me(), &v, NOW).expect_err("ATTACK: an envelope spent twice");
    // The replay is NAMED as one (the run's report and the R6 ledger show a
    // second use), never reported as an envelope that was not issued.
    assert!(
        e.contains("already used"),
        "ATTACK: a replayed envelope was reported as never issued: {e}"
    );
    issue(&d, me(), &env.spent_store_id, &env.envelope_id, NOW)
        .expect_err("ATTACK: a used envelope was re-issued");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn s5_never_issued_foreign_and_fresh_stores_refuse_and_the_envelope_stays_unspent() {
    let mut env = sample();
    let d = issued_store(&mut env);
    let mut other = env.clone();
    other.envelope_id = new_id().unwrap();
    let e = consume(&d, me(), &verified(&other), NOW)
        .expect_err("ATTACK: a never-issued envelope spent");
    assert!(e.contains("never issued"), "{e}");
    // a fresh empty store: its own id, no .issued
    let fresh = tmpdir("fresh");
    init_store(&fresh, me()).unwrap();
    let e = consume(&fresh, me(), &verified(&env), NOW)
        .expect_err("ATTACK: a fresh store re-admitted it");
    assert!(
        e.contains("issued into"),
        "ATTACK: a fresh store was not refused by its id: {e}"
    );
    // a store holding the .issued but under another id
    let mut foreign = env.clone();
    foreign.spent_store_id = new_id().unwrap();
    consume(&d, me(), &verified(&foreign), NOW)
        .expect_err("ATTACK: a foreign store id was accepted");
    assert!(
        d.join(format!("{}.issued", env.envelope_id)).is_file(),
        "a refused envelope stays unspent"
    );
    consume(&d, me(), &verified(&env), NOW).expect("and it still spends once");
    std::fs::remove_dir_all(&d).unwrap();
    std::fs::remove_dir_all(&fresh).unwrap();
}

#[test]
fn s5_concurrent_consumers_exactly_one_proceeds() {
    for n in [2usize, 8] {
        let mut env = sample();
        let d = issued_store(&mut env);
        let v = verified(&env);
        let wins: usize = std::thread::scope(|s| {
            let hs: Vec<_> = (0..n)
                .map(|_| s.spawn(|| consume(&d, me(), &v, NOW).is_ok()))
                .collect();
            hs.into_iter().map(|h| usize::from(h.join().unwrap())).sum()
        });
        assert_eq!(
            wins, 1,
            "ATTACK: {wins} of {n} concurrent runs spent one envelope"
        );
        std::fs::remove_dir_all(&d).unwrap();
    }
}

#[test]
fn s5_consume_never_replaces_an_existing_used_record() {
    let mut env = sample();
    let d = issued_store(&mut env);
    // A .used AND an .issued (a re-issue raced a spend): the rename must not replace.
    std::fs::write(d.join(format!("{}.used", env.envelope_id)), b"{}\n").unwrap();
    let e =
        consume(&d, me(), &verified(&env), NOW).expect_err("ATTACK: a spent record was replaced");
    assert!(e.contains("already used"), "{e}");
    std::fs::remove_dir_all(&d).unwrap();
}

#[test]
fn s5_issue_and_consume_refuse_malformed_records() {
    let mut env = sample();
    let d = issued_store(&mut env);
    let issued = d.join(format!("{}.issued", env.envelope_id));
    assert_eq!(
        std::fs::metadata(&issued).unwrap().permissions().mode() & 0o777,
        0o600,
        "ATTACK: an issued record was written readable by others"
    );
    let e = issue(&d, me(), &env.spent_store_id, "../x", NOW)
        .expect_err("ATTACK: a path-shaped envelope id was issued");
    assert!(e.contains("32 lowercase hex"), "{e}");
    let e = issue(&d, me(), &new_id().unwrap(), &new_id().unwrap(), NOW)
        .expect_err("ATTACK: an envelope was issued into a store of another id");
    assert!(e.contains("has id"), "{e}");
    std::fs::remove_file(&issued).unwrap();
    std::fs::create_dir(&issued).unwrap();
    let e = consume(&d, me(), &verified(&env), NOW)
        .expect_err("ATTACK: a directory .issued record was spent");
    // Refused BEFORE the spend: the directory was not renamed to `.used`
    // (a later write failure would leave it spent and the refusal late).
    assert!(
        issued.is_dir() && !d.join(format!("{}.used", env.envelope_id)).exists(),
        "ATTACK: a directory .issued record was spent ({e})"
    );
    assert!(e.contains("not a regular file"), "{e}");
    std::fs::remove_dir_all(&d).unwrap();
}

// ── §5.2 no-widen ────────────────────────────────────────────────────────────

#[test]
fn s6_cli_limits_only_narrow() {
    let env = sample();
    let eff = env.narrow(&Requested::default()).unwrap();
    assert_eq!(
        (
            eff.max_proposals,
            eff.deadline_ms,
            eff.check_timeout_ms,
            eff.candidates,
            eff.max_steps
        ),
        (4, 60_000, 10_000, 3, 8),
        "absent CLI values take the envelope's"
    );
    assert_eq!(eff.write_prefixes, vec!["src".to_string()]);
    let same = Requested {
        max_proposals: Some(4),
        deadline_ms: Some(60_000),
        check_timeout_ms: Some(10_000),
        candidates: Some(3),
        max_steps: Some(8),
        write_prefixes: vec!["src/a".into()],
    };
    assert_eq!(
        env.narrow(&same).unwrap().write_prefixes,
        vec!["src/a".to_string()]
    );
    for (flag, r) in [
        (
            "--max-proposals",
            Requested {
                max_proposals: Some(5),
                ..Default::default()
            },
        ),
        (
            "--deadline-ms",
            Requested {
                deadline_ms: Some(60_001),
                ..Default::default()
            },
        ),
        (
            "--check-timeout-ms",
            Requested {
                check_timeout_ms: Some(10_001),
                ..Default::default()
            },
        ),
        (
            "--candidates",
            Requested {
                candidates: Some(4),
                ..Default::default()
            },
        ),
        (
            "--budget",
            Requested {
                max_steps: Some(9),
                ..Default::default()
            },
        ),
    ] {
        let e = env
            .narrow(&r)
            .expect_err(&format!("ATTACK: {flag} widened the envelope"));
        assert!(e.starts_with(flag), "{e}");
    }
    for p in ["srcx", "lib", ".", "src/../lib", "/src"] {
        env.narrow(&Requested {
            write_prefixes: vec![p.into()],
            ..Default::default()
        })
        .expect_err(&format!("ATTACK: write prefix {p:?} widened the envelope"));
    }
}

#[test]
fn s6_the_generator_must_equal_the_envelopes() {
    let env = sample();
    env.check_generator("ai", "claude-x", "anthropic", "api.anthropic.com:443")
        .unwrap();
    for (k, m, p, h) in [
        ("cmd", "claude-x", "anthropic", "api.anthropic.com:443"),
        ("ai", "claude-y", "anthropic", "api.anthropic.com:443"),
        ("ai", "claude-x", "openai", "api.anthropic.com:443"),
        ("ai", "claude-x", "anthropic", "evil.example.com:443"),
        ("ai", "claude-x", "anthropic", "api.anthropic.com:8443"),
        ("ai", "claude-x", "anthropic", "api.anthropic.com"),
    ] {
        env.check_generator(k, m, p, h)
            .expect_err("ATTACK: another generator was admitted");
    }
    assert_eq!(env.principal(), "envelope:0123456789abcdef0123456789abcdef");
}

/// EG-1 (O-NHR-08): `/4` pinned the bare host, so any port on it was
/// admitted. A validly signed `/4` envelope is refused by name, never re-read
/// as `/5`; `verify_for_audit` still reads it under its own grammar.
#[test]
fn an_axon_approval_4_envelope_is_refused_by_name() {
    let key = mint().verify_key();
    let mut v = serde_json::to_value(sample()).unwrap();
    v["schema"] = json!("axon-approval/4");
    v["generator"]["endpoint_host"] = json!("api.anthropic.com");
    let env = sign_raw(v);
    let e = verify(&env, &key, NOW)
        .expect_err("ATTACK: O-NHR-08 an axon-approval/4 envelope verified for a run");
    assert!(
        e.contains("axon-approval/4") && e.contains("port"),
        "ATTACK: O-NHR-08 an axon-approval/4 envelope was not refused by name: {e}"
    );
    let audit = verify_for_audit(&env, &key, NOW).expect("the audit reads a retired /4");
    assert_eq!(audit.envelope.generator.endpoint_host, "api.anthropic.com");
    // The minter never signs a retired schema.
    let mut s = sample();
    s.schema = "axon-approval/4".into();
    mint()
        .sign(&s)
        .expect_err("ATTACK: O-NHR-08 the minter signed an axon-approval/4 envelope");
}

/// EG-1 (O-NHR-08): a `/5` endpoint is `host:port`; anything else is refused
/// at verify (signed) and at mint.
#[test]
fn an_endpoint_without_its_port_is_refused() {
    let key = mint().verify_key();
    for bad in [
        "api.anthropic.com",
        "h:0",
        "h:080",
        "h:65536",
        "h:",
        ":443",
        "H:443",
        "h h:443",
        "::1:443",
        "h:+443",
    ] {
        let mut v = serde_json::to_value(sample()).unwrap();
        v["generator"]["endpoint_host"] = json!(bad);
        let e = verify(&sign_raw(v), &key, NOW).expect_err(&format!(
            "ATTACK: O-NHR-08 the endpoint {bad:?} without a valid port verified"
        ));
        assert!(e.contains("host:port"), "{bad:?}: {e}");
        let mut s = sample();
        s.generator.endpoint_host = bad.into();
        mint().sign(&s).expect_err(&format!(
            "ATTACK: O-NHR-08 the endpoint {bad:?} without a valid port was minted"
        ));
    }
    for good in ["h:1", "h:65535", "[::1]:443", "127.0.0.1:18307"] {
        let mut s = sample();
        s.generator.endpoint_host = good.into();
        let env = mint().sign(&s).expect(good);
        verify(&env, &key, NOW).expect(good);
    }
}
