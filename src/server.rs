use esp_idf_svc::sys;
use std::net::TcpListener;
use std::os::fd::IntoRawFd;

pub fn run(
    port: u16,
    certificate_path: &str,
    private_key_path: &str,
    handshake_timeout_seconds: u64,
    gemini_config: &crate::config::GeminiConfig,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tls_config = crate::tls::ServerTlsConfig::load(certificate_path, private_key_path)?;

    let listener = TcpListener::bind(("0.0.0.0", port))?;
    log::info!("TLS listener started on port {}", port);

    loop {
        let (stream, peer) = match listener.accept() {
            Ok(connection) => connection,
            Err(err) => {
                log::warn!("TCP accept failed: {}", err);
                continue;
            }
        };

        log::info!("TCP connection accepted from {}", peer);

        let tls = unsafe { sys::esp_tls_init() };
        if tls.is_null() {
            log::error!("Failed to allocate TLS session");
            drop(stream);
            continue;
        }

        let socket_fd = stream.into_raw_fd();
        if let Err(err) = set_socket_timeout(socket_fd, handshake_timeout_seconds) {
            log::warn!("Failed to configure socket timeouts for {}: {}", peer, err);
            unsafe {
                sys::close(socket_fd);
                sys::esp_tls_server_session_delete(tls);
            }
            continue;
        }
        let result =
            unsafe { sys::esp_tls_server_session_create(tls_config.as_mut_ptr(), socket_fd, tls) };

        if result == 0 {
            log::info!("TLS handshake succeeded for {}", peer);
            if let Err(err) = set_socket_timeout(socket_fd, gemini_config.request_timeout_seconds) {
                log::warn!("Failed to configure request timeout for {}: {}", peer, err);
            } else {
                let timeout = std::time::Duration::from_secs(gemini_config.request_timeout_seconds);

                match crate::gemini::read_request_line(
                    tls,
                    gemini_config.request_line_max_bytes,
                    timeout,
                ) {
                    Ok(line) => match crate::gemini_parser::parse_request_line(
                        &line,
                        gemini_config.request_line_max_bytes,
                    ) {
                        Ok(uri) => log::info!("Gemini request from {}: {}", peer, uri),
                        Err(err) => log::warn!("Invalid Gemini request from {}: {}", peer, err),
                    },
                    Err(err) => log::warn!("Failed to read Gemini request from {}: {}", peer, err),
                }
            }
        } else {
            log::warn!("TLS handshake failed for {} with error {}", peer, result);
        }

        // Release TLS resources, then close the socket owned by this session.
        unsafe {
            sys::esp_tls_server_session_delete(tls);
            sys::close(socket_fd);
        }
    }
}

fn set_socket_timeout(
    socket_fd: i32,
    timeout_seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = esp_idf_svc::sys::timeval {
        tv_sec: timeout_seconds.min(i64::MAX as u64) as i64,
        tv_usec: 0,
    };

    for option in [esp_idf_svc::sys::SO_RCVTIMEO, esp_idf_svc::sys::SO_SNDTIMEO] {
        let result = unsafe {
            esp_idf_svc::sys::lwip_setsockopt(
                socket_fd,
                esp_idf_svc::sys::SOL_SOCKET as i32,
                option as i32,
                (&timeout as *const esp_idf_svc::sys::timeval).cast(),
                std::mem::size_of_val(&timeout) as esp_idf_svc::sys::socklen_t,
            )
        };

        if result != 0 {
            return Err(format!("Failed to set socket timeout (option {option})").into());
        }
    }

    Ok(())
}
