use std::ffi::CString;
use esp_idf_svc::sys as sys;

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

/// Verify that files can be accessed through the mounted LittleFS filesystem.
fn test_littlefs_read() {
    // Define the path of the test file on the LittleFS mount point.
    let test_path = "/littlefs/test.txt";

    // Try to read the test file as UTF-8 text.
    match std::fs::read_to_string(test_path) {
        Ok(contents) => {
            log::info!("LittleFS test file read successfully: {}", contents);
        }
        Err(err) => {
            log::warn!("Could not read {}: {}", test_path, err);
        }
    }
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
            test_littlefs_read();
        }
        Err(err) => {
            log::error!("Failed to mount LittleFS: {}", err);
        }
    }

    log::info!("Hello, world!");
}
