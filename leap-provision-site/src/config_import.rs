//! Local configuration parsing and QR decoding. Errors never include payload values.
use leap_api::types::{DownloaderConfig, LeapConfig, S3Config};
use secrecy::ExposeSecret;

pub const MAX_CONFIG_BYTES: usize = 64 * 1024;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Bundle {
    version: u32,
    downloader_config: DownloaderConfig,
    s3_config: S3Config,
}

pub fn parse_config(text: &str, is_toml: bool) -> Result<LeapConfig, &'static str> {
    if text.len() > MAX_CONFIG_BYTES {
        return Err("Configuration exceeds the 64 KiB limit.");
    }
    let bundle: Bundle = if is_toml {
        toml::from_str(text)
            .map_err(|_| "Invalid TOML configuration. Check the documented fields and types.")?
    } else {
        serde_json::from_str(text)
            .map_err(|_| "Invalid JSON configuration. Check the documented fields and types.")?
    };
    if bundle.version != 1 {
        return Err("Unsupported configuration version. Expected version 1.");
    }
    let d = &bundle.downloader_config;
    if d.concurrent_downloads == 0 {
        return Err("Concurrent downloads must be positive.");
    }
    if !d.retry_params.backoff_factor.is_finite() || d.retry_params.backoff_factor <= 1.0 {
        return Err("Backoff factor must be finite and greater than 1.");
    }
    if bundle.s3_config.bucket.scheme_str() != Some("s3")
        || bundle.s3_config.bucket.host().is_none()
    {
        return Err("Bucket must be an s3:// URI with a bucket name.");
    }
    if bundle.s3_config.access_key_id.expose_secret().is_empty()
        || bundle
            .s3_config
            .secret_access_key
            .expose_secret()
            .is_empty()
    {
        return Err("Access key ID and secret access key are required.");
    }
    Ok(LeapConfig {
        downloader_config: bundle.downloader_config,
        s3_config: bundle.s3_config,
    })
}

pub fn decode_qr(width: usize, height: usize, gray: &[u8]) -> Result<LeapConfig, &'static str> {
    let mut decoder = quircs::Quirc::default();
    let mut payloads = Vec::new();
    for code in decoder.identify(width, height, gray).flatten() {
        if let Ok(data) = code.decode() {
            payloads.push(data.payload);
        }
    }
    if payloads.len() > 1 {
        return Err(
            "Multiple QR codes found. Use an image containing only the configuration code.",
        );
    }
    let payload = payloads
        .pop()
        .ok_or("No readable QR code found. Try a clearer, closer image.")?;
    let text =
        std::str::from_utf8(&payload).map_err(|_| "QR configuration must contain UTF-8 JSON.")?;
    parse_config(text, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    const JSON: &str = include_str!("../example-config.json");

    #[test]
    fn imports_json_and_equivalent_toml() {
        let config = parse_config(JSON, false).unwrap();
        assert_eq!(config.downloader_config.concurrent_downloads, 4);
        assert_eq!(config.downloader_config.update_interval.as_secs(), 3600);
        assert_eq!(
            config.s3_config.secret_access_key.expose_secret(),
            "REPLACE_WITH_SECRET_ACCESS_KEY"
        );
        let mut value = serde_json::to_value(&config).unwrap();
        value["version"] = 1.into();
        let toml = toml::to_string(&value).unwrap();
        let imported = parse_config(&toml, true).unwrap();
        assert_eq!(imported.s3_config.bucket, config.s3_config.bucket);
        assert_eq!(
            imported
                .downloader_config
                .retry_params
                .initial_backoff
                .as_secs(),
            1
        );
    }

    #[test]
    fn rejects_invalid_fields_without_leaking_secrets() {
        for (old, new) in [
            ("\"version\": 1", "\"version\": 2"),
            ("\"concurrent_downloads\": 4", "\"concurrent_downloads\": 0"),
            ("\"backoff_factor\": 2.0", "\"backoff_factor\": 1.0"),
            ("s3://school-content", "https://school-content"),
            ("\"1h\"", "\"not-a-duration\""),
        ] {
            let error = parse_config(&JSON.replace(old, new), false).unwrap_err();
            assert!(!error.contains("REPLACE_WITH_SECRET"));
        }
        assert!(parse_config(&"x".repeat(MAX_CONFIG_BYTES + 1), false).is_err());
    }

    #[test]
    fn imports_toml_with_optional_fields_omitted() {
        let config = parse_config(
            r#"
version = 1
[downloader_config]
concurrent_downloads = 7
update_interval = "23m"
[downloader_config.retry_params]
initial_backoff = "250ms"
backoff_factor = 1.5
max_backoff = "3m"
[s3_config]
bucket = "s3://another-bucket"
access_key_id = "test-id"
secret_access_key = "test-secret"
"#,
            true,
        )
        .unwrap();
        assert_eq!(config.downloader_config.concurrent_downloads, 7);
        assert_eq!(config.downloader_config.update_interval.as_secs(), 1380);
        assert_eq!(
            config
                .downloader_config
                .retry_params
                .initial_backoff
                .as_millis(),
            250
        );
        assert_eq!(
            config.downloader_config.retry_params.max_backoff.as_secs(),
            180
        );
        assert_eq!(config.s3_config.region, None);
        assert_eq!(config.s3_config.force_path_style, None);
    }

    #[test]
    fn decodes_real_qr_pixels() {
        let qr = qrcode::QrCode::new(JSON.as_bytes()).unwrap();
        let size = (qr.width() + 8) * 4;
        let mut pixels = vec![255; size * size];
        for y in 0..qr.width() {
            for x in 0..qr.width() {
                if qr[(x, y)] == qrcode::Color::Dark {
                    for dy in 0..4 {
                        for dx in 0..4 {
                            pixels[((y + 4) * 4 + dy) * size + (x + 4) * 4 + dx] = 0;
                        }
                    }
                }
            }
        }
        let config = decode_qr(size, size, &pixels).unwrap();
        assert_eq!(config.s3_config.bucket.host(), Some("school-content"));
        assert!(decode_qr(size, size, &vec![255; size * size]).is_err());
        let mut pair = Vec::with_capacity(size * size * 2);
        for row in pixels.chunks_exact(size) {
            pair.extend_from_slice(row);
            pair.extend_from_slice(row);
        }
        assert_eq!(
            decode_qr(size * 2, size, &pair).unwrap_err(),
            "Multiple QR codes found. Use an image containing only the configuration code."
        );
    }
}
