use esp_idf_svc::sys;
use std::ffi::CString;
use std::io::Write;

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

    if result == sys::ESP_OK { Ok(()) } else { Err(result) }
}

pub fn append_boot_log(path: &str) -> Result<(), std::io::Error> {
    let relative = path.trim_start_matches('/');
    if relative.is_empty()
        || relative.as_bytes().contains(&0)
        || relative.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "boot log path must be a file path relative to LittleFS root",
        ));
    }

    const ROOT: &[u8] = b"/littlefs/";
    let relative_bytes = relative.as_bytes();
    let target_len = ROOT.len() + relative_bytes.len();
    if target_len >= 256 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "boot log path is too long",
        ));
    }

    let mut target_buffer = [0_u8; 256];
    target_buffer[..ROOT.len()].copy_from_slice(ROOT);
    target_buffer[ROOT.len()..target_len].copy_from_slice(relative_bytes);
    let target = std::str::from_utf8(&target_buffer[..target_len]).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "invalid boot log path")
    })?;
    let parent_end = target.rfind('/').ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "boot log path has no parent directory",
        )
    })?;
    std::fs::create_dir_all(&target[..parent_end])?;

    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(target)?;
    let mut timestamp = [0_u8; 40];
    let timestamp_len = crate::clock::format_local_time(&mut timestamp).ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::Other, "system time is not available")
    })?;

    file.write_all(&timestamp[..timestamp_len])?;
    file.write_all(b" - Booted in ")?;
    write!(file, "{}ms\n", crate::clock::uptime_ms())?;
    file.flush()
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
