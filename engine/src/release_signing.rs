use std::fmt;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    BadPublicKey(String),

    BadSignature(String),

    Io(String),

    InvalidSignature,
}

impl fmt::Display for VerifyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VerifyError::BadPublicKey(e) => write!(f, "bad release public key: {e}"),
            VerifyError::BadSignature(e) => write!(f, "bad signature file: {e}"),
            VerifyError::Io(e) => write!(f, "io error: {e}"),
            VerifyError::InvalidSignature => {
                write!(f, "INVALID signature - do not install this artifact")
            }
        }
    }
}

impl std::error::Error for VerifyError {}

fn decode_base64(s: &str) -> Result<Vec<u8>, String> {
    fn val(c: u8) -> Result<u8, String> {
        match c {
            b'A'..=b'Z' => Ok(c - b'A'),
            b'a'..=b'z' => Ok(c - b'a' + 26),
            b'0'..=b'9' => Ok(c - b'0' + 52),
            b'+' => Ok(62),
            b'/' => Ok(63),
            _ => Err(format!("invalid base64 char: {}", c as char)),
        }
    }
    let bytes: Vec<u8> = s.bytes().filter(|&b| b != b'\n' && b != b'\r').collect();
    if !bytes.len().is_multiple_of(4) {
        return Err("base64 length not a multiple of 4".into());
    }
    let mut out = Vec::with_capacity(bytes.len() * 3 / 4);
    for chunk in bytes.chunks(4) {
        let mut n: u32 = 0;
        let mut pad = 0;
        for (i, &c) in chunk.iter().enumerate() {
            if c == b'=' {
                pad += 1;
            } else {
                if pad > 0 {
                    return Err("padding in the middle of base64".into());
                }
                n |= (val(c)? as u32) << (18 - 6 * i);
            }
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Ok(out)
}

pub fn verify_bytes(
    public_key_b64: &str,
    file_bytes: &[u8],
    signature_content: &str,
) -> Result<(), VerifyError> {
    verify_bytes_with_comment(public_key_b64, file_bytes, signature_content).map(|_| ())
}

pub fn verify_bytes_with_comment(
    public_key_b64: &str,
    file_bytes: &[u8],
    signature_content: &str,
) -> Result<String, VerifyError> {
    let pk = minisign_verify::PublicKey::from_base64(public_key_b64.trim())
        .map_err(|e| VerifyError::BadPublicKey(e.to_string()))?;
    let sig = minisign_verify::Signature::decode(signature_content)
        .map_err(|e| VerifyError::BadSignature(e.to_string()))?;

    pk.verify(file_bytes, &sig, false)
        .map_err(|_| VerifyError::InvalidSignature)?;
    Ok(sig.trusted_comment().to_string())
}

pub fn verify_file(public_key_b64: &str, path: &Path) -> Result<(), VerifyError> {
    let sig_path = format!("{}.minisig", path.display());
    let file_bytes = fs::read(path).map_err(|e| VerifyError::Io(e.to_string()))?;
    let sig_content = fs::read_to_string(&sig_path).map_err(|e| VerifyError::Io(e.to_string()))?;
    verify_bytes(public_key_b64, &file_bytes, &sig_content)
}

pub fn updater_public_key(tauri_conf_json: &str) -> Option<String> {
    let conf: serde_json::Value = serde_json::from_str(tauri_conf_json).ok()?;
    let wrapped = conf
        .pointer("/plugins/updater/pubkey")?
        .as_str()?
        .trim()
        .to_string();
    if wrapped.is_empty() || wrapped.contains("PLACEHOLDER") {
        return None;
    }
    let pub_file = String::from_utf8(decode_base64(&wrapped).ok()?).ok()?;
    let key_line = pub_file
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with("untrusted comment:"))?
        .to_string();
    minisign_verify::PublicKey::from_base64(&key_line).ok()?;
    Some(key_line)
}

pub fn updater_pubkey_is_placeholder(tauri_conf_json: &str) -> bool {
    updater_public_key(tauri_conf_json).is_none()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateRejection {
    BadVersion(String),

    NotNewer {
        current: String,
        offered: String,
    },

    BadManifest(String),

    BadSignature(String),

    MissingSignedFile,

    VersionNotSigned {
        version: String,
        signed_file: String,
    },

    UrlFileMismatch {
        url_file: String,
        signed_file: String,
    },
}

impl fmt::Display for UpdateRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UpdateRejection::BadVersion(v) => write!(f, "not a semantic version: '{v}'"),
            UpdateRejection::NotNewer { current, offered } => {
                write!(f, "offered version {offered} is not newer than {current}")
            }
            UpdateRejection::BadManifest(e) => write!(f, "bad update manifest: {e}"),
            UpdateRejection::BadSignature(e) => write!(f, "bad update signature: {e}"),
            UpdateRejection::MissingSignedFile => {
                write!(f, "signature trusted comment does not name the signed file")
            }
            UpdateRejection::VersionNotSigned {
                version,
                signed_file,
            } => write!(
                f,
                "manifest offers {version} but the signature covers '{signed_file}'"
            ),
            UpdateRejection::UrlFileMismatch {
                url_file,
                signed_file,
            } => write!(
                f,
                "download '{url_file}' is not the signed file '{signed_file}'"
            ),
        }
    }
}

impl std::error::Error for UpdateRejection {}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SemVer {
    core: [u64; 3],
    pre: Vec<String>,
}

fn parse_semver(v: &str) -> Result<SemVer, UpdateRejection> {
    let bad = || UpdateRejection::BadVersion(v.to_string());
    let t = v.trim();
    let t = t.strip_prefix('v').unwrap_or(t);
    let t = t.split('+').next().unwrap_or("");
    let (core, pre) = match t.split_once('-') {
        Some((c, p)) if !p.is_empty() => (c, p.split('.').map(str::to_string).collect()),
        Some(_) => return Err(bad()),
        None => (t, Vec::new()),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 {
        return Err(bad());
    }
    let mut out = [0u64; 3];
    for (i, p) in parts.iter().enumerate() {
        if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) {
            return Err(bad());
        }
        out[i] = p.parse().map_err(|_| bad())?;
    }
    Ok(SemVer { core: out, pre })
}

fn semver_cmp(a: &SemVer, b: &SemVer) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match a.core.cmp(&b.core) {
        Ordering::Equal => {}
        o => return o,
    }
    match (a.pre.is_empty(), b.pre.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for (x, y) in a.pre.iter().zip(b.pre.iter()) {
        let o = match (x.parse::<u64>(), y.parse::<u64>()) {
            (Ok(n), Ok(m)) => n.cmp(&m),
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            _ => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.pre.len().cmp(&b.pre.len())
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            let hex = |c: u8| (c as char).to_digit(16);
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn signed_file_name(signature_b64: &str) -> Result<String, UpdateRejection> {
    let raw = decode_base64(signature_b64.trim()).map_err(UpdateRejection::BadSignature)?;
    let text = String::from_utf8(raw)
        .map_err(|_| UpdateRejection::BadSignature("signature is not UTF-8".into()))?;
    let sig = minisign_verify::Signature::decode(&text)
        .map_err(|e| UpdateRejection::BadSignature(e.to_string()))?;
    sig.trusted_comment()
        .split('\t')
        .find_map(|f| f.trim().strip_prefix("file:"))
        .map(|f| f.trim().to_string())
        .filter(|f| !f.is_empty())
        .ok_or(UpdateRejection::MissingSignedFile)
}

pub fn check_update_offer(
    current_version: &str,
    offered_version: &str,
    download_url: &str,
    signature_b64: &str,
) -> Result<(), UpdateRejection> {
    let current = parse_semver(current_version)?;
    let offered = parse_semver(offered_version)?;
    if semver_cmp(&offered, &current) != std::cmp::Ordering::Greater {
        return Err(UpdateRejection::NotNewer {
            current: current_version.to_string(),
            offered: offered_version.to_string(),
        });
    }
    let signed_file = signed_file_name(signature_b64)?;
    let version_token = offered_version.trim().trim_start_matches('v');
    let carries_version = signed_file
        .split('_')
        .any(|tok| tok.trim_start_matches('v') == version_token);
    if !carries_version {
        return Err(UpdateRejection::VersionNotSigned {
            version: offered_version.to_string(),
            signed_file,
        });
    }
    let path = download_url.split(['?', '#']).next().unwrap_or("");
    let url_file = percent_decode(path.rsplit('/').next().unwrap_or(""));
    if url_file != signed_file {
        return Err(UpdateRejection::UrlFileMismatch {
            url_file,
            signed_file,
        });
    }
    Ok(())
}

pub fn check_update_manifest(
    current_version: &str,
    manifest_json: &str,
    target: &str,
) -> Result<(), UpdateRejection> {
    let m: serde_json::Value = serde_json::from_str(manifest_json)
        .map_err(|e| UpdateRejection::BadManifest(e.to_string()))?;
    let version = m
        .get("version")
        .and_then(|v| v.as_str())
        .ok_or_else(|| UpdateRejection::BadManifest("missing version".into()))?;
    let entry = m
        .pointer(&format!("/platforms/{target}"))
        .ok_or_else(|| UpdateRejection::BadManifest(format!("no platform '{target}'")))?;
    let field = |k: &str| {
        entry
            .get(k)
            .and_then(|v| v.as_str())
            .ok_or_else(|| UpdateRejection::BadManifest(format!("missing platforms.{target}.{k}")))
    };
    check_update_offer(current_version, version, field("url")?, field("signature")?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const TEST_PUBKEY: &str = "RWQdtwrmteKxjKJr7y54fi1dyz9HDRpXUOghMxrKTduBtg3oOgrttbwi";
    const TEST_FILE_BYTES: &[u8] = b"zencode test artifact\n";

    const TEST_MINISIG: &str = "untrusted comment: signature from minisign secret key\nRUQdtwrmteKxjP9ZDdQqgLwUFtfreh/grgQpZ2GYWa02vXxwUAUvqgYN2Q2ibdIaxnXzScZ3Hu0ybLl6Lk0yspaZ1lxMeJ9EAA0=\ntrusted comment: test artifact\nmlfo4rsTacWkJlPODc4zGLY0JJxMUyhPHXbYaKQw5QRvMnUzEF7AafTOYbBhx22sedmMuVp0VmaGYS8YXYKaDA==\n";

    #[test]
    fn valid_signature_verifies() {
        assert!(verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, TEST_MINISIG).is_ok());
    }

    #[test]
    fn tampered_file_fails() {
        let mut tampered = TEST_FILE_BYTES.to_vec();
        tampered[0] = b'X';
        assert_eq!(
            verify_bytes(TEST_PUBKEY, &tampered, TEST_MINISIG),
            Err(VerifyError::InvalidSignature)
        );
    }

    #[test]
    fn wrong_key_fails() {
        let other = "RWTGCiWTINb+iEMIQnVXTJ64+XVBCJj4UUGls0PrzSC0dqttm2vxwZ/0";
        assert_eq!(
            verify_bytes(other, TEST_FILE_BYTES, TEST_MINISIG),
            Err(VerifyError::InvalidSignature)
        );
    }

    #[test]
    fn bad_public_key_rejected() {
        assert!(matches!(
            verify_bytes("not-base64!!!", TEST_FILE_BYTES, TEST_MINISIG),
            Err(VerifyError::BadPublicKey(_))
        ));
    }

    #[test]
    fn malformed_signature_rejected() {
        assert!(matches!(
            verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, "garbage"),
            Err(VerifyError::BadSignature(_))
        ));
        assert!(matches!(
            verify_bytes(TEST_PUBKEY, TEST_FILE_BYTES, "untrusted comment: x\n!!!"),
            Err(VerifyError::BadSignature(_))
        ));
    }

    #[test]
    fn verify_file_roundtrip() {
        let dir = std::env::temp_dir().join(format!("zencode-r15-test-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("artifact.bin");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(TEST_FILE_BYTES).unwrap();

        let sig_path = format!("{}.minisig", path.display());
        fs::write(&sig_path, TEST_MINISIG).unwrap();
        assert!(verify_file(TEST_PUBKEY, &path).is_ok());
        let _ = fs::remove_file(&path);
        let _ = fs::remove_file(&sig_path);
    }

    #[test]
    fn verify_file_missing_signature_errors() {
        let dir =
            std::env::temp_dir().join(format!("zencode-r15-test-nosig-{}", std::process::id()));
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("artifact.bin");
        fs::write(&path, TEST_FILE_BYTES).unwrap();
        let err = verify_file(TEST_PUBKEY, &path).unwrap_err();
        assert!(matches!(err, VerifyError::Io(_)));
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn placeholder_detection() {
        let with_placeholder =
            r#"{"plugins":{"updater":{"pubkey":"PLACEHOLDER-D3-CHECKLIST: generate"}}}"#;
        assert!(updater_pubkey_is_placeholder(with_placeholder));
        assert!(updater_pubkey_is_placeholder(
            r#"{"plugins":{"updater":{"pubkey":""}}}"#
        ));
        assert!(updater_pubkey_is_placeholder(r#"{"plugins":{}}"#));
        assert!(updater_pubkey_is_placeholder("not json"));

        let bare = r#"{"plugins":{"updater":{"pubkey":"RWQdtwrmteKxjKJr7y54fi1dyz9HDRpXUOghMxrKTduBtg3oOgrttbwi"}}}"#;
        assert!(updater_pubkey_is_placeholder(bare));

        let wrapped = r#"{"plugins":{"updater":{"pubkey":"dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXkgOENCMUUyQjVFNjBBQjcxRApSV1FkdHdybXRlS3hqS0pyN3k1NGZpMWR5ejlIRFJwWFVPZ2hNeHJLVGR1QnRnM29PZ3J0dGJ3aQo="}}}"#;
        assert!(!updater_pubkey_is_placeholder(wrapped));
        assert_eq!(updater_public_key(wrapped).as_deref(), Some(TEST_PUBKEY));
    }

    #[test]
    fn trusted_comment_is_returned_only_after_verification() {
        assert_eq!(
            verify_bytes_with_comment(TEST_PUBKEY, TEST_FILE_BYTES, TEST_MINISIG).unwrap(),
            "test artifact"
        );

        let forged =
            TEST_MINISIG.replace("trusted comment: test artifact", "trusted comment: v9.9.9");
        assert_eq!(
            verify_bytes_with_comment(TEST_PUBKEY, TEST_FILE_BYTES, &forged),
            Err(VerifyError::InvalidSignature)
        );
    }

    #[test]
    fn release_key_is_wired() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let conf = fs::read_to_string(root.join("desktop/src-tauri/tauri.conf.json")).unwrap();
        let pub_file = fs::read_to_string(root.join("docs/keys/zencode-release.pub")).unwrap();
        let expected = pub_file.lines().nth(1).unwrap().trim();
        assert_eq!(updater_public_key(&conf).as_deref(), Some(expected));
    }

    fn encode_base64(data: &[u8]) -> String {
        const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        for c in data.chunks(3) {
            let n = (c[0] as u32) << 16
                | (*c.get(1).unwrap_or(&0) as u32) << 8
                | *c.get(2).unwrap_or(&0) as u32;
            out.push(T[(n >> 18) as usize & 63] as char);
            out.push(T[(n >> 12) as usize & 63] as char);
            out.push(if c.len() > 1 {
                T[(n >> 6) as usize & 63] as char
            } else {
                '='
            });
            out.push(if c.len() > 2 {
                T[n as usize & 63] as char
            } else {
                '='
            });
        }
        out
    }

    fn tauri_signature(file: &str) -> String {
        let comment = format!("trusted comment: timestamp:1760745600\tfile:{file}");
        encode_base64(
            TEST_MINISIG
                .replace("trusted comment: test artifact", &comment)
                .as_bytes(),
        )
    }

    fn manifest(version: &str, url: &str, signature: &str) -> String {
        serde_json::json!({
            "version": version,
            "notes": "test",
            "platforms": { "linux-x86_64": { "url": url, "signature": signature } }
        })
        .to_string()
    }

    const URL_031: &str = "https://gitlab.com/7enlab/zen-desktop/-/releases/v0.3.1/downloads/ZenCode%20Desktop_0.3.1_amd64.AppImage";

    #[test]
    fn update_manifest_with_signed_version_is_accepted() {
        let sig = tauri_signature("ZenCode Desktop_0.3.1_amd64.AppImage");
        assert_eq!(
            signed_file_name(&sig).unwrap(),
            "ZenCode Desktop_0.3.1_amd64.AppImage"
        );
        let m = manifest("0.3.1", URL_031, &sig);
        assert_eq!(check_update_manifest("0.3.0", &m, "linux-x86_64"), Ok(()));

        let m = manifest("v0.3.1", URL_031, &sig);
        assert_eq!(check_update_manifest("0.1.0", &m, "linux-x86_64"), Ok(()));
    }

    #[test]
    fn update_manifest_with_mismatched_version_is_rejected() {
        let sig = tauri_signature("ZenCode Desktop_0.3.1_amd64.AppImage");
        let m = manifest("0.3.2", URL_031, &sig);
        assert_eq!(
            check_update_manifest("0.3.0", &m, "linux-x86_64"),
            Err(UpdateRejection::VersionNotSigned {
                version: "0.3.2".into(),
                signed_file: "ZenCode Desktop_0.3.1_amd64.AppImage".into(),
            })
        );

        let sig = tauri_signature("ZenCode Desktop_10.3.1_amd64.AppImage");
        let url = "https://x/ZenCode%20Desktop_10.3.1_amd64.AppImage";
        assert!(matches!(
            check_update_offer("0.3.0", "0.3.1", url, &sig),
            Err(UpdateRejection::VersionNotSigned { .. })
        ));
    }

    #[test]
    fn update_url_must_point_at_the_signed_file() {
        let sig = tauri_signature("ZenCode Desktop_0.3.1_amd64.AppImage");
        let url = "https://gitlab.com/x/-/releases/v0.3.1/downloads/evil_0.3.1_amd64.AppImage";
        assert!(matches!(
            check_update_offer("0.3.0", "0.3.1", url, &sig),
            Err(UpdateRejection::UrlFileMismatch { .. })
        ));
    }

    #[test]
    fn update_must_be_newer() {
        let sig = tauri_signature("ZenCode Desktop_0.3.1_amd64.AppImage");
        for current in ["0.3.1", "0.4.0", "v1.0.0"] {
            assert!(matches!(
                check_update_offer(current, "0.3.1", URL_031, &sig),
                Err(UpdateRejection::NotNewer { .. })
            ));
        }

        let sig = tauri_signature("z_0.3.1-rc.2_x.AppImage");
        let url = "https://x/z_0.3.1-rc.2_x.AppImage";
        assert_eq!(
            check_update_offer("0.3.1-rc.1", "0.3.1-rc.2", url, &sig),
            Ok(())
        );
        assert!(matches!(
            check_update_offer("0.3.1", "0.3.1-rc.2", url, &sig),
            Err(UpdateRejection::NotNewer { .. })
        ));
        assert!(matches!(
            check_update_offer("0.3", "0.3.1", URL_031, &sig),
            Err(UpdateRejection::BadVersion(_))
        ));
    }

    #[test]
    fn update_signature_must_name_a_file() {
        let sig = encode_base64(TEST_MINISIG.as_bytes());
        assert_eq!(
            check_update_offer("0.3.0", "0.3.1", URL_031, &sig),
            Err(UpdateRejection::MissingSignedFile)
        );
        assert!(matches!(
            check_update_offer("0.3.0", "0.3.1", URL_031, "!!not base64!!"),
            Err(UpdateRejection::BadSignature(_))
        ));
        assert!(matches!(
            check_update_manifest("0.3.0", "{\"version\":\"0.3.1\"}", "linux-x86_64"),
            Err(UpdateRejection::BadManifest(_))
        ));
    }

    const FIXTURE_PUBKEY: &str = "RWRaSFNcTx6H8ZYC0RLy36GJrvXrYulwmLlKBha1lbiHmm5NTpdKidsO";
    const FIXTURE_BYTES: &[u8] = b"zencode updater fixture\n";
    const FIXTURE_SIG_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVSYVNGTmNUeDZIOGRjZ3ZtQTJYMG9wWWhuZFdsanZMdTBEcXpCWkxyOTVpR05wMVZBYUM1d3huTVp6YUN5SmJDTmY3TlJSWEYwZHR4cUFDRTZ5MWVSdGFlenJRemoxNVFJPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyMzEyMTk3CWZpbGU6emVuY29kZS1kZXNrdG9wXzAuMy4xX2FtZDY0LkFwcEltYWdlCnpJTWlUZ0tUT1BjZEdVcU02aTdocDRRWFV5VkVWN2M4di9TRDlsd2ZsanlPcDVMTGR3elRnVWxmRnh1WVdhSHFraVpxSGNFOGpEYjloemNXSExOcUJ3PT0K";

    #[test]
    fn real_tauri_signature_binds_the_version() {
        let sig_text = String::from_utf8(decode_base64(FIXTURE_SIG_B64).unwrap()).unwrap();
        let comment = verify_bytes_with_comment(FIXTURE_PUBKEY, FIXTURE_BYTES, &sig_text).unwrap();
        assert!(comment.ends_with("file:zencode-desktop_0.3.1_amd64.AppImage"));

        let url = "https://gitlab.com/api/v4/projects/87760189/packages/generic/zencode-desktop/v0.3.1/zencode-desktop_0.3.1_amd64.AppImage";
        let ok = manifest("0.3.1", url, FIXTURE_SIG_B64);
        assert_eq!(check_update_manifest("0.3.0", &ok, "linux-x86_64"), Ok(()));
        let relabeled = manifest("0.3.2", url, FIXTURE_SIG_B64);
        assert!(matches!(
            check_update_manifest("0.3.0", &relabeled, "linux-x86_64"),
            Err(UpdateRejection::VersionNotSigned { .. })
        ));
    }

    #[test]
    fn updater_uses_only_the_gitlab_endpoint() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let conf = fs::read_to_string(root.join("desktop/src-tauri/tauri.conf.json")).unwrap();
        let conf: serde_json::Value = serde_json::from_str(&conf).unwrap();
        let endpoints = conf
            .pointer("/plugins/updater/endpoints")
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(endpoints.len(), 1);
        assert!(endpoints[0]
            .as_str()
            .unwrap()
            .starts_with("https://gitlab.com/7enlab/zen-desktop/"));
    }

    #[test]
    fn base64_decoder_vectors() {
        assert_eq!(decode_base64("").unwrap(), b"");
        assert_eq!(decode_base64("Zg==").unwrap(), b"f");
        assert_eq!(decode_base64("Zm8=").unwrap(), b"fo");
        assert_eq!(decode_base64("Zm9v").unwrap(), b"foo");
        assert!(decode_base64("!").is_err());
        assert!(decode_base64("abc").is_err());
    }
}
