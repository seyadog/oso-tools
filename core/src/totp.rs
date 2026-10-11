//! One-time codes (TOTP, RFC 6238), the six digits an authenticator app shows. A seed is kept in the vault as a
//! secret of its own kind, either as the bare base32 text or as the `otpauth://` link a QR code holds.

use anyhow::{Result, bail};
use hmac::{Hmac, Mac};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Algo {
    Sha1,
    Sha256,
    Sha512,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Spec {
    pub secret: Vec<u8>,
    pub digits: u32,
    pub period: u64,
    pub algo: Algo,
}

/// RFC 4648 base32, any case, with spaces, dashes and padding ignored.
pub fn base32_decode(text: &str) -> Option<Vec<u8>> {
    let mut bits: u32 = 0;
    let mut have = 0;
    let mut out = vec![];
    for c in text.chars().filter(|c| !c.is_whitespace() && *c != '-' && *c != '=') {
        let v = match c.to_ascii_uppercase() {
            c @ 'A'..='Z' => c as u32 - 'A' as u32,
            c @ '2'..='7' => c as u32 - '2' as u32 + 26,
            _ => return None,
        };
        bits = (bits << 5) | v;
        have += 5;
        if have >= 8 {
            have -= 8;
            out.push((bits >> have) as u8);
            bits &= (1 << have) - 1;
        }
    }
    (!out.is_empty()).then_some(out)
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Ok(v) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(if b[i] == b'+' { b' ' } else { b[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A seed as kept in the vault: bare base32, or an `otpauth://totp/…?secret=…&digits=&period=&algorithm=` link.
pub fn parse(value: &str) -> Result<Spec> {
    let v = value.trim();
    if let Some(rest) = v.strip_prefix("otpauth://") {
        let Some(rest) = rest.strip_prefix("totp/") else { bail!("only time-based codes (otpauth://totp/…) are supported") };
        let query = rest.split_once('?').map(|(_, q)| q).unwrap_or("");
        let get = |k: &str| query.split('&').filter_map(|p| p.split_once('=')).find(|(n, _)| n.eq_ignore_ascii_case(k)).map(|(_, v)| percent_decode(v));
        let secret = get("secret").and_then(|s| base32_decode(&s)).ok_or_else(|| anyhow::anyhow!("the link has no readable secret"))?;
        let digits = get("digits").and_then(|d| d.parse().ok()).unwrap_or(6);
        let period = get("period").and_then(|d| d.parse().ok()).unwrap_or(30);
        let algo = match get("algorithm").map(|a| a.to_uppercase()).as_deref() {
            None | Some("SHA1") => Algo::Sha1,
            Some("SHA256") => Algo::Sha256,
            Some("SHA512") => Algo::Sha512,
            Some(o) => bail!("unknown algorithm {o}"),
        };
        return check(Spec { secret, digits, period, algo });
    }
    let Some(secret) = base32_decode(v) else { bail!("that is not a base32 secret or an otpauth:// link") };
    check(Spec { secret, digits: 6, period: 30, algo: Algo::Sha1 })
}

fn check(s: Spec) -> Result<Spec> {
    if !(6..=10).contains(&s.digits) || s.period == 0 || s.period > 3600 {
        bail!("the digits or the period of that code are out of range");
    }
    Ok(s)
}

fn mac(algo: Algo, key: &[u8], counter: u64) -> Vec<u8> {
    let msg = counter.to_be_bytes();
    match algo {
        Algo::Sha1 => Hmac::<sha1::Sha1>::new_from_slice(key).map(|mut m| {
            m.update(&msg);
            m.finalize().into_bytes().to_vec()
        }),
        Algo::Sha256 => Hmac::<sha2::Sha256>::new_from_slice(key).map(|mut m| {
            m.update(&msg);
            m.finalize().into_bytes().to_vec()
        }),
        Algo::Sha512 => Hmac::<sha2::Sha512>::new_from_slice(key).map(|mut m| {
            m.update(&msg);
            m.finalize().into_bytes().to_vec()
        }),
    }
    .unwrap_or_default()
}

/// The code for `unix` seconds.
pub fn code(spec: &Spec, unix: u64) -> String {
    let h = mac(spec.algo, &spec.secret, unix / spec.period);
    if h.len() < 20 {
        return String::new();
    }
    let off = (h[h.len() - 1] & 0xf) as usize;
    let n = u32::from_be_bytes([h[off] & 0x7f, h[off + 1], h[off + 2], h[off + 3]]) as u64;
    format!("{:0width$}", n % 10u64.pow(spec.digits), width = spec.digits as usize)
}

/// Seconds until the code changes.
pub fn seconds_left(spec: &Spec, unix: u64) -> u64 {
    spec.period - unix % spec.period
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(key: &[u8], algo: Algo) -> Spec {
        Spec { secret: key.to_vec(), digits: 8, period: 30, algo }
    }

    /// The test values of RFC 6238, appendix B.
    #[test]
    fn the_codes_of_the_rfc_are_reproduced() {
        let k1 = b"12345678901234567890";
        let k256 = b"12345678901234567890123456789012";
        let k512 = b"1234567890123456789012345678901234567890123456789012345678901234";
        for (t, sha1, sha256, sha512) in [
            (59u64, "94287082", "46119246", "90693936"),
            (1111111109, "07081804", "68084774", "25091201"),
            (1111111111, "14050471", "67062674", "99943326"),
            (1234567890, "89005924", "91819424", "93441116"),
            (2000000000, "69279037", "90698825", "38618901"),
            (20000000000, "65353130", "77737706", "47863826"),
        ] {
            assert_eq!(code(&spec(k1, Algo::Sha1), t), sha1, "sha1 at {t}");
            assert_eq!(code(&spec(k256, Algo::Sha256), t), sha256, "sha256 at {t}");
            assert_eq!(code(&spec(k512, Algo::Sha512), t), sha512, "sha512 at {t}");
        }
    }

    #[test]
    fn base32_reads_what_authenticator_apps_show() {
        assert_eq!(base32_decode("JBSWY3DPEHPK3PXP").unwrap(), b"Hello!\xDE\xAD\xBE\xEF");
        assert_eq!(base32_decode("jbsw y3dp-ehpk 3pxp==").unwrap(), b"Hello!\xDE\xAD\xBE\xEF", "spaces, dashes, padding and case do not matter");
        assert_eq!(base32_decode("GEZDGNBVGY3TQOJQ").unwrap(), b"1234567890");
        assert!(base32_decode("not base32!").is_none() && base32_decode("").is_none() && base32_decode("1189").is_none());
    }

    #[test]
    fn a_link_carries_the_digits_period_and_algorithm() {
        let s = parse("otpauth://totp/Example:me@x.com?secret=GEZDGNBVGY3TQOJQ&issuer=Example&digits=8&period=60&algorithm=SHA256").unwrap();
        assert_eq!((s.digits, s.period, s.algo, s.secret.as_slice()), (8, 60, Algo::Sha256, &b"1234567890"[..]));
        let plain = parse("GEZDGNBVGY3TQOJQ").unwrap();
        assert_eq!((plain.digits, plain.period, plain.algo), (6, 30, Algo::Sha1));
        assert_eq!(parse("otpauth://totp/x?secret=GEZDGNBVGY3TQOJQ").unwrap(), plain);
        assert_eq!(code(&plain, 59).len(), 6);
        for bad in ["otpauth://hotp/x?secret=GEZDGNBVGY3TQOJQ", "otpauth://totp/x?issuer=a", "otpauth://totp/x?secret=GEZDGNBVGY3TQOJQ&digits=3", "otpauth://totp/x?secret=GEZDGNBVGY3TQOJQ&period=0", "otpauth://totp/x?secret=GEZDGNBVGY3TQOJQ&algorithm=MD5", "hello! world", ""] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        assert_eq!(parse("otpauth://totp/x?secret=GEZDGNBVGY3T%51OJQ").unwrap().secret, b"1234567890", "percent-escapes are read");
    }

    #[test]
    fn the_time_left_counts_down_inside_each_period() {
        let s = parse("GEZDGNBVGY3TQOJQ").unwrap();
        assert_eq!((seconds_left(&s, 60), seconds_left(&s, 89), seconds_left(&s, 61)), (30, 1, 29));
        assert_eq!(code(&s, 60), code(&s, 89), "the same code for the whole period");
        assert_ne!(code(&s, 89), code(&s, 90));
    }
}
