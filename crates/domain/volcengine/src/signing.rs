use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use time::{OffsetDateTime, macros::format_description};

use crate::Credentials;

pub(crate) const HOST: &str = "ark.cn-beijing.volces.com";
const REGION: &str = "cn-beijing";
const SERVICE: &str = "ark";
const VERSION: &str = "2024-01-01";
const SIGNED_HEADERS: &str = "host;x-content-sha256;x-date";

type HmacSha256 = Hmac<Sha256>;

pub(crate) struct SignedHeaders {
    pub authorization: String,
    pub payload_hash: String,
}

pub(crate) fn x_date() -> Result<String, time::error::Format> {
    OffsetDateTime::now_utc().format(format_description!(
        "[year][month][day]T[hour][minute][second]Z"
    ))
}

pub(crate) fn sign(
    action: &str,
    body: &[u8],
    credentials: Credentials<'_>,
    x_date: &str,
) -> Option<SignedHeaders> {
    let bytes = x_date.as_bytes();
    if bytes.len() != 16
        || bytes[8] != b'T'
        || bytes[15] != b'Z'
        || !bytes[..8]
            .iter()
            .chain(&bytes[9..15])
            .all(u8::is_ascii_digit)
    {
        return None;
    }

    let date = &x_date[..8];
    let query = format!("Action={action}&Version={VERSION}");
    let payload_hash = sha256_hex(body);
    let canonical_headers =
        format!("host:{HOST}\nx-content-sha256:{payload_hash}\nx-date:{x_date}\n");
    let canonical_request =
        format!("POST\n/\n{query}\n{canonical_headers}\n{SIGNED_HEADERS}\n{payload_hash}");
    let credential_scope = format!("{date}/{REGION}/{SERVICE}/request");
    let string_to_sign = format!(
        "HMAC-SHA256\n{x_date}\n{credential_scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let date_key = hmac(credentials.secret_access_key.as_bytes(), date.as_bytes())?;
    let region_key = hmac(&date_key, REGION.as_bytes())?;
    let service_key = hmac(&region_key, SERVICE.as_bytes())?;
    let signing_key = hmac(&service_key, b"request")?;
    let signature = hex(&hmac(&signing_key, string_to_sign.as_bytes())?);
    let authorization = format!(
        "HMAC-SHA256 Credential={}/{credential_scope}, SignedHeaders={SIGNED_HEADERS}, Signature={signature}",
        credentials.access_key_id
    );

    Some(SignedHeaders {
        authorization,
        payload_hash,
    })
}

fn hmac(key: &[u8], value: &[u8]) -> Option<[u8; 32]> {
    let mut mac = HmacSha256::new_from_slice(key).ok()?;
    mac.update(value);
    Some(mac.finalize().into_bytes().into())
}

fn sha256_hex(value: &[u8]) -> String {
    hex(&Sha256::digest(value))
}

fn hex(value: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";

    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value {
        encoded.push(DIGITS[usize::from(byte >> 4)] as char);
        encoded.push(DIGITS[usize::from(byte & 0x0f)] as char);
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_volcengine_signing_vector() {
        let signed = sign(
            "GetSeatInfoUsage",
            br#"{"SeatID":"seat-1","ProjectName":"default"}"#,
            Credentials {
                access_key_id: "AKIDEXAMPLE",
                secret_access_key: "secret",
            },
            "20260810T120000Z",
        )
        .expect("valid date");

        assert_eq!(
            signed.payload_hash,
            "8fd56cd70cd257bbeb841642f44c88e5ac40461762ea43d23b9392a3c852b5de"
        );
        assert_eq!(
            signed.authorization,
            "HMAC-SHA256 Credential=AKIDEXAMPLE/20260810/cn-beijing/ark/request, SignedHeaders=host;x-content-sha256;x-date, Signature=7da2a8b64677ed5df397a1f995cdd071266db7741cda281101f9fb4ecb5814bd"
        );
    }
}
