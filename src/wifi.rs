use core::convert::TryInto;
use esp_idf_svc::eventloop::EspSystemEventLoop;
use esp_idf_svc::hal::peripherals::Peripherals;
use esp_idf_svc::nvs::EspDefaultNvsPartition;
use esp_idf_svc::wifi::{AuthMethod, BlockingWifi, ClientConfiguration, Configuration, EspWifi};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static WIFI_READY: AtomicBool = AtomicBool::new(false);
static WIFI_INITIALIZATION_FAILED: AtomicBool = AtomicBool::new(false);

fn connect_wifi(
    config: &crate::config::WifiConfig,
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

pub fn is_ready() -> bool {
    WIFI_READY.load(Ordering::Relaxed)
}

pub fn initialization_failed() -> bool {
    WIFI_INITIALIZATION_FAILED.load(Ordering::Relaxed)
}

pub fn wifi_task(config: crate::config::WifiConfig) {
    let mut wifi = match connect_wifi(&config) {
        Ok(wifi) => wifi,
        Err(err) => {
            log::error!("Failed to initialize WiFi: {}", err);
            WIFI_INITIALIZATION_FAILED.store(true, Ordering::Relaxed);
            return;
        }
    };

    let mut retry_delay = Duration::from_secs(1);
    let max_retry_delay = Duration::from_secs(30);

    loop {
        match wifi.connect() {
            Ok(()) => match wifi.wait_netif_up() {
                Ok(()) => {
                    log::info!("WiFi connected");
                    retry_delay = Duration::from_secs(1);

                    loop {
                        match crate::clock::synchronize_time() {
                            Ok(()) => {
                                log::info!("System time synchronized using NTP");
                                WIFI_READY.store(true, Ordering::Relaxed);
                                break;
                            }
                            Err(err) => {
                                log::warn!("NTP synchronization failed: {}; retrying", err);
                                std::thread::sleep(Duration::from_secs(5));
                                match wifi.is_connected() {
                                    Ok(true) => {}
                                    _ => break,
                                }
                            }
                        }
                    }

                    match wifi.wifi_wait_while(|| wifi.is_connected(), None) {
                        Ok(()) => log::warn!("WiFi disconnected"),
                        Err(err) => log::warn!("WiFi event wait failed: {}", err),
                    }
                }
                Err(err) => log::warn!("WiFi network interface is not up: {}", err),
            },
            Err(err) => log::warn!("WiFi connection attempt failed: {}", err),
        }

        std::thread::sleep(retry_delay);
        retry_delay = std::cmp::min(retry_delay * 2, max_retry_delay);
    }
}
