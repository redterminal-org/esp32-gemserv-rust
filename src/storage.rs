use esp_idf_svc::sys;
use std::ffi::CString;

pub fn mount_littlefs() -> Result<(), i32> {
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

pub fn check_tls_files(
    certificate_path: &str,
    private_key_path: &str,
) -> Result<(), std::io::Error> {
    use std::fs::File;

    File::open(certificate_path)?;
    log::info!("TLS certificate file is readable");

    File::open(private_key_path)?;
    log::info!("TLS private key file is readable");

    Ok(())
}
