use esp_idf_svc::sys;
use std::fs::File;
use std::io::Read;
use std::net::TcpListener;
use std::os::fd::IntoRawFd;
use std::time::Duration;

use crate::config::GeminiConfig;
use crate::gemini_response::GeminiStatus;

pub fn run(
    hostname: &str,
    port: u16,
    certificate_path: &str,
    private_key_path: &str,
    handshake_timeout_seconds: u64,
    gemini_config: &GeminiConfig,
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

        let connection_result = (|| -> Result<(), &'static str> {
            set_socket_timeout(socket_fd, handshake_timeout_seconds)
                .map_err(|_| "Failed to configure TLS handshake timeout")?;

            let result = unsafe {
                sys::esp_tls_server_session_create(tls_config.as_mut_ptr(), socket_fd, tls)
            };

            if result != 0 {
                log::warn!("TLS handshake failed for {} with error {}", peer, result);
                return Ok(());
            }

            log::info!("TLS handshake succeeded for {}", peer);

            set_socket_timeout(socket_fd, gemini_config.request_timeout_seconds)
                .map_err(|_| "Failed to configure request timeout")?;

            let timeout = Duration::from_secs(gemini_config.request_timeout_seconds);

            let request_line = match crate::gemini::read_request_line(
                tls,
                gemini_config.request_line_max_bytes,
                timeout,
            ) {
                Ok(line) => line,
                Err(err) => {
                    log::warn!("Failed to read Gemini request from {}: {}", peer, err);
                    send_error(tls, GeminiStatus::BadRequest, "Invalid request", timeout)?;
                    return Ok(());
                }
            };

            let uri = match crate::gemini_parser::parse_request_line(
                &request_line,
                gemini_config.request_line_max_bytes,
            ) {
                Ok(uri) => uri,
                Err(err) => {
                    log::warn!("Invalid Gemini request from {}: {}", peer, err);
                    send_error(tls, GeminiStatus::BadRequest, "Invalid request", timeout)?;
                    return Ok(());
                }
            };

            let request_path =
                match crate::gemini_parser::extract_request_path(&uri, hostname, port) {
                    Ok(path) => path,
                    Err(err) => {
                        log::warn!("Rejected Gemini request from {}: {}", peer, err);
                        send_error(
                            tls,
                            GeminiStatus::BadRequest,
                            "Invalid request URI",
                            timeout,
                        )?;
                        return Ok(());
                    }
                };

            let resolved_path = match crate::content_path::resolve_url_path("/", &request_path) {
                Ok(path) => path,
                Err(err) => {
                    log::warn!("Rejected URL path from {}: {}", peer, err);
                    send_error(tls, GeminiStatus::BadRequest, "Invalid path", timeout)?;
                    return Ok(());
                }
            };

            let file_path = match crate::content_path::content_file_path(&resolved_path) {
                Ok(path) => path,
                Err(err) => {
                    log::warn!("Rejected content path from {}: {}", peer, err);
                    send_error(tls, GeminiStatus::BadRequest, "Invalid path", timeout)?;
                    return Ok(());
                }
            };

            let mut file = match File::open(&file_path) {
                Ok(file) => file,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    log::info!("Gemini content not found: {}", file_path);
                    send_error(tls, GeminiStatus::NotFound, "Not found", timeout)?;
                    return Ok(());
                }
                Err(err) => {
                    log::error!("Failed to open {}: {}", file_path, err);
                    send_error(
                        tls,
                        GeminiStatus::TemporaryFailure,
                        "Unable to read content",
                        timeout,
                    )?;
                    return Ok(());
                }
            };

            let header = crate::gemini_response::success_header(&file_path, gemini_config)
                .map_err(|_| "Failed to construct Gemini response header")?;

            crate::gemini::write_all(tls, &header, timeout)?;

            let chunk_size = gemini_config.response_chunk_size.clamp(1, 8192);
            let mut buffer = vec![0_u8; chunk_size];

            loop {
                let count = match file.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => count,
                    Err(err) => {
                        log::error!("Failed while reading {}: {}", file_path, err);
                        return Err("Content read failed");
                    }
                };

                crate::gemini::write_all(tls, &buffer[..count], timeout)?;
            }

            log::info!(
                "Gemini response sent to {}: URI={}, file={}",
                peer,
                uri,
                file_path
            );

            Ok(())
        })();

        if let Err(err) = connection_result {
            log::warn!("Connection processing failed for {}: {}", peer, err);
        }

        // Gemini uses one request per connection. Release TLS resources, then
        // close the accepted socket.
        unsafe {
            sys::esp_tls_server_session_delete(tls);
            sys::close(socket_fd);
        }
    }
}

fn send_error(
    tls: *mut sys::esp_tls_t,
    status: GeminiStatus,
    message: &str,
    timeout: Duration,
) -> Result<(), &'static str> {
    let header = status.response_header(message)?;
    crate::gemini::write_all(tls, &header, timeout)
}

fn set_socket_timeout(
    socket_fd: i32,
    timeout_seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = sys::timeval {
        tv_sec: timeout_seconds.min(i64::MAX as u64) as i64,
        tv_usec: 0,
    };

    for option in [sys::SO_RCVTIMEO, sys::SO_SNDTIMEO] {
        let result = unsafe {
            sys::lwip_setsockopt(
                socket_fd,
                sys::SOL_SOCKET as i32,
                option as i32,
                (&timeout as *const sys::timeval).cast(),
                std::mem::size_of_val(&timeout) as sys::socklen_t,
            )
        };

        if result != 0 {
            return Err(format!("Failed to set socket timeout (option {option})").into());
        }
    }

    Ok(())
}
