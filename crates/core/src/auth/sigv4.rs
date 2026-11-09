//! AWS Signature Version 4 request signing (header form).
//!
//! Pure: the time is an argument. Chunked/streaming signatures and SigV4A are not supported.

use hmac::{Hmac, Mac};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use sha2::{Digest as _, Sha256};

use super::hex;

/// Payload hash for a body that is streamed rather than held in memory.
pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";

/// Credentials and scope for one signature. `session_token` empty means none.
pub struct Credentials<'a> {
    pub access_key: &'a str,
    pub secret_key: &'a str,
    pub session_token: &'a str,
    pub region: &'a str,
    pub service: &'a str,
}

/// The result of signing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signed {
    pub canonical_request: String,
    pub string_to_sign: String,
    /// The `Authorization` header value.
    pub authorization: String,
    /// Headers to add to the request, `Authorization` included.
    pub added: Vec<(String, String)>,
}

/// Everything but RFC 3986 unreserved characters is encoded.
const AWS_ENCODE: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'_')
    .remove(b'.')
    .remove(b'~');

/// SHA-256 of `data` as lower-case hex.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    // HMAC accepts keys of any length, so this cannot fail.
    let mut mac = Hmac::<Sha256>::new_from_slice(key).expect("hmac accepts any key length");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// `(YYYYMMDD, YYYYMMDDTHHMMSSZ)` for a Unix time in seconds, UTC.
pub fn amz_date(unix_secs: u64) -> (String, String) {
    let days = i64::try_from(unix_secs / 86_400).unwrap_or(0);
    let rem = unix_secs % 86_400;
    // Civil-from-days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let date = format!("{year:04}{month:02}{day:02}");
    let stamp = format!(
        "{date}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    );
    (date, stamp)
}

fn canonical_path(url: &reqwest::Url, service: &str) -> String {
    let path = url.path();
    if path.is_empty() {
        return "/".to_owned();
    }
    if service == "s3" {
        // S3 signs the path as sent, encoded once.
        return path.to_owned();
    }
    // Other services encode each (already encoded) segment a second time.
    path.split('/')
        .map(|seg| utf8_percent_encode(seg, AWS_ENCODE).to_string())
        .collect::<Vec<_>>()
        .join("/")
}

fn canonical_query(url: &reqwest::Url) -> String {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .map(|(k, v)| {
            (
                utf8_percent_encode(&k, AWS_ENCODE).to_string(),
                utf8_percent_encode(&v, AWS_ENCODE).to_string(),
            )
        })
        .collect();
    pairs.sort();
    pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&")
}

/// Trims and collapses runs of spaces and tabs to one space.
fn normalize_value(v: &str) -> String {
    v.split([' ', '\t'])
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Signs a request. `headers` are the headers of the prepared request, without
/// `Authorization`, `x-amz-date` or the headers this function adds. `payload_hash` is the
/// lower-case hex SHA-256 of the body, or [`UNSIGNED_PAYLOAD`].
pub fn sign(
    method: &str,
    url: &str,
    headers: &[(String, String)],
    payload_hash: &str,
    creds: &Credentials<'_>,
    unix_secs: u64,
) -> Result<Signed, String> {
    let url = reqwest::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    let (date, stamp) = amz_date(unix_secs);
    let s3 = creds.service == "s3";
    let has_token = !creds.session_token.is_empty();

    let mut added: Vec<(String, String)> = vec![("x-amz-date".into(), stamp.clone())];
    if has_token {
        added.push(("x-amz-security-token".into(), creds.session_token.into()));
    }
    if s3 {
        added.push(("x-amz-content-sha256".into(), payload_hash.into()));
    }

    let ours = |name: &str| added.iter().any(|(n, _)| n == name);
    let mut all: Vec<(String, String)> = headers
        .iter()
        .map(|(n, v)| (n.to_ascii_lowercase(), normalize_value(v)))
        .filter(|(n, _)| n != "authorization" && !ours(n))
        .collect();
    if !all.iter().any(|(n, _)| n == "host") {
        let host = url.host_str().unwrap_or_default();
        let host = match url.port() {
            Some(p) => format!("{host}:{p}"),
            None => host.to_owned(),
        };
        all.push(("host".into(), host));
    }
    all.extend(added.iter().map(|(n, v)| (n.clone(), normalize_value(v))));
    all.sort_by(|a, b| a.0.cmp(&b.0));

    // Same-name headers join with commas, in request order.
    let mut merged: Vec<(String, String)> = Vec::new();
    for (n, v) in all {
        match merged.last_mut() {
            Some((ln, lv)) if *ln == n => {
                lv.push(',');
                lv.push_str(&v);
            }
            _ => merged.push((n, v)),
        }
    }
    let canonical_headers: String = merged.iter().map(|(n, v)| format!("{n}:{v}\n")).collect();
    let signed_headers = merged
        .iter()
        .map(|(n, _)| n.as_str())
        .collect::<Vec<_>>()
        .join(";");

    let canonical_request = format!(
        "{method}\n{}\n{}\n{canonical_headers}\n{signed_headers}\n{payload_hash}",
        canonical_path(&url, creds.service),
        canonical_query(&url),
    );
    let scope = format!("{date}/{}/{}/aws4_request", creds.region, creds.service);
    let string_to_sign = format!(
        "AWS4-HMAC-SHA256\n{stamp}\n{scope}\n{}",
        sha256_hex(canonical_request.as_bytes())
    );

    let k_date = hmac(
        format!("AWS4{}", creds.secret_key).as_bytes(),
        date.as_bytes(),
    );
    let k_region = hmac(&k_date, creds.region.as_bytes());
    let k_service = hmac(&k_region, creds.service.as_bytes());
    let k_signing = hmac(&k_service, b"aws4_request");
    let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));

    let authorization = format!(
        "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed_headers}, \
         Signature={signature}",
        creds.access_key
    );
    added.push(("authorization".into(), authorization.clone()));
    Ok(Signed {
        canonical_request,
        string_to_sign,
        authorization,
        added,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Vectors: the AWS Signature Version 4 test suite
    // (https://github.com/awslabs/aws-c-auth/tree/main/tests/aws-signing-test-suite/v4,
    // the `header-*` files of each case). Every case: access key AKIDEXAMPLE, secret
    // wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY, region us-east-1, service `service`,
    // time 2015-08-30T12:36:00Z.

    const TIME: u64 = 1_440_938_160;
    const EMPTY_HASH: &str = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";

    fn creds(token: &str) -> Credentials<'_> {
        Credentials {
            access_key: "AKIDEXAMPLE",
            secret_key: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY",
            session_token: token,
            region: "us-east-1",
            service: "service",
        }
    }

    fn h(n: &str, v: &str) -> (String, String) {
        (n.to_owned(), v.to_owned())
    }

    fn check(
        method: &str,
        url: &str,
        headers: &[(String, String)],
        payload_hash: &str,
        token: &str,
        canonical: &str,
        sts: &str,
        authorization: &str,
    ) {
        let s = sign(method, url, headers, payload_hash, &creds(token), TIME).unwrap();
        assert_eq!(s.canonical_request, canonical);
        assert_eq!(s.string_to_sign, sts);
        assert_eq!(s.authorization, authorization);
    }

    #[test]
    fn date_formatting() {
        assert_eq!(
            amz_date(TIME),
            ("20150830".into(), "20150830T123600Z".into())
        );
        assert_eq!(amz_date(0).1, "19700101T000000Z");
        // 2024-02-29T23:59:59Z, a leap day.
        assert_eq!(amz_date(1_709_251_199).1, "20240229T235959Z");
    }

    #[test]
    fn get_vanilla() {
        check(
            "GET",
            "http://example.amazonaws.com/",
            &[h("Host", "example.amazonaws.com")],
            EMPTY_HASH,
            "",
            "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\n\
             host;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             bb579772317eb040ac9ed261061d46c1f17a8133879d6129b6e1c25292927e63",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31",
        );
    }

    #[test]
    fn get_vanilla_query_order_key_case() {
        // Host comes from the URL here rather than a header.
        check(
            "GET",
            "http://example.amazonaws.com/?Param2=value2&Param1=value1",
            &[],
            EMPTY_HASH,
            "",
            "GET\n/\nParam1=value1&Param2=value2\nhost:example.amazonaws.com\n\
             x-amz-date:20150830T123600Z\n\nhost;x-amz-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             816cd5b414d056048ba4f7c5386d6e0533120fb1fcfa93762cf0fc39e2cf19e0",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=b97d918cfa904a5beff61c982a1b6f458b799221646efd99d3219ec94cdf2500",
        );
    }

    #[test]
    fn post_vanilla() {
        check(
            "POST",
            "http://example.amazonaws.com/",
            &[h("Host", "example.amazonaws.com")],
            EMPTY_HASH,
            "",
            "POST\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\n\
             host;x-amz-date\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             553f88c9e4d10fc9e109e2aeb65f030801b70c2f6468faca261d401ae622fc87",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, \
             Signature=5da7c1a2acd57cee7505fc6676e4e544621c30862966e37dddb68e92efbe5d6b",
        );
    }

    #[test]
    fn post_x_www_form_urlencoded() {
        let body_hash = "9095672bbd1f56dfc5b65f3e153adc8731a4a654192329106275f4c7b24d0b6e";
        check(
            "POST",
            "http://example.amazonaws.com/",
            &[
                h("Content-Type", "application/x-www-form-urlencoded"),
                h("Host", "example.amazonaws.com"),
                h("Content-Length", "13"),
                // The suite signs the body hash header for this case.
                h("x-amz-content-sha256", body_hash),
            ],
            body_hash,
            "",
            "POST\n/\n\ncontent-length:13\n\
             content-type:application/x-www-form-urlencoded\n\
             host:example.amazonaws.com\n\
             x-amz-content-sha256:9095672bbd1f56dfc5b65f3e153adc8731a4a654192329106275f4c7b24d0b6e\n\
             x-amz-date:20150830T123600Z\n\n\
             content-length;content-type;host;x-amz-content-sha256;x-amz-date\n\
             9095672bbd1f56dfc5b65f3e153adc8731a4a654192329106275f4c7b24d0b6e",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             b1edd1d03544c25390e32085d55b57acc9a3961bb59415ff86c45c3d89d16cfb",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=content-length;content-type;host;x-amz-content-sha256;x-amz-date, \
             Signature=d3875051da38690788ef43de4db0d8f280229d82040bfac253562e56c3f20e0b",
        );
    }

    #[test]
    fn get_header_value_trim() {
        check(
            "GET",
            "http://example.amazonaws.com/",
            &[
                h("Host", "example.amazonaws.com"),
                h("My-Header1", " value1"),
                h("My-Header2", "\"a   b   c\""),
            ],
            EMPTY_HASH,
            "",
            "GET\n/\n\nhost:example.amazonaws.com\nmy-header1:value1\n\
             my-header2:\"a b c\"\nx-amz-date:20150830T123600Z\n\n\
             host;my-header1;my-header2;x-amz-date\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             a726db9b0df21c14f559d0a978e563112acb1b9e05476f0a6a1c7d68f28605c7",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;my-header1;my-header2;x-amz-date, \
             Signature=acc3ed3afb60bb290fc8d2dd0098b9911fcaa05412b367055dee359757a9c736",
        );
    }

    #[test]
    fn get_vanilla_with_session_token() {
        let token = "6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267";
        check(
            "GET",
            "http://example.amazonaws.com/",
            &[h("Host", "example.amazonaws.com")],
            EMPTY_HASH,
            token,
            "GET\n/\n\nhost:example.amazonaws.com\nx-amz-date:20150830T123600Z\n\
             x-amz-security-token:6e86291e8372ff2a2260956d9b8aae1d763fbf315fa00fa31553b73ebf194267\n\n\
             host;x-amz-date;x-amz-security-token\n\
             e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "AWS4-HMAC-SHA256\n20150830T123600Z\n20150830/us-east-1/service/aws4_request\n\
             067b36aa60031588cea4a4cde1f21215227a047690c72247f1d70b32fbbfad2b",
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date;x-amz-security-token, \
             Signature=07ec1639c89043aa0e3e2de82b96708f198cceab042d4a97044c66dd9f74e7f8",
        );
    }

    #[test]
    fn s3_adds_the_content_hash_header_and_added_headers_are_returned() {
        let c = Credentials {
            service: "s3",
            ..creds("tok")
        };
        let s = sign(
            "PUT",
            "http://example.amazonaws.com/b/k",
            &[],
            UNSIGNED_PAYLOAD,
            &c,
            TIME,
        )
        .unwrap();
        let names: Vec<&str> = s.added.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "x-amz-date",
                "x-amz-security-token",
                "x-amz-content-sha256",
                "authorization"
            ]
        );
        assert!(s.canonical_request.ends_with("\nUNSIGNED-PAYLOAD"));
    }

    #[test]
    fn non_s3_paths_are_encoded_twice_and_s3_once() {
        let url = "http://example.amazonaws.com/a%20b";
        let other = sign("GET", url, &[], EMPTY_HASH, &creds(""), TIME).unwrap();
        assert!(other.canonical_request.starts_with("GET\n/a%2520b\n"));
        let c = Credentials {
            service: "s3",
            ..creds("")
        };
        let s3 = sign("GET", url, &[], EMPTY_HASH, &c, TIME).unwrap();
        assert!(s3.canonical_request.starts_with("GET\n/a%20b\n"));
    }
}
