//! Payload builders for the three content types: free text, website link
//! and Wi-Fi login. Pure logic, no Tauri types, fully unit tested.
//!
//! The payload string is what actually gets encoded into the QR matrix —
//! building it in Rust (not the frontend) keeps escaping and validation
//! out of the webview.

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// Free text, multi-line, any Unicode. Kept exactly as typed (no trimming),
/// because every character is part of the encoded content.
pub fn text_payload(text: &str) -> String {
    text.to_string()
}

/// True when a text payload has no meaningful content (empty or only
/// whitespace), so the UI can disable Save and skip generation.
pub fn is_blank(text: &str) -> bool {
    text.chars().all(char::is_whitespace)
}

// ---------------------------------------------------------------------------
// Website
// ---------------------------------------------------------------------------

/// Normalizes a website link and validates it. Returns the final URL that
/// goes into the QR code.
///
/// Rules: surrounding whitespace is trimmed; a missing scheme gets
/// `https://` added; only http and https are accepted here (other schemes
/// belong in the Text tab); the host must be non-empty and the URL must not
/// contain whitespace or control characters.
pub fn normalize_url(input: &str) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("Enter a website address.".into());
    }
    // Tolerate a pasted "protocol-relative" link ("//example.com/x").
    let trimmed = trimmed.strip_prefix("//").unwrap_or(trimmed);

    // A scheme is present when a letters-only prefix is followed by ':'
    // before any path separator (keeps "example.com:8080/x" as host:port).
    let first_colon = trimmed.find(':');
    let first_separator = trimmed.find(['/', '?', '#']).unwrap_or(trimmed.len());
    let explicit_scheme = first_colon.is_some_and(|colon| {
        colon < first_separator
            && colon > 0
            && trimmed[..colon].chars().all(|c| c.is_ascii_alphabetic())
    });

    let lower = trimmed.to_ascii_lowercase();
    let url = if lower.starts_with("http://") || lower.starts_with("https://") {
        trimmed.to_string()
    } else if explicit_scheme {
        return Err(
            "Only http and https links are supported. Put other link types in the Text tab.".into(),
        );
    } else {
        format!("https://{trimmed}")
    };

    if url.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("The link must not contain spaces or control characters.".into());
    }

    // Host check: everything up to the first /, ? or # after the scheme,
    // with optional userinfo (user@host) and port (host:8080) stripped.
    let after_scheme = url.split_once("://").map_or("", |(_, rest)| rest);
    let authority = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host = authority.rsplit('@').next().unwrap_or_default();
    let host = host.split(':').next().unwrap_or_default();
    if host.is_empty() {
        return Err("The link has no valid host name.".into());
    }

    Ok(url)
}

// ---------------------------------------------------------------------------
// Wi-Fi
// ---------------------------------------------------------------------------

/// Wi-Fi security type. WPA covers WPA/WPA2/WPA3 — the standard Wi-Fi QR
/// payload uses the same `WPA` token for all of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WifiSecurity {
    Wpa,
    Wep,
    None,
}

impl WifiSecurity {
    /// Maps the UI token to a security type; anything unknown is an error.
    pub fn from_ui(value: &str) -> Result<Self, String> {
        match value {
            "wpa" => Ok(WifiSecurity::Wpa),
            "wep" => Ok(WifiSecurity::Wep),
            "nopass" => Ok(WifiSecurity::None),
            other => Err(format!("Unknown Wi-Fi security type: {other}")),
        }
    }

    fn token(self) -> &'static str {
        match self {
            WifiSecurity::Wpa => "WPA",
            WifiSecurity::Wep => "WEP",
            WifiSecurity::None => "nopass",
        }
    }
}

/// All inputs for a Wi-Fi login QR code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WifiOptions {
    pub ssid: String,
    pub password: String,
    pub security: WifiSecurity,
    pub hidden: bool,
}

/// Escapes the special characters of the Wi-Fi payload format: backslash,
/// semicolon, comma, colon and double quote are each prefixed with a
/// backslash. Backslash must be escaped first so earlier escapes are not
/// doubled — iterating characters one by one gives that for free.
fn escape_wifi(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        if matches!(c, '\\' | ';' | ',' | ':' | '"') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Builds the standard Wi-Fi payload:
/// `WIFI:T:<type>;S:<ssid>;P:<password>;H:true;;` (H only when hidden,
/// P only when the network uses a password).
pub fn wifi_payload(options: &WifiOptions) -> Result<String, String> {
    let ssid = options.ssid.trim();
    if ssid.is_empty() {
        return Err("Enter the network name (SSID).".into());
    }
    if options.security != WifiSecurity::None && options.password.is_empty() {
        return Err("Enter the network password, or choose \u{201c}No password\u{201d}.".into());
    }

    let mut payload = format!(
        "WIFI:T:{};S:{};",
        options.security.token(),
        escape_wifi(ssid)
    );
    if options.security != WifiSecurity::None {
        payload.push_str(&format!("P:{};", escape_wifi(&options.password)));
    }
    if options.hidden {
        payload.push_str("H:true;");
    }
    payload.push(';');
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_keeps_everything() {
        assert_eq!(text_payload("  hi \nthere  "), "  hi \nthere  ");
        assert_eq!(
            text_payload("\u{0633}\u{0644}\u{0627}\u{0645}"),
            "\u{0633}\u{0644}\u{0627}\u{0645}"
        );
    }

    #[test]
    fn blank_detection() {
        assert!(is_blank(""));
        assert!(is_blank(" \t\n "));
        assert!(!is_blank("a"));
        assert!(!is_blank(" \u{00a0}x"));
    }

    #[test]
    fn url_adds_https_and_trims() {
        assert_eq!(
            normalize_url("  example.com/path ").unwrap(),
            "https://example.com/path"
        );
        assert_eq!(
            normalize_url("HTTP://Example.COM").unwrap(),
            "HTTP://Example.COM"
        );
        assert_eq!(normalize_url("https://a.b").unwrap(), "https://a.b");
    }

    #[test]
    fn url_rejects_bad_input() {
        assert!(normalize_url("").is_err());
        assert!(normalize_url("   ").is_err());
        assert!(normalize_url("ftp://example.com").is_err());
        assert!(normalize_url("javascript:alert(1)").is_err());
        assert!(normalize_url("https://ex ample.com").is_err());
        assert!(normalize_url("https://").is_err());
        assert!(normalize_url("https:// user@host").is_err());
    }

    #[test]
    fn url_accepts_ports_userinfo_and_no_tld() {
        assert_eq!(
            normalize_url("example.com:8080/x").unwrap(),
            "https://example.com:8080/x"
        );
        assert_eq!(
            normalize_url("https://user:pw@host.local").unwrap(),
            "https://user:pw@host.local"
        );
        assert_eq!(normalize_url("myserver").unwrap(), "https://myserver");
    }

    #[test]
    fn wifi_escapes_all_special_characters() {
        let escaped = escape_wifi("a\\b;c,d:e\"f");
        assert_eq!(escaped, "a\\\\b\\;c\\,d\\:e\\\"f");
    }

    #[test]
    fn wifi_payload_shapes() {
        let wifi = WifiOptions {
            ssid: "Home".into(),
            password: "secret".into(),
            security: WifiSecurity::Wpa,
            hidden: false,
        };
        assert_eq!(wifi_payload(&wifi).unwrap(), "WIFI:T:WPA;S:Home;P:secret;;");

        let hidden = WifiOptions {
            hidden: true,
            ..wifi
        };
        assert_eq!(
            wifi_payload(&hidden).unwrap(),
            "WIFI:T:WPA;S:Home;P:secret;H:true;;"
        );

        let wep = WifiOptions {
            security: WifiSecurity::Wep,
            ..hidden
        };
        assert_eq!(
            wifi_payload(&wep).unwrap(),
            "WIFI:T:WEP;S:Home;P:secret;H:true;;"
        );

        let open = WifiOptions {
            ssid: "Cafe".into(),
            password: String::new(),
            security: WifiSecurity::None,
            hidden: false,
        };
        assert_eq!(wifi_payload(&open).unwrap(), "WIFI:T:nopass;S:Cafe;;");
    }

    #[test]
    fn wifi_password_with_special_characters_survives_roundtrip_shape() {
        let wifi = WifiOptions {
            ssid: "net;work".into(),
            password: "p\\a,s:s\"w;".into(),
            security: WifiSecurity::Wpa,
            hidden: false,
        };
        let payload = wifi_payload(&wifi).unwrap();
        // Escaped SSID and password contain no raw special characters
        // outside their escaped form.
        assert_eq!(
            payload,
            "WIFI:T:WPA;S:net\\;work;P:p\\\\a\\,s\\:s\\\"w\\;;;"
        );
    }

    #[test]
    fn wifi_rejects_missing_ssid_or_password() {
        let no_ssid = WifiOptions {
            ssid: "  ".into(),
            password: "x".into(),
            security: WifiSecurity::Wpa,
            hidden: false,
        };
        assert!(wifi_payload(&no_ssid).is_err());

        let no_password = WifiOptions {
            ssid: "net".into(),
            password: String::new(),
            security: WifiSecurity::Wpa,
            hidden: false,
        };
        assert!(wifi_payload(&no_password).is_err());
    }

    #[test]
    fn wifi_security_from_ui() {
        assert_eq!(WifiSecurity::from_ui("wpa"), Ok(WifiSecurity::Wpa));
        assert_eq!(WifiSecurity::from_ui("nopass"), Ok(WifiSecurity::None));
        assert!(WifiSecurity::from_ui("wpa2").is_err());
    }
}
