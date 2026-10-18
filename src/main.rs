#![allow(linker_messages)]
mod clock;
mod config;
mod content_path;
mod gemini;
mod gemini_parser;
mod gemini_response;
mod server;
mod storage;
mod tls;
mod wifi;

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();
    clock::start();

    // Bind the log crate to the ESP Logging facilities.
    esp_idf_svc::log::EspLogger::initialize_default();

    match storage::mount_littlefs() {
        Ok(()) => {
            log::info!("LittleFS mounted successfully");

            match config::load_config("/littlefs/config.toml") {
                Ok(config) => {
                    log::info!("Configuration loaded successfully");

                    let timezone = config.gemini.timezone.as_deref().filter(|value| !value.trim().is_empty()).unwrap_or("UTC0");
                    if let Err(err) = clock::initialize_timezone(timezone) {
                        log::error!("Failed to configure timezone: {}", err);
                    } else if let Err(err) = storage::check_tls_files(
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
                                while !wifi::is_ready() && !wifi::initialization_failed() {
                                    std::thread::sleep(std::time::Duration::from_millis(100));
                                }

                                if wifi::is_ready() {
                                    log::info!("WiFi connected and NTP time synchronized");
                                    if let Err(err) = server::run(
                                        &config.server.hostname,
                                        config.server.port,
                                        &config.tls.certificate_path,
                                        &config.tls.private_key_path,
                                        config.tls.handshake_timeout_seconds,
                                        &config.gemini,
                                        config.gemini.boot_log_path.as_deref(),
                                    ) {
                                        log::error!("TLS listener failed: {}", err);
                                    }
                                } else {
                                    log::error!("WiFi initialization failed; server will not start");
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
