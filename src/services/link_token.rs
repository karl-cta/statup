//! The secret half of the links sent by email, shared by the password reset
//! and the subscription links. A token reads `<id>.<secret>`: the id finds
//! the stored row, the secret proves the holder got the email.

use std::fmt::Write as _;

use rand::Rng;
use rand::distributions::Alphanumeric;
use sha2::{Digest, Sha256};

/// Characters of the secret: about 190 bits, beyond any guessing.
const SECRET_LENGTH: usize = 32;

pub(super) fn new_secret() -> String {
    rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(SECRET_LENGTH)
        .map(char::from)
        .collect()
}

pub(super) fn token(id: i64, secret: &str) -> String {
    format!("{id}.{secret}")
}

/// The id and the secret of a well formed token.
pub(super) fn parse(token: &str) -> Option<(i64, &str)> {
    let (id, secret) = token.trim().split_once('.')?;
    let well_formed =
        secret.len() == SECRET_LENGTH && secret.bytes().all(|b| b.is_ascii_alphanumeric());
    well_formed.then_some(())?;
    Some((id.parse().ok()?, secret))
}

/// The SHA-256 of a random secret, in hexadecimal, as it is stored. A fast
/// hash is enough: the secret is long and random, so a slow one protects
/// nothing, and it would let anyone sending made-up links keep the server
/// hashing while sign-ins wait.
pub(super) fn secret_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    let mut text = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Whether `secret` is the one `stored` was made from.
pub(super) fn matches(secret: &str, stored: &str) -> bool {
    same_secret(&secret_hash(secret), stored)
}

/// Whether two secrets are the same. Every byte is compared whatever the
/// first ones say, so the time taken does not tell how much matched.
pub(super) fn same_secret(given: &str, stored: &str) -> bool {
    if given.len() != stored.len() {
        return false;
    }
    let difference = given
        .bytes()
        .zip(stored.bytes())
        .fold(0, |acc, (a, b)| acc | (a ^ b));
    difference == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_parses_back_to_its_id_and_secret() {
        let secret = new_secret();

        let link = token(42, &secret);
        let parsed = parse(&link);

        assert_eq!(parsed, Some((42, secret.as_str())));
    }

    #[test]
    fn a_malformed_token_is_refused() {
        let secret = new_secret();
        let short = &secret[..31];
        let symbols = format!("{}-", &secret[..31]);
        let cases = [
            secret.clone(),
            format!("7{secret}"),
            token(7, short),
            token(7, &symbols),
            token(7, ""),
            format!("seven.{secret}"),
            String::new(),
        ];

        for case in cases {
            assert_eq!(parse(&case), None, "{case:?}");
        }
    }

    #[test]
    fn whitespace_around_a_token_is_trimmed() {
        let secret = new_secret();

        let link = format!(" \n{}\t ", token(5, &secret));
        let parsed = parse(&link);

        assert_eq!(parsed, Some((5, secret.as_str())));
    }

    #[test]
    fn secrets_are_the_same_only_when_every_byte_is() {
        let secret = new_secret();
        let mut one_off = secret.clone().into_bytes();
        one_off[31] ^= 1;
        let one_off = String::from_utf8(one_off).unwrap();

        assert!(same_secret(&secret, &secret));
        assert!(!same_secret(&secret, &one_off));
        assert!(!same_secret(&secret, &secret[..31]));
        assert!(!same_secret("", &secret));
    }

    #[test]
    fn a_secret_matches_only_its_own_hash() {
        let secret = new_secret();
        let stored = secret_hash(&secret);

        assert_eq!(stored.len(), 64);
        assert!(matches(&secret, &stored));
        assert!(!matches(&new_secret(), &stored));
        assert!(!matches(
            &secret,
            "$argon2id$v=19$m=19456,t=2,p=1$c2FsdA$aGFzaA"
        ));
    }
}
