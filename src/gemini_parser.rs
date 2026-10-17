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

pub fn extract_request_path<'a>(
    uri: &'a str,
    expected_hostname: &str,
    expected_port: u16,
) -> Result<&'a str, &'static str> {
    let authority_and_path = uri
        .strip_prefix("gemini://")
        .ok_or("Request URI must use the gemini:// scheme")?;

    let authority_end = authority_and_path
        .find(['/', '?', '#'])
        .unwrap_or(authority_and_path.len());
    if authority_and_path.contains('#') {
        return Err("Request URI must not contain a fragment");
    }

    let authority = &authority_and_path[..authority_end];
    let remainder = &authority_and_path[authority_end..];

    if authority.is_empty() || authority.contains('@') {
        return Err("Invalid request authority");
    }

    let (hostname, port) = match authority.rsplit_once(':') {
        Some((hostname, port_text)) if !hostname.contains(']') => {
            let port = port_text
                .parse::<u16>()
                .map_err(|_| "Invalid request port")?;
            (hostname, Some(port))
        }
        _ => (authority, None),
    };

    if !hostname.eq_ignore_ascii_case(expected_hostname) {
        return Err("Request host does not match server hostname");
    }

    if port.is_some_and(|port| port != expected_port) {
        return Err("Request port does not match server port");
    }

    if remainder.starts_with('#') {
        return Err("Request URI must not contain a fragment");
    }

    let path_and_query = remainder.split('#').next().unwrap_or(remainder);

    if path_and_query.is_empty() {
        return Ok("/");
    }

    if path_and_query.starts_with('?') {
        return Ok(path_and_query);
    }

    Ok(path_and_query)
}

#[cfg(test)]
mod tests {
    use super::{extract_request_path, parse_request_line};

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

    #[test]
    fn extracts_root_path_when_path_is_missing() {
        assert_eq!(
            extract_request_path("gemini://gemini.example.net", "gemini.example.net", 1965),
            Ok("/")
        );
    }

    #[test]
    fn extracts_path_and_query() {
        assert_eq!(
            extract_request_path(
                "gemini://gemini.example.net/docs?lang=de",
                "gemini.example.net",
                1965
            ),
            Ok("/docs?lang=de")
        );
    }

    #[test]
    fn accepts_matching_explicit_port() {
        assert_eq!(
            extract_request_path(
                "gemini://gemini.example.net:1965/about.gmi",
                "gemini.example.net",
                1965
            ),
            Ok("/about.gmi")
        );
    }

    #[test]
    fn rejects_wrong_hostname() {
        assert_eq!(
            extract_request_path("gemini://other.example.net/", "gemini.example.net", 1965),
            Err("Request host does not match server hostname")
        );
    }

    #[test]
    fn rejects_wrong_port() {
        assert_eq!(
            extract_request_path(
                "gemini://gemini.example.net:1966/",
                "gemini.example.net",
                1965
            ),
            Err("Request port does not match server port")
        );
    }

    #[test]
    fn rejects_invalid_port() {
        assert_eq!(
            extract_request_path(
                "gemini://gemini.example.net:abc/",
                "gemini.example.net",
                1965
            ),
            Err("Invalid request port")
        );
    }

    #[test]
    fn rejects_userinfo() {
        assert_eq!(
            extract_request_path(
                "gemini://user@gemini.example.net/",
                "gemini.example.net",
                1965
            ),
            Err("Invalid request authority")
        );
    }

    #[test]
    fn rejects_fragments() {
        assert_eq!(
            extract_request_path(
                "gemini://gemini.example.net/about#section",
                "gemini.example.net",
                1965
            ),
            Err("Request URI must not contain a fragment")
        );
    }
}
