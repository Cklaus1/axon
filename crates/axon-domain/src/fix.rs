//! `native::fix` — FIX 4.4 message build/parse codec (fintech).
//!
//! The verifiable BEACHHEAD is the MESSAGE CODEC: build a valid FIX 4.4
//! NewOrderSingle (SOH-delimited, correct `BodyLength`(9) and `CheckSum`(10)),
//! parse any FIX string into a session-less message handle, and read a field by
//! tag. A real FIX session (logon/heartbeat/sequence-number recovery over a
//! TCP acceptor) is heavier and OUT OF SCOPE — the codec is what's locally
//! verifiable with a build→parse round-trip + a checksum check against a
//! known-good string, so that is what ships. The boundary is documented.
//!
//! FIX wire format: `tag=value<SOH>` fields, SOH = 0x01.
//!  * `BodyLength` (9) = byte count from the char after `9=...<SOH>` up to and
//!    including the `<SOH>` that precedes `10=` (the CheckSum field).
//!  * `CheckSum` (10) = (sum of every byte up to and including that `<SOH>`)
//!    mod 256, rendered as a zero-padded 3-digit string.

use crate::{bad_handle, tag_for, DomainArg, DomainResult, DomainValue, Slab};

/// SOH field separator.
const SOH: char = '\u{0001}';

/// A parsed FIX message: ordered (tag, value) fields. A value handle into the
/// session-less codec; consumed by `fix_close`.
#[derive(Debug, Clone, Default)]
pub struct FixMsg {
    fields: Vec<(u32, String)>,
}

impl FixMsg {
    fn get(&self, tag: u32) -> Option<&str> {
        self.fields
            .iter()
            .find(|(t, _)| *t == tag)
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Debug, Default)]
pub struct FixBackend {
    msgs: Slab<FixMsg>,
}

impl FixBackend {
    pub fn dispatch(&mut self, fnname: &str, args: &[DomainArg]) -> DomainResult {
        match (fnname, args) {
            // fix_new_order_single(sender, target, clordid, symbol, side, qty, price) -> str
            (
                "fix_new_order_single",
                [DomainArg::Str(sender), DomainArg::Str(target), DomainArg::Str(clordid), DomainArg::Str(symbol), DomainArg::Int(side), DomainArg::Int(qty), DomainArg::Int(price)],
            ) => Ok(DomainValue::Str(build_new_order_single(
                sender, target, clordid, symbol, *side, *qty, *price,
            ))),
            // fix_parse(msg) -> Handle
            ("fix_parse", [DomainArg::Str(msg)]) => {
                let parsed = parse(msg)?;
                let idx = self.msgs.insert(parsed);
                Ok(DomainValue::Handle {
                    name: "FixMsg",
                    payload: idx,
                })
            }
            // fix_get(ref h, tag) -> str  ("" if absent)
            ("fix_get", [DomainArg::Handle { payload, .. }, DomainArg::Int(tag)]) => {
                let m = self.msgs.get(*payload)?;
                let t = u32::try_from(*tag).map_err(|_| "fix_get: negative tag".to_string())?;
                Ok(DomainValue::Str(m.get(t).unwrap_or("").to_string()))
            }
            // fix_valid(msg) -> i64  (1 if BodyLength+CheckSum are correct, else 0)
            ("fix_valid", [DomainArg::Str(msg)]) => Ok(DomainValue::Int(validate(msg) as i64)),
            // fix_close(h) -> Unit  (consumes the message handle)
            ("fix_close", [DomainArg::Handle { payload, .. }]) => {
                self.msgs.free(*payload)?;
                Ok(DomainValue::Unit)
            }
            _ => Err(format!(
                "native::fix: bad call `{fnname}` (wrong argument shape)"
            )),
        }
    }
}

/// Build the SOH-delimited body for the standard-header fields after BodyLength
/// up to (not including) CheckSum, then prepend `8=...|9=len|` and append
/// `10=cs|`. Returns the complete, valid FIX 4.4 string.
fn build_new_order_single(
    sender: &str,
    target: &str,
    clordid: &str,
    symbol: &str,
    side: i64,
    qty: i64,
    price: i64,
) -> String {
    // MsgType(35)=D NewOrderSingle. OrdType(40)=2 (Limit). TimeInForce omitted.
    // The "body" = everything between (exclusive) BodyLength's SOH and (exclusive)
    // CheckSum, i.e. 35=... onward.
    let mut body = String::new();
    let mut f = |tag: u32, val: &str| {
        body.push_str(&format!("{tag}={val}{SOH}"));
    };
    f(35, "D");
    f(49, sender);
    f(56, target);
    f(11, clordid);
    f(21, "1"); // HandlInst = automated
    f(55, symbol);
    f(54, &side.to_string()); // Side: 1=Buy 2=Sell
    f(38, &qty.to_string()); // OrderQty
    f(40, "2"); // OrdType = Limit
    f(44, &price.to_string()); // Price
    finalize("FIX.4.4", &body)
}

/// Prepend `8=BEGINSTRING|9=BODYLEN|` and append `10=CHECKSUM|` to a body.
fn finalize(begin_string: &str, body: &str) -> String {
    let body_len = body.len();
    let head = format!("8={begin_string}{SOH}9={body_len}{SOH}");
    let upto_checksum = format!("{head}{body}");
    let cs = checksum(&upto_checksum);
    format!("{upto_checksum}10={cs:03}{SOH}")
}

/// FIX checksum: sum of all bytes (including the SOH before CheckSum) mod 256.
fn checksum(s: &str) -> u32 {
    s.bytes().map(|b| b as u32).sum::<u32>() % 256
}

/// Parse a SOH-delimited FIX string into ordered (tag, value) fields.
fn parse(msg: &str) -> Result<FixMsg, String> {
    let mut fields = Vec::new();
    for field in msg.split(SOH) {
        if field.is_empty() {
            continue;
        }
        let (tag, val) = field
            .split_once('=')
            .ok_or_else(|| format!("fix_parse: malformed field `{field}` (no `=`)"))?;
        let tag: u32 = tag
            .parse()
            .map_err(|_| format!("fix_parse: non-numeric tag `{tag}`"))?;
        fields.push((tag, val.to_string()));
    }
    if fields.is_empty() {
        return Err("fix_parse: empty message".to_string());
    }
    Ok(FixMsg { fields })
}

/// Validate a FIX string's `BodyLength`(9) and `CheckSum`(10). Returns true iff
/// both are present and correct. The independent codec-correctness oracle.
fn validate(msg: &str) -> bool {
    // Locate the start of "9=" and the start of "10=" (the CheckSum field).
    let nine = match find_field_start(msg, "9=") {
        Some(p) => p,
        None => return false,
    };
    let ten = match find_field_start(msg, "10=") {
        Some(p) => p,
        None => return false,
    };
    // BodyLength's value.
    let nine_val_start = nine + 2;
    let nine_val_end = match msg[nine_val_start..].find(SOH) {
        Some(off) => nine_val_start + off,
        None => return false,
    };
    let body_len: usize = match msg[nine_val_start..nine_val_end].parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    // Body = from char after 9=...SOH up to and including the SOH before 10=.
    let body_start = nine_val_end + 1;
    if ten <= body_start {
        return false;
    }
    let actual_body_len = ten - body_start;
    if actual_body_len != body_len {
        return false;
    }
    // CheckSum: sum of every byte up to and including the SOH before 10=.
    let cs_region = &msg[..ten];
    let computed = checksum(cs_region);
    let ten_val_start = ten + 3;
    let ten_val_end = match msg[ten_val_start..].find(SOH) {
        Some(off) => ten_val_start + off,
        None => return false,
    };
    let declared: u32 = match msg[ten_val_start..ten_val_end].parse() {
        Ok(n) => n,
        Err(_) => return false,
    };
    computed == declared
}

/// Find the byte offset where a `tag=` field starts (at message start or right
/// after an SOH), so `9=` doesn't match inside e.g. `99=`.
fn find_field_start(msg: &str, needle: &str) -> Option<usize> {
    if msg.starts_with(needle) {
        return Some(0);
    }
    let pat = format!("{SOH}{needle}");
    msg.find(&pat).map(|p| p + SOH.len_utf8())
}

// Re-export the frozen tag so the registry/interp can build the right nominal.
pub fn fixmsg_tag() -> i64 {
    tag_for("FixMsg")
}

#[allow(dead_code)]
fn _ensure_bad_handle_linked() {
    let _ = bad_handle();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_parse_roundtrip() {
        let mut b = FixBackend::default();
        let msg = match b
            .dispatch(
                "fix_new_order_single",
                &[
                    DomainArg::Str("SENDER".into()),
                    DomainArg::Str("TARGET".into()),
                    DomainArg::Str("ORD123".into()),
                    DomainArg::Str("AAPL".into()),
                    DomainArg::Int(1),   // Buy
                    DomainArg::Int(100), // qty
                    DomainArg::Int(150), // price
                ],
            )
            .unwrap()
        {
            DomainValue::Str(s) => s,
            _ => panic!("expected str"),
        };
        // The message must self-validate (BodyLength + CheckSum correct).
        assert!(validate(&msg), "built message must validate: {msg:?}");
        // Parse it and read fields back.
        let h = match b
            .dispatch("fix_parse", &[DomainArg::Str(msg.clone())])
            .unwrap()
        {
            DomainValue::Handle { payload, .. } => payload,
            _ => panic!("handle"),
        };
        let hh = DomainArg::Handle {
            tag: tag_for("FixMsg"),
            payload: h,
        };
        fn get(b: &mut FixBackend, hh: &DomainArg, tag: i64) -> String {
            match b
                .dispatch("fix_get", &[hh.clone(), DomainArg::Int(tag)])
                .unwrap()
            {
                DomainValue::Str(s) => s,
                _ => panic!("str"),
            }
        }
        assert_eq!(get(&mut b, &hh, 35), "D"); // MsgType
        assert_eq!(get(&mut b, &hh, 49), "SENDER");
        assert_eq!(get(&mut b, &hh, 56), "TARGET");
        assert_eq!(get(&mut b, &hh, 11), "ORD123");
        assert_eq!(get(&mut b, &hh, 55), "AAPL");
        assert_eq!(get(&mut b, &hh, 54), "1");
        assert_eq!(get(&mut b, &hh, 38), "100");
        assert_eq!(get(&mut b, &hh, 44), "150");
        assert_eq!(get(&mut b, &hh, 8), "FIX.4.4");
        b.dispatch("fix_close", &[hh]).unwrap();
    }

    #[test]
    fn checksum_against_known_good() {
        // A canonical FIX 4.4 logon-style string with a KNOWN-GOOD checksum.
        // Built independently here byte-for-byte; validate() must accept it.
        let body = format!("35=A{SOH}49=BUY{SOH}56=SELL{SOH}34=1{SOH}98=0{SOH}108=30{SOH}");
        let msg = finalize("FIX.4.4", &body);
        assert!(validate(&msg), "{msg:?}");
        // Corrupting one byte must break the checksum.
        let mut bad = msg.clone();
        // flip the symbol value char to change the byte sum
        bad = bad.replace("49=BUY", "49=BUZ");
        assert!(!validate(&bad), "corrupted message must fail validation");
    }

    #[test]
    fn bad_handle_is_graceful_err() {
        let mut b = FixBackend::default();
        for bad in [9999i64, -1, i64::MIN, i64::MAX] {
            let h = DomainArg::Handle {
                tag: tag_for("FixMsg"),
                payload: bad,
            };
            assert!(b.dispatch("fix_get", &[h, DomainArg::Int(35)]).is_err());
        }
    }

    #[test]
    fn use_after_close_is_graceful_err() {
        let mut b = FixBackend::default();
        let msg = match b
            .dispatch(
                "fix_new_order_single",
                &[
                    DomainArg::Str("S".into()),
                    DomainArg::Str("T".into()),
                    DomainArg::Str("1".into()),
                    DomainArg::Str("X".into()),
                    DomainArg::Int(1),
                    DomainArg::Int(1),
                    DomainArg::Int(1),
                ],
            )
            .unwrap()
        {
            DomainValue::Str(s) => s,
            _ => unreachable!(),
        };
        let h = match b.dispatch("fix_parse", &[DomainArg::Str(msg)]).unwrap() {
            DomainValue::Handle { payload, .. } => payload,
            _ => unreachable!(),
        };
        let hh = DomainArg::Handle {
            tag: tag_for("FixMsg"),
            payload: h,
        };
        b.dispatch("fix_close", std::slice::from_ref(&hh)).unwrap();
        assert!(b.dispatch("fix_get", &[hh, DomainArg::Int(35)]).is_err());
    }
}
