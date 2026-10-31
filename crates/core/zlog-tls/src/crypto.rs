use std::fmt;
use std::str::FromStr;

use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};

const BASE32: &[u8; 32] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

fn sha256(bytes: &[u8]) -> [u8; 32] {
    let mut out = [0; 32];
    out.copy_from_slice(digest(&SHA256, bytes).as_ref());
    out
}

fn same(a: &[u8; 32], b: &[u8; 32]) -> bool {
    let diff = a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y));
    std::hint::black_box(diff) == 0
}

pub(crate) fn hex(bytes: &[u8], sep: &str) -> String {
    bytes
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(sep)
}

pub(crate) fn unhex_vec(text: &str) -> Option<Vec<u8>> {
    let digits = text.as_bytes();
    if !digits.len().is_multiple_of(2) {
        return None;
    }
    digits
        .chunks(2)
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let digits: Vec<u8> = text
        .bytes()
        .filter(|b| !matches!(b, b':' | b' ' | b'-'))
        .collect();
    if digits.len() != 64 {
        return None;
    }
    let mut out = [0; 32];
    for (o, pair) in out.iter_mut().zip(digits.chunks(2)) {
        *o = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
    }
    Some(out)
}

#[derive(Clone, PartialEq, Eq)]
pub struct Token(String);

impl Token {
    pub fn generate() -> Token {
        let mut bytes = [0u8; 20];
        SystemRandom::new()
            .fill(&mut bytes)
            .expect("the system random generator failed");
        let mut bits = 0u32;
        let mut n = 0;
        let mut text = String::new();
        for b in bytes {
            bits = (bits << 8) | u32::from(b);
            n += 8;
            while n >= 5 {
                n -= 5;
                if !text.is_empty() && text.len() % 5 == 4 {
                    text.push('-');
                }
                text.push(char::from(BASE32[((bits >> n) & 31) as usize]));
            }
        }
        Token(text)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn hash(&self) -> TokenHash {
        TokenHash::of(&self.0)
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct TokenHash([u8; 32]);

impl TokenHash {
    fn of(token: &str) -> TokenHash {
        let norm: String = token
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .map(|c| c.to_ascii_uppercase())
            .collect();
        TokenHash(sha256(norm.as_bytes()))
    }

    pub fn verify(&self, token: &str) -> bool {
        same(&self.0, &TokenHash::of(token).0)
    }
}

impl fmt::Display for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0, ""))
    }
}

impl fmt::Debug for TokenHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TokenHash({self})")
    }
}

impl FromStr for TokenHash {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        unhex(s)
            .map(TokenHash)
            .ok_or_else(|| "not a token hash".into())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint([u8; 32]);

impl Fingerprint {
    pub fn of(cert_der: &[u8]) -> Fingerprint {
        Fingerprint(sha256(cert_der))
    }
}

impl fmt::Display for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&hex(&self.0, ":"))
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({self})")
    }
}

impl FromStr for Fingerprint {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        unhex(s)
            .map(Fingerprint)
            .ok_or_else(|| "not a SHA-256 fingerprint".into())
    }
}
