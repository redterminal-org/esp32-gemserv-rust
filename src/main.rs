use std::ffi::CString;
use esp_idf_svc::sys as sys;
use core::convert::TryInto;
use esp_idf_svc::wifi::{AuthMethod, ClientConfiguration, Configuration};
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{BlockingWifi, EspWifi};

mod config;

fn connect_wifi(
    config: &config::WifiConfig,
) -> Result<BlockingWifi<EspWifi<'static>>, Box<dyn std::error::Error>> {
    let peripherals = Peripherals::take()?;
    let sys_loop = EspSystemEventLoop::take()?;
    let nvs = EspDefaultNvsPartition::take()?;

    let mut wifi = BlockingWifi::wrap(
        EspWifi::new(peripherals.modem, sys_loop.clone(), Some(nvs))?,
        sys_loop,
    )?;

    let wifi_configuration = Configuration::Client(ClientConfiguration {
        ssid: config.ssid.as_str().try_into()?,
        bssid: None,
        auth_method: AuthMethod::WPA2Personal,
        password: config.password.as_str().try_into()?,
        channel: None,
        ..Default::default()
    });

    wifi.set_configuration(&wifi_configuration)?;
    wifi.start()?;
    log::info!("WiFi started");

    Ok(wifi)
}

fn wifi_task(config: config::WifiConfig) {
    let mut wifi = match connect_wifi(&config) {
        Ok(wifi) => wifi,
        Err(err) => {
            log::error!("Failed to initialize WiFi: {}", err);
            return;
        }
    };

    let mut retry_delay = std::time::Duration::from_secs(1);
    let max_retry_delay = std::time::Duration::from_secs(30);

    loop {
        match wifi.connect() {
            Ok(()) => match wifi.wait_netif_up() {
                Ok(()) => {
                    log::info!("WiFi connected");
                    retry_delay = std::time::Duration::from_secs(1);

                    match wifi.wifi_wait_while(|| wifi.is_connected(), None) {
                        Ok(()) => log::warn!("WiFi disconnected"),
                        Err(err) => {
                            log::warn!("WiFi event wait failed: {}", err);
                        }
                    }
                }
                Err(err) => {
                    log::warn!("WiFi network interface is not up: {}", err);
                }
            },
            Err(err) => {
                log::warn!("WiFi connection attempt failed: {}", err);
            }
        }

        std::thread::sleep(retry_delay);
        retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
    }
}

fn mount_littlefs() -> Result<(), i32> {
    let base_path = CString::new("/littlefs").unwrap();
    let partition_label = CString::new("littlefs").unwrap();

    let mut conf = sys::esp_vfs_littlefs_conf_t::default();
    conf.base_path = base_path.as_ptr();
    conf.partition_label = partition_label.as_ptr();
    conf.set_format_if_mount_failed(0);
    conf.set_read_only(0);
    conf.set_dont_mount(0);
    conf.set_grow_on_mount(0);

    let result = unsafe { sys::esp_vfs_littlefs_register(&conf) };

    if result == sys::ESP_OK {
        Ok(())
    } else {
        Err(result)
    }
}

fn check_tls_files() -> Result<(), std::io::Error> {
    use std::fs::File;

    File::open("/littlefs/certs/server.crt")?;
    log::info!("TLS certificate file is readable");

    File::open("/littlefs/certs/server.key")?;
    log::info!("TLS private key file is readable");

    Ok(())
}

fn main() {
    // It is necessary to call this function once. Otherwise, some patches to the runtime
    // implemented by esp-idf-sys might not link properly. See https://github.com/esp-rs/esp-idf-template/issues/71
    esp_idf_svc::sys::link_patches();

    // Bind the log crate to the ESP Logging facilities
    esp_idf_svc::log::EspLogger::initialize_default();

    match mount_littlefs() {
        Ok(()) => {
            log::info!("LittleFS mounted successfully");
    
    match config::load_config("/littlefs/config.toml") {
        Ok(config) => {
            log::info!("Configuration loaded successfully");
    
            if let Err(err) = check_tls_files() {
                log::error!("Failed to access TLS files: {}", err);
            } else {

                let wifi_config = config.wifi;
                
                match std::thread::Builder::new()
                    .name("wifi-reconnect".to_owned())
                    .stack_size(6144)
                    .spawn(move || wifi_task(wifi_config))
                {
                    Ok(_wifi_thread) => {
                        log::info!("WiFi reconnect task started");
                
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(60));
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
