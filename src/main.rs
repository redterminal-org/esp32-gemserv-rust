#![allow(linker_messages)]
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
                                if let Err(err) = server::run(
                                    &config.server.hostname,
                                    config.server.port,
                                    &config.tls.certificate_path,
                                    &config.tls.private_key_path,
                                    config.tls.handshake_timeout_seconds,
                                    &config.gemini,
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
