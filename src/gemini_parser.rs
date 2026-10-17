pub fn parse_request_line(line: &[u8], max_bytes: usize) -> Result<String, &'static str> {
    if line.len() > max_bytes {
        return Err("Request line too long");
    }

    let line = line
        .strip_suffix(b"\r\n")
        .ok_or("Request line must end with CRLF")?;

    if line.is_empty() {
        return Err("Request line is empty");
    }

    let uri = std::str::from_utf8(line).map_err(|_| "Request line is not valid UTF-8")?;

    if !uri.starts_with("gemini://") {
        return Err("Request URI must use the gemini:// scheme");
    }

    if uri.chars().any(|character| character.is_control()) {
        return Err("Request URI contains control characters");
    }

    Ok(uri.to_owned())
}

#[cfg(test)]
mod tests {
    use super::parse_request_line;

    #[test]
    fn accepts_valid_gemini_uri() {
        assert_eq!(
            parse_request_line(b"gemini://example.org/docs\r\n", 1024).unwrap(),
            "gemini://example.org/docs"
        );
    }

    #[test]
    fn rejects_missing_crlf() {
        assert_eq!(
            parse_request_line(b"gemini://example.org", 1024),
            Err("Request line must end with CRLF")
        );
    }

    #[test]
    fn rejects_empty_request_line() {
        assert_eq!(
            parse_request_line(b"\r\n", 1024),
            Err("Request line is empty")
        );
    }

    #[test]
    fn rejects_line_over_limit() {
        let line = b"gemini://example.org/\r\n";
        assert_eq!(
            parse_request_line(line, line.len() - 1),
            Err("Request line too long")
        );
    }

    #[test]
    fn accepts_line_exactly_at_limit() {
        let line = b"gemini://example.org/\r\n";
        assert_eq!(
            parse_request_line(line, line.len()).unwrap(),
            "gemini://example.org/"
        );
    }

    #[test]
    fn rejects_invalid_utf8() {
        assert_eq!(
            parse_request_line(b"gemini://example.org/\xff\r\n", 1024),
            Err("Request line is not valid UTF-8")
        );
    }

    #[test]
    fn rejects_wrong_scheme() {
        assert_eq!(
            parse_request_line(b"https://example.org/\r\n", 1024),
            Err("Request URI must use the gemini:// scheme")
        );
    }

    #[test]
    fn rejects_control_characters() {
        assert_eq!(
            parse_request_line(b"gemini://example.org/\x01\r\n", 1024),
            Err("Request URI contains control characters")
        );
    }
}
