use leap_api::types::WirelessConfig;
use thiserror::Error;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum WirelessQrError {
    #[error(r#"Missing "WIFI:" prefix"#)]
    MissingPrefix,

    #[error("Missing value for key: {0}")]
    MissingValue(String),

    #[error("Found empty key in WiFi QR code")]
    EmptyKey,

    #[error("Unexpected end of the WiFi QR data")]
    UnexpectedEnd,

    #[error("The QR code contains additional trailing text")]
    TrailingText,

    #[error("Missing WiFi SSID")]
    MissingSSID,

    #[error("Missing password")]
    MissingPassword,

    #[error("Unsupported wireless password type: {0}")]
    UnsupportedPasswordType(String),

    #[error("Unknown wireless password type: {0}")]
    UnknownPasswordType(String),

    #[error("Extra unsupported keys: {0:?}")]
    ExtraUnsupportedKeys(Vec<String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum ParserState {
    Key,
    Value,
    Completed,
}

struct Parser {
    escaped: bool,
    state: ParserState,
    key: String,
    value: String,
    kv_pairs: Vec<(String, String)>,
}

impl Parser {
    fn new() -> Self {
        Self {
            escaped: false,
            state: ParserState::Key,
            value: String::new(),
            key: String::new(),
            kv_pairs: vec![],
        }
    }

    fn feed(&mut self, c: char) -> Result<(), WirelessQrError> {
        match (c, &self.state) {
            (_, ParserState::Completed) => {
                return Err(WirelessQrError::TrailingText);
            }

            (_, _) if self.escaped => {
                self.value.push(c);
                self.escaped = false;
            }
            ('\\', _) => {
                self.escaped = true;
            }

            (':', ParserState::Key) if self.value.is_empty() => {
                return Err(WirelessQrError::EmptyKey);
            }
            (':', ParserState::Key) => {
                self.key = std::mem::take(&mut self.value);
                self.state = ParserState::Value;
            }

            (';', ParserState::Value) => {
                self.kv_pairs.push((
                    std::mem::take(&mut self.key),
                    std::mem::take(&mut self.value),
                ));
                self.state = ParserState::Key;
            }
            (';', ParserState::Key) if self.value.is_empty() => {
                self.state = ParserState::Completed;
            }
            // ':' is omitted here because the original decoder allowed unscaped colons in the values, only looking for a `;`.
            // Original decoder: https://github.com/zxing/zxing/blob/33dfdefcb35576612841e614d44ba9edc9aee2b5/core/src/main/java/com/google/zxing/client/result/WifiResultParser.java#L39
            (';', _) => {
                return Err(WirelessQrError::MissingValue(std::mem::take(
                    &mut self.value,
                )));
            }

            _ => {
                self.value.push(c);
            }
        }

        Ok(())
    }

    fn complete(self) -> Result<Vec<(String, String)>, WirelessQrError> {
        if !self.value.is_empty()
            || !self.key.is_empty()
            || self.state != ParserState::Completed
            || self.escaped
        {
            return Err(WirelessQrError::UnexpectedEnd);
        }
        Ok(self.kv_pairs)
    }
}

pub fn parse_wifi_qr_code_settings(data: &str) -> Result<WirelessConfig, WirelessQrError> {
    let rest = data
        .strip_prefix("WIFI:")
        .ok_or(WirelessQrError::MissingPrefix)?;

    let mut parser = Parser::new();
    for c in rest.chars() {
        parser.feed(c)?;
    }

    let mut kv_pairs = parser.complete()?;

    let mut extract = |tag| {
        kv_pairs
            .extract_if(.., |(k, _)| k == tag)
            .map(|(_, v)| v)
            .next()
    };

    let ssid = extract("S").ok_or(WirelessQrError::MissingSSID)?;
    let password = extract("P");
    let password_type = extract("T");

    // TODO: Make use of the hidden flag
    let _hidden = extract("H");

    let password = match (password_type.as_deref(), password.as_deref()) {
        (Some("nopass") | Some("") | None, _) => String::new(),
        (Some("WPA") | Some("WPA2"), None | Some("")) => {
            return Err(WirelessQrError::MissingPassword);
        }
        (Some("WPA") | Some("WPA2"), Some(p)) => p.to_string(),
        (Some(t @ "WPA2-EAP"), _) => {
            return Err(WirelessQrError::UnsupportedPasswordType(t.to_string()));
        }
        (Some(unkn_method), _) => {
            return Err(WirelessQrError::UnknownPasswordType(
                unkn_method.to_string(),
            ));
        }
    };
    let password = password.into();

    if !kv_pairs.is_empty() {
        return Err(WirelessQrError::ExtraUnsupportedKeys(
            kv_pairs.into_iter().map(|(k, _)| k).collect(),
        ));
    }

    Ok(WirelessConfig {
        ssid,
        password,
        ip_config: leap_api::types::IpConfig::Dhcp,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use googletest::prelude::*;
    use leap_api::types::IpConfig;
    use rstest::rstest;
    use secrecy::ExposeSecret;

    #[gtest]
    fn imports_standard_fields_in_any_order_and_preserves_whitespace() -> googletest::Result<()> {
        let config =
            parse_wifi_qr_code_settings("WIFI:P: pass123 ;H:false;S: School Wi-Fi ;T:WPA;;")
                .unwrap();
        verify_eq!(config.ssid, " School Wi-Fi ")?;
        verify_eq!(config.password.expose_secret(), " pass123 ")?;
        verify_that!(config.ip_config, pat!(IpConfig::Dhcp))?;
        Ok(())
    }

    #[gtest]
    #[rstest]
    #[case::escaped_punctuation(
        r#"WIFI:T:WPA;S:\"foo\;bar\\baz\";P:ab\,cd\:ef\\gh\;ij;;"#,
        "\"foo;bar\\baz\"",
        "ab,cd:ef\\gh;ij"
    )]
    // Even backslashes leave a delimiter; odd ones escape it.
    #[case::backslash_parity(
        r"WIFI:T:WPA;S:school\\;P:password\\\;suffix;;",
        "school\\",
        "password\\;suffix"
    )]
    // Quotes are literal characters, not shell-style grouping or JSON quoting.
    #[case::literal_quotes(r#"WIFI:T:WPA;S:"ABCD";P:"abcdefgh";;"#, "\"ABCD\"", "\"abcdefgh\"")]
    // The original decoder allowed unscaped colons in the values, only looking for a `;`.
    // Original decoder: https://github.com/zxing/zxing/blob/33dfdefcb35576612841e614d44ba9edc9aee2b5/core/src/main/java/com/google/zxing/client/result/WifiResultParser.java#L39
    #[case::raw_colons(
        "WIFI:T:WPA;S:School:West:Lab;P:password:with:colons;;",
        "School:West:Lab",
        "password:with:colons"
    )]
    #[case::escaped_colons(
        r"WIFI:T:WPA;S:School\:West\:Lab;P:password\:with\:colons;;",
        "School:West:Lab",
        "password:with:colons"
    )]
    // Escapes are literal even outside the usual punctuation set.
    #[case::escaped_letter(r"WIFI:S:scho\ol;T:nopass;;", "school", "")]
    fn decodes_field_values(
        #[case] code: &str,
        #[case] ssid: &str,
        #[case] password: &str,
    ) -> googletest::Result<()> {
        let config = parse_wifi_qr_code_settings(code)?;
        verify_eq!(config.ssid, ssid)?;
        verify_eq!(config.password.expose_secret(), password)?;
        verify_that!(config.ip_config, pat!(IpConfig::Dhcp))?;
        Ok(())
    }

    #[rstest]
    #[case::explicit_open("WIFI:T:nopass;S:school;;")]
    #[case::missing_security("WIFI:S:school;;")]
    #[case::empty_security("WIFI:T:;S:school;;")]
    #[case::ignored_password("WIFI:T:nopass;S:school;P:ignored;;")]
    fn imports_open_networks(#[case] code: &str) -> googletest::Result<()> {
        let config = parse_wifi_qr_code_settings(code)?;
        verify_eq!(config.ssid, "school")?;
        verify_eq!(config.password.expose_secret(), "")?;
        verify_that!(config.ip_config, pat!(IpConfig::Dhcp))?;
        Ok(())
    }

    #[rstest]
    #[case::wep(
        "WIFI:T:WEP;S:school;P:secret123;;",
        WirelessQrError::UnknownPasswordType("WEP".to_string())
    )]
    #[case::enterprise(
        "WIFI:T:WPA2-EAP;S:school;I:user;P:secret123;;",
        WirelessQrError::UnsupportedPasswordType("WPA2-EAP".to_string())
    )]
    #[case::unsupported_eap_field(
        "WIFI:T:WPA;S:school;P:secret123;E:TLS;;",
        WirelessQrError::ExtraUnsupportedKeys(vec!["E".to_string()]),
    )]
    #[case::missing_wpa_password("WIFI:T:WPA;S:school;;", WirelessQrError::MissingPassword)]
    #[case::empty_wpa_password("WIFI:T:WPA2;S:school;P:;;", WirelessQrError::MissingPassword)]
    #[case::duplicate_ssid(
        "WIFI:T:WPA;S:first;S:second;P:password;;",
        WirelessQrError::ExtraUnsupportedKeys(vec!["S".to_owned()])
    )]
    #[case::duplicate_security(
        "WIFI:T:WPA;S:school;P:password;T:WEP;;",
        WirelessQrError::ExtraUnsupportedKeys(vec!["T".to_owned()])
    )]
    #[case::missing_separator("WIFI:S;", WirelessQrError::MissingValue("S".to_owned()))]
    #[case::empty_key("WIFI::school;", WirelessQrError::EmptyKey)]
    #[case::escaped_value_end("WIFI:S:school\\", WirelessQrError::UnexpectedEnd)]
    #[case::escaped_key_end("WIFI:S\\", WirelessQrError::UnexpectedEnd)]
    #[case::missing_prefix("WIFI", WirelessQrError::MissingPrefix)]
    #[case::railing_data(
        "WIFI:P: pass123 ;H:false;S: School Wi-Fi ;T:WPA;;some_other_stuff",
        WirelessQrError::TrailingText
    )]
    fn rejects_invalid_codes(
        #[case] code: &str,
        #[case] expected: WirelessQrError,
    ) -> googletest::Result<()> {
        verify_that!(parse_wifi_qr_code_settings(code), err(eq(&expected)))?;
        Ok(())
    }
}
