//! Code generation, normalization, and invite link parser.

use rand::Rng;
use url::Url;

pub const ALPHABET: &str = "0123456789ABCDEFGHJKMNPQRSTUVWXYZ";
pub const CODE_LENGTH: usize = 6;

/// Generates a random 6-character party room code.
pub fn generate_code() -> String {
    let mut rng = rand::thread_rng();
    let chars: Vec<char> = ALPHABET.chars().collect();
    (0..CODE_LENGTH)
        .map(|_| chars[rng.gen_range(0..chars.len())])
        .collect()
}

/// Normalises a user-typed or pasted party code:
/// - Removes non-alphanumeric characters and spaces
/// - Converts to uppercase
/// - Maps ambiguous characters: 'I' -> '1', 'L' -> '1', 'O' -> '0'
pub fn normalise_code(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    for ch in input.trim().chars() {
        let upper = ch.to_ascii_uppercase();
        if upper == 'I' || upper == 'L' {
            result.push('1');
        } else if upper == 'O' {
            result.push('0');
        } else if ALPHABET.contains(upper) {
            result.push(upper);
        }
    }
    result
}

/// Validates if the code is exactly 6 characters and belongs to the alphabet.
pub fn is_valid_code(code: &str) -> bool {
    if code.len() != CODE_LENGTH {
        return false;
    }
    code.chars().all(|c| ALPHABET.contains(c))
}

/// Extracted party code and optional server URL from text, invite links, or aftertone:// URIs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtractedCode {
    pub code: String,
    pub server_url: Option<String>,
}

/// Parses a code or invite link:
/// - "aftertone://party/7A9K2M"
/// - "https://aftertone.app/invite/7A9K2M"
/// - "7A9K2M"
pub fn extract_code_and_server(raw: &str) -> Option<ExtractedCode> {
    if raw.trim().is_empty() {
        return None;
    }
    let mut str = raw.trim().to_string();

    // If looks like invite link without protocol, prepend https://
    if !str.contains("://") && (str.contains("/invite/") || str.contains("aftertone")) {
        str = format!("https://{}", str);
    }

    // Try URL parsing
    if str.contains("://") {
        if let Ok(parsed_url) = Url::parse(&str) {
            let query_server = parsed_url
                .query_pairs()
                .find(|(k, _)| k == "server")
                .map(|(_, v)| v.into_owned());

            // 1. aftertone://party/<CODE> or custom scheme
            if parsed_url.scheme().starts_with("aftertone") {
                let path_trimmed = parsed_url.path().trim_matches('/');
                let candidate = if !path_trimmed.is_empty() {
                    path_trimmed.to_string()
                } else {
                    parsed_url
                        .query_pairs()
                        .find(|(k, _)| k == "code")
                        .map(|(_, v)| v.into_owned())
                        .unwrap_or_default()
                };
                let norm = normalise_code(&candidate);
                if is_valid_code(&norm) {
                    return Some(ExtractedCode {
                        code: norm,
                        server_url: query_server,
                    });
                }
            }

            // 2. /invite/<CODE> or /party/<CODE>
            let path_segments: Vec<&str> = parsed_url
                .path_segments()
                .map(|c| c.collect())
                .unwrap_or_default();
            for i in 0..path_segments.len() {
                let seg = path_segments[i];
                if (seg.eq_ignore_ascii_case("invite") || seg.eq_ignore_ascii_case("party"))
                    && i + 1 < path_segments.len()
                {
                    let candidate = path_segments[i + 1];
                    let norm = normalise_code(candidate);
                    if is_valid_code(&norm) {
                        return Some(ExtractedCode {
                            code: norm,
                            server_url: query_server,
                        });
                    }
                }
            }

            // 3. Query param ?code=<CODE>
            if let Some((_, code_val)) = parsed_url.query_pairs().find(|(k, _)| k == "code") {
                let norm = normalise_code(&code_val);
                if is_valid_code(&norm) {
                    return Some(ExtractedCode {
                        code: norm,
                        server_url: query_server,
                    });
                }
            }
        }
    }

    // Direct code
    let direct = normalise_code(&str);
    if is_valid_code(&direct) {
        return Some(ExtractedCode {
            code: direct,
            server_url: None,
        });
    }

    // Check tokens within string
    for token in str.split(|c: char| !c.is_alphanumeric()) {
        let norm = normalise_code(token);
        if is_valid_code(&norm) {
            return Some(ExtractedCode {
                code: norm,
                server_url: None,
            });
        }
    }

    None
}
