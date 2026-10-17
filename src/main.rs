use esp_idf_svc::sys;
use std::net::TcpListener;
use std::os::fd::IntoRawFd;

mod config;
mod storage;
mod tls;
mod wifi;

fn tls_listener(
    port: u16,
    certificate_path: &str,
    private_key_path: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut tls_config = tls::ServerTlsConfig::load(certificate_path, private_key_path)?;

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
        let result =
            unsafe { sys::esp_tls_server_session_create(tls_config.as_mut_ptr(), socket_fd, tls) };

        if result == 0 {
            log::info!("TLS handshake succeeded for {}", peer);
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

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities.
    esp_idf_svc::log::EspLogger::initialize_default();

    match storage::mount_littlefs() {
        Ok(()) => {
            log::info!("LittleFS mounted successfully");

            match config::load_config("/littlefs/config.toml") {
                Ok(config) => {
                    log::info!("Configuration loaded successfully");

                    if let Err(err) = storage::check_tls_files(
                        &config.tls.certificate_path,
                        &config.tls.private_key_path,
                    ) {
                        log::error!("Failed to access TLS files: {}", err);
                    } else {
                        let wifi_config = config.wifi;

                        match std::thread::Builder::new()
                            .name("wifi-reconnect".to_owned())
                            .stack_size(6144)
                            .spawn(move || wifi::wifi_task(wifi_config))
                        {
                            Ok(_wifi_thread) => {
                                log::info!("WiFi reconnect task started");

                                if let Err(err) = tls_listener(
                                    config.server.port,
                                    &config.tls.certificate_path,
                                    &config.tls.private_key_path,
                                ) {
                                    log::error!("TLS listener failed: {}", err);
                                }
                            }
                            Err(err) => {
                                log::error!("Failed to start WiFi task: {}", err);
                            }
                        }
                    }
                }
                Err(err) => {
                    log::error!("Failed to load configuration: {}", err);
                }
            }
        }
        Err(err) => {
            log::error!("Failed to mount LittleFS: {}", err);
        }
    }

    log::info!("Hello, world!");
}
