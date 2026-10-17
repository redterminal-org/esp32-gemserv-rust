pub fn resolve_url_path(current_path: &str, requested_path: &str) -> Result<String, &'static str> {
    if requested_path.is_empty() {
        return Err("Requested path is empty");
    }

    if requested_path.contains('#') {
        return Err("URL fragments are not supported");
    }

    if requested_path.contains('\\') || requested_path.chars().any(char::is_control) {
        return Err("Requested path contains invalid characters");
    }

    let path = requested_path.split_once('?').map_or(requested_path, |(path, _)| path);

    if path.is_empty() {
        return Err("Requested path is empty");
    }

    let decoded = percent_decode(path)?;

    if decoded.starts_with("//") {
        return Err("Network-path references are not supported");
    }

    let mut segments: Vec<&str> = Vec::new();

    if !decoded.starts_with('/') {
        for segment in current_path.trim_start_matches('/').split('/') {
            if !segment.is_empty() {
                segments.push(segment);
            }
        }

        // Relative links resolve from the current document's directory.
        if !segments.is_empty() {
            segments.pop();
        }
    }

    for segment in decoded.trim_start_matches('/').split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                if segments.pop().is_none() {
                    return Err("Path escapes content root");
                }
            }
            value => segments.push(value),
        }
    }

    if segments.iter().any(|segment| {
        segment.contains(':') || segment.contains('\\') || segment.chars().any(char::is_control)
    }) {
        return Err("Resolved path contains invalid characters");
    }

    let mut resolved = String::from("/");
    resolved.push_str(&segments.join("/"));

    if decoded.ends_with('/') && !resolved.ends_with('/') {
        resolved.push('/');
    }

    Ok(resolved)
}

fn percent_decode(input: &str) -> Result<String, &'static str> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err("Invalid percent encoding");
            }

            let high = hex_value(bytes[index + 1]).ok_or("Invalid percent encoding")?;
            let low = hex_value(bytes[index + 2]).ok_or("Invalid percent encoding")?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    String::from_utf8(decoded).map_err(|_| "Path is not valid UTF-8")
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

pub fn content_file_path(url_path: &str) -> Result<String, &'static str> {
    if !url_path.starts_with('/') {
        return Err("Resolved URL path must be absolute");
    }

    let path = url_path.split('?').next().unwrap_or(url_path);

    if path.contains('%')
        || path.contains('\\')
        || path.contains(':')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|segment| segment == "." || segment == "..")
    {
        return Err("Unsafe content path");
    }

    let relative_path = path.trim_start_matches('/');

    let relative_path = if relative_path.is_empty() || path.ends_with('/') {
        format!("{relative_path}index.gmi")
    } else {
        relative_path.to_owned()
    };

    if relative_path.is_empty() || relative_path.starts_with('/') {
        return Err("Unsafe content path");
    }

    Ok(format!("/littlefs/gemini/{relative_path}"))
}

#[cfg(test)]
mod tests {
    use super::resolve_url_path;

    #[test]
    fn resolves_root_relative_path() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "/about.gmi"),
            Ok("/about.gmi".to_owned())
        );
    }

    #[test]
    fn resolves_relative_path_from_current_directory() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "about.gmi"),
            Ok("/docs/about.gmi".to_owned())
        );
    }

    #[test]
    fn resolves_dot_segments() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "./about.gmi"),
            Ok("/docs/about.gmi".to_owned())
        );
    }

    #[test]
    fn resolves_parent_segment_within_root() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "../index.gmi"),
            Ok("/index.gmi".to_owned())
        );
    }

    #[test]
    fn rejects_parent_segment_above_root() {
        assert_eq!(
            resolve_url_path("/index.gmi", "../config.toml"),
            Err("Path escapes content root")
        );
    }

    #[test]
    fn rejects_encoded_traversal_above_root() {
        assert_eq!(
            resolve_url_path("/index.gmi", "%2e%2e/config.toml"),
            Err("Path escapes content root")
        );
    }

    #[test]
    fn rejects_invalid_percent_encoding() {
        assert_eq!(
            resolve_url_path("/index.gmi", "%2Gfile"),
            Err("Invalid percent encoding")
        );
    }

    #[test]
    fn rejects_backslashes() {
        assert_eq!(
            resolve_url_path("/index.gmi", r"..\secret"),
            Err("Requested path contains invalid characters")
        );
    }

    use super::content_file_path;

    #[test]
    fn maps_root_to_index_file() {
        assert_eq!(
            content_file_path("/"),
            Ok("/littlefs/gemini/index.gmi".to_owned())
        );
    }

    #[test]
    fn maps_directory_with_trailing_slash_to_index_file() {
        assert_eq!(
            content_file_path("/docs/"),
            Ok("/littlefs/gemini/docs/index.gmi".to_owned())
        );
    }

    #[test]
    fn maps_regular_file_under_content_root() {
        assert_eq!(
            content_file_path("/docs/start.gmi"),
            Ok("/littlefs/gemini/docs/start.gmi".to_owned())
        );
    }

    #[test]
    fn removes_query_from_filesystem_path() {
        assert_eq!(
            content_file_path("/search.gmi?q=hello"),
            Ok("/littlefs/gemini/search.gmi".to_owned())
        );
    }

    #[test]
    fn rejects_parent_directory_traversal() {
        assert_eq!(
            content_file_path("/../config.toml"),
            Err("Unsafe content path")
        );
    }

    #[test]
    fn rejects_non_absolute_paths() {
        assert_eq!(
            content_file_path("docs/start.gmi"),
            Err("Resolved URL path must be absolute")
        );
    }

    #[test]
    fn rejects_encoded_traversal_after_decoding() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "%2e%2e/%2e%2e/config.toml"),
            Err("Path escapes content root")
        );
    }

    #[test]
    fn does_not_decode_double_encoded_traversal_twice() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "%252e%252e/config.toml"),
            Ok("/docs/%2e%2e/config.toml".to_owned())
        );
    }

    #[test]
    fn rejects_remaining_percent_encoding_in_content_path() {
        assert_eq!(
            content_file_path("/docs/%2e%2e/config.toml"),
            Err("Unsafe content path")
        );
    }

    #[test]
    fn resolves_relative_path_with_query_parameter() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "../about.gmi?lang=de"),
            Ok("/about.gmi".to_owned())
        );
    }

    #[test]
    fn resolves_query_only_reference_to_current_directory_index_path() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "?lang=de"),
            Err("Requested path is empty")
        );
    }

    #[test]
    fn rejects_url_fragments() {
        assert_eq!(
            resolve_url_path("/docs/start.gmi", "about.gmi#section"),
            Err("URL fragments are not supported")
        );
    }
}
