use crate::config::GeminiConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum GeminiStatus {
    Input = 10,
    SensitiveInput = 11,
    Success = 20,
    TemporaryRedirect = 30,
    PermanentRedirect = 31,
    TemporaryFailure = 40,
    ServerUnavailable = 41,
    CGIError = 42,
    ProxyError = 43,
    SlowDown = 44,
    PermanentFailure = 50,
    NotFound = 51,
    Gone = 52,
    ProxyRequestRefused = 53,
    BadRequest = 59,
    ClientCertificateRequired = 60,
    CertificateNotAuthorized = 61,
    CertificateNotValid = 62,
}

impl GeminiStatus {
    pub const fn code(self) -> u8 {
        self as u8
    }

    pub const fn allows_body(self) -> bool {
        self.code() / 10 == 2
    }

    pub fn response_header(self, meta: &str) -> Result<Vec<u8>, &'static str> {
        if meta.contains(['\r', '\n']) || meta.starts_with('\u{feff}') {
            return Err("Invalid Gemini response meta");
        }

        if meta.len() > 1024 {
            return Err("Gemini response meta exceeds 1024 bytes");
        }

        if !self.allows_body() && meta.is_empty() {
            return Err("Gemini error or redirect response requires meta");
        }

        let meta = if self == Self::Success && meta.is_empty() {
            "text/gemini; charset=utf-8"
        } else {
            meta
        };

        let header = format!("{:02} {}\r\n", self.code(), meta);

        if header.len() > 1026 {
            return Err("Gemini response header exceeds the protocol limit");
        }

        Ok(header.into_bytes())
    }
}

pub fn mime_type_for_path(path: &str) -> &'static str {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    let extension = path.rsplit('/').next().unwrap_or(path);
    let extension = extension
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();

    match extension.as_str() {
        "gmi" | "gemini" => "text/gemini",
        "txt" | "text" | "log" => "text/plain",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "csv" => "text/csv",
        "xml" => "application/xml",
        "json" => "application/json",
        "js" | "mjs" => "text/javascript",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "ico" => "image/vnd.microsoft.icon",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "tif" | "tiff" => "image/tiff",
        "pdf" => "application/pdf",
        "zip" => "application/zip",
        "gz" => "application/gzip",
        "tar" => "application/x-tar",
        "wasm" => "application/wasm",
        "mp3" => "audio/mpeg",
        "ogg" => "audio/ogg",
        "wav" => "audio/wav",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

pub fn success_header(path: &str, config: &GeminiConfig) -> Result<Vec<u8>, &'static str> {
    let mime_type = mime_type_for_path(path);

    let meta = if mime_type == "text/gemini" {
        match config.lang.as_deref() {
            Some(lang) if !lang.is_empty() => {
                validate_lang(lang)?;
                format!("{mime_type}; charset=utf-8; lang={lang}")
            }
            _ => format!("{mime_type}; charset=utf-8"),
        }
    } else if mime_type.starts_with("text/") {
        format!("{mime_type}; charset=utf-8")
    } else {
        mime_type.to_owned()
    };

    GeminiStatus::Success.response_header(&meta)
}

fn validate_lang(lang: &str) -> Result<(), &'static str> {
    if lang.len() > 128
        || lang.contains([';', '\r', '\n', ' ', '\t'])
        || lang.starts_with('\u{feff}')
        || !lang.split(',').all(|tag| {
            !tag.is_empty() && tag.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err("Invalid Gemini lang parameter");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{mime_type_for_path, GeminiStatus};

    #[test]
    fn status_codes_match_gemini_protocol() {
        assert_eq!(GeminiStatus::Input.code(), 10);
        assert_eq!(GeminiStatus::Success.code(), 20);
        assert_eq!(GeminiStatus::NotFound.code(), 51);
        assert_eq!(GeminiStatus::CertificateNotValid.code(), 62);
    }

    #[test]
    fn success_with_empty_meta_uses_default_mime_type() {
        assert_eq!(
            GeminiStatus::Success.response_header("").unwrap(),
            b"20 text/gemini; charset=utf-8\r\n"
        );
    }

    #[test]
    fn rejects_meta_with_line_breaks() {
        assert!(GeminiStatus::Success
            .response_header("text/plain\r\n51 Not found")
            .is_err());
    }

    #[test]
    fn rejects_meta_over_1024_bytes() {
        assert!(GeminiStatus::Success
            .response_header(&"x".repeat(1025))
            .is_err());
    }

    #[test]
    fn maps_common_file_extensions() {
        assert_eq!(mime_type_for_path("/index.gmi"), "text/gemini");
        assert_eq!(mime_type_for_path("/assets/site.css"), "text/css");
        assert_eq!(mime_type_for_path("/images/logo.png"), "image/png");
        assert_eq!(
            mime_type_for_path("/download.unknown"),
            "application/octet-stream"
        );
    }

    #[test]
    fn extension_matching_is_case_insensitive() {
        assert_eq!(mime_type_for_path("/IMAGE.PNG"), "image/png");
    }
}
