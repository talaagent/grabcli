//! The hydra device JWT.
//!
//! Grab's web app gates its menu and quote/order endpoints behind an
//! `x-hydra-jwt` header. It is an RS256 JWT whose claims are a browser
//! fingerprint, signed with a key the web app carries. The token has a ~10
//! minute TTL, so it is re-minted often.
//!
//! Two ways to obtain one:
//!
//! - **native** ([`mint_native`]): sign the claims locally with an RS256 key
//!   loaded from a PEM file the user supplies (`<config>/hydra-key.pem`). The
//!   key is NOT shipped with grabcli; see `docs/hydra.md`. The fingerprint
//!   claims are opaque strings — the server validates the signature, not their
//!   content — so we generate stable-looking values.
//! - **CDP** ([`crate::cdp`]): drive a logged-in browser over the Chrome
//!   DevTools Protocol, let it mint a token the normal way, and sniff the
//!   header off the wire.
//!
//! The constants below (issuer, audience, version, TTL) describe the token
//! *shape* Grab expects. They are protocol facts, not secrets.

#[cfg(feature = "native-hydra")]
use aws_lc_rs::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
#[cfg(feature = "native-hydra")]
use rustls_pki_types::{PrivateKeyDer, pem::PemObject};
#[cfg(feature = "native-hydra")]
use serde_json::json;

use crate::error::{Error, ErrorKind, Result};
#[cfg(feature = "native-hydra")]
use crate::jwt::b64url;
use crate::jwt::now;

#[cfg(feature = "native-hydra")]
const ISS: &str = "gaa";
#[cfg(feature = "native-hydra")]
const AUD: &str = "gfw";
#[cfg(feature = "native-hydra")]
const NAME: &str = "grabtaxi";
/// Hydra web app version claim (`ver`). Bump if Grab rotates it.
pub const DEFAULT_VER: &str = "1.19.0.24";
/// Token lifetime Grab issues (seconds).
#[cfg(feature = "native-hydra")]
const TTL_SECS: i64 = 600;

/// Opaque browser-fingerprint claims. Any stable values are accepted because
/// the server checks the signature, not the fingerprint.
#[derive(Debug, Clone)]
pub struct DeviceFields {
    pub br_id: String,
    pub br_id_v2: String,
    pub br_uid: String,
    pub suspicious: bool,
}

impl DeviceFields {
    /// Generate random, plausible-looking fingerprint fields.
    pub fn random() -> Self {
        Self {
            br_id: format!("{}y6r", hex(17)),
            br_id_v2: format!("{}y6r", hex(17)),
            br_uid: uuid_v4(),
            suspicious: false,
        }
    }
}

fn rand_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    // getrandom can fail only on exotic platforms; fall back to time-seeded
    // bytes so minting still produces a (less random) value rather than panic.
    if getrandom::fill(&mut buf).is_err() {
        let seed = now() as u64 ^ (std::process::id() as u64);
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(i as u64)
                >> 33) as u8;
        }
    }
    buf
}

fn hex(n: usize) -> String {
    rand_bytes(n.div_ceil(2))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>()
        .chars()
        .take(n)
        .collect()
}

fn uuid_v4() -> String {
    let mut b = rand_bytes(16);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: Vec<String> = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}{}{}{}-{}{}-{}{}-{}{}-{}{}{}{}{}{}",
        h[0],
        h[1],
        h[2],
        h[3],
        h[4],
        h[5],
        h[6],
        h[7],
        h[8],
        h[9],
        h[10],
        h[11],
        h[12],
        h[13],
        h[14],
        h[15]
    )
}

#[cfg(feature = "native-hydra")]
fn load_key(pem: &str) -> Result<RsaKeyPair> {
    let invalid = |detail: String| {
        Error::new(
            ErrorKind::HydraExpired,
            format!("could not read hydra signing key: {detail}"),
        )
    };
    let der = PrivateKeyDer::from_pem_slice(pem.as_bytes()).map_err(|e| invalid(e.to_string()))?;
    match der {
        PrivateKeyDer::Pkcs1(key) => RsaKeyPair::from_der(key.secret_pkcs1_der()),
        PrivateKeyDer::Pkcs8(key) => RsaKeyPair::from_pkcs8(key.secret_pkcs8_der()),
        _ => {
            return Err(invalid(
                "expected an RSA PKCS#1 or PKCS#8 private key".into(),
            ));
        }
    }
    .map_err(|e| invalid(e.to_string()))
}

/// Mint a hydra JWT natively by signing claims with the given RS256 key (PEM).
/// Returns `(token, exp)`.
#[cfg(feature = "native-hydra")]
pub fn mint_native(key_pem: &str, fields: &DeviceFields, ver: &str) -> Result<(String, i64)> {
    let key = load_key(key_pem)?;

    let iat = now();
    let exp = iat + TTL_SECS;
    let header = json!({"alg": "RS256", "typ": "JWT"});
    let claims = json!({
        "iss": ISS,
        "aud": AUD,
        "name": NAME,
        "iat": iat,
        "exp": exp,
        "nbf": iat,
        "ver": ver,
        "brID": fields.br_id,
        "sus": fields.suspicious,
        "brIDv2": fields.br_id_v2,
        "brUID": fields.br_uid,
    });

    let signing_input = format!(
        "{}.{}",
        b64url(&serde_json::to_vec(&header)?),
        b64url(&serde_json::to_vec(&claims)?)
    );
    let mut signature = vec![0; key.public_modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        signing_input.as_bytes(),
        &mut signature,
    )
    .map_err(|_| Error::new(ErrorKind::HydraExpired, "could not sign hydra JWT"))?;
    let token = format!("{signing_input}.{}", b64url(&signature));
    Ok((token, exp))
}

/// Native minting is compiled out: grabcli-core was built without the
/// `native-hydra` feature (no native signer). Always fails; mint over CDP instead.
#[cfg(not(feature = "native-hydra"))]
pub fn mint_native(_key_pem: &str, _fields: &DeviceFields, _ver: &str) -> Result<(String, i64)> {
    Err(Error::new(
        ErrorKind::HydraExpired,
        "native hydra minting is not built in (grabcli-core without the \
         `native-hydra` feature); set hydra_mode = \"cdp\"",
    ))
}

#[cfg(all(test, feature = "native-hydra"))]
pub(crate) mod tests {
    use super::*;
    use aws_lc_rs::{
        encoding::{AsDer, Pkcs8V1Der},
        rsa::KeySize,
        signature::{KeyPair, RSA_PKCS1_2048_8192_SHA256, UnparsedPublicKey},
    };

    // A throwaway key generated in-test; never a production key.
    pub(crate) fn test_key() -> (String, RsaKeyPair) {
        use base64::Engine as _;
        let key = RsaKeyPair::generate(KeySize::Rsa2048).expect("keygen");
        let der: Pkcs8V1Der<'static> = key.as_der().expect("DER");
        let body = base64::engine::general_purpose::STANDARD.encode(der.as_ref());
        let pem = format!("-----BEGIN PRIVATE KEY-----\n{body}\n-----END PRIVATE KEY-----\n");
        (pem, key)
    }

    #[test]
    fn accepts_pkcs1_and_pkcs8_pem_keys() {
        use base64::Engine as _;
        let (pkcs8_pem, key) = test_key();
        let der: Pkcs8V1Der<'static> = key.as_der().expect("DER");
        let info = pkcs8::PrivateKeyInfo::try_from(der.as_ref()).expect("PKCS#8");
        let body = base64::engine::general_purpose::STANDARD.encode(info.private_key);
        let pkcs1_pem =
            format!("-----BEGIN RSA PRIVATE KEY-----\n{body}\n-----END RSA PRIVATE KEY-----\n");
        assert!(load_key(&pkcs8_pem).is_ok());
        assert!(load_key(&pkcs1_pem).is_ok());
    }

    #[test]
    fn mints_a_verifiable_rs256_jwt() -> Result<()> {
        let (pem, key) = test_key();
        let fields = DeviceFields::random();
        let (token, exp) = mint_native(&pem, &fields, DEFAULT_VER)?;

        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);

        // Claims are what Grab expects.
        let claims = crate::jwt::payload(&token)?;
        assert_eq!(claims["iss"], "gaa");
        assert_eq!(claims["aud"], "gfw");
        assert_eq!(claims["name"], "grabtaxi");
        assert_eq!(claims["ver"], DEFAULT_VER);
        assert_eq!(claims["sus"], false);
        assert_eq!(exp - claims["iat"].as_i64().unwrap_or(0), 600);

        // Signature verifies against the public half.
        let vk = UnparsedPublicKey::new(&RSA_PKCS1_2048_8192_SHA256, key.public_key().as_ref());
        let signing_input = format!("{}.{}", parts[0], parts[1]);
        let sig_bytes =
            base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, parts[2])
                .expect("sig b64");
        assert!(vk.verify(signing_input.as_bytes(), &sig_bytes).is_ok());
        Ok(())
    }

    #[test]
    fn device_fields_look_right() {
        let f = DeviceFields::random();
        assert_eq!(f.br_id.len(), 20);
        assert!(f.br_uid.contains('-'));
        assert_eq!(f.br_uid.len(), 36);
    }

    #[test]
    fn bad_key_is_hydra_error() {
        let e = mint_native("not a pem", &DeviceFields::random(), DEFAULT_VER)
            .expect_err("should error");
        assert_eq!(e.kind, ErrorKind::HydraExpired);
    }
}
