use esp_idf_svc::sys;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::time::Duration;

static BOOT_TIME_US_HIGH: AtomicU32 = AtomicU32::new(0);
static BOOT_TIME_US_LOW: AtomicU32 = AtomicU32::new(0);
static SNTP_INITIALIZED: AtomicBool = AtomicBool::new(false);
static TIME_SYNCHRONIZED: AtomicBool = AtomicBool::new(false);

pub fn start() {
    let started = unsafe { sys::esp_timer_get_time() } as u64;
    BOOT_TIME_US_HIGH.store((started >> 32) as u32, Ordering::Relaxed);
    BOOT_TIME_US_LOW.store(started as u32, Ordering::Relaxed);
}

pub fn uptime_ms() -> u64 {
    let started = ((BOOT_TIME_US_HIGH.load(Ordering::Relaxed) as u64) << 32)
        | BOOT_TIME_US_LOW.load(Ordering::Relaxed) as u64;
    let now = unsafe { sys::esp_timer_get_time() }.max(0) as u64;
    now.saturating_sub(started) / 1_000
}

pub fn initialize_timezone(timezone: &str) -> Result<(), std::io::Error> {
    if timezone.as_bytes().contains(&0) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "timezone contains a NUL byte",
        ));
    }

    std::env::set_var("TZ", timezone);
    unsafe { tzset() };
    Ok(())
}

pub fn unix_time_seconds() -> Option<i64> {
    if !TIME_SYNCHRONIZED.load(Ordering::Relaxed) {
        return None;
    }

    let mut value: sys::time_t = 0;
    let result = unsafe { sys::time(&mut value) };
    if result < 0 { None } else { Some(value as i64) }
}

pub fn format_local_time(output: &mut [u8]) -> Option<usize> {
    format_time(true, output)
}

pub fn format_utc_time(output: &mut [u8]) -> Option<usize> {
    format_time(false, output)
}

fn format_time(local: bool, output: &mut [u8]) -> Option<usize> {
    if !TIME_SYNCHRONIZED.load(Ordering::Relaxed) {
        return None;
    }

    let mut value: sys::time_t = 0;
    if unsafe { sys::time(&mut value) } < 0 {
        return None;
    }

    let mut tm: sys::tm = unsafe { std::mem::zeroed() };
    let result = unsafe {
        if local {
            sys::localtime_r(&value, &mut tm)
        } else {
            sys::gmtime_r(&value, &mut tm)
        }
    };
    if result.is_null() {
        return None;
    }

    let format: &[u8] = if local {
        b"%Y-%m-%d %H:%M:%S %z\0"
    } else {
        b"%Y-%m-%d %H:%M:%S UTC\0"
    };
    let length = unsafe {
        sys::strftime(
            output.as_mut_ptr().cast(),
            output.len(),
            format.as_ptr().cast(),
            &tm,
        )
    };
    if length == 0 { None } else { Some(length) }
}

pub fn synchronize_time() -> Result<(), &'static str> {
    unsafe {
        if SNTP_INITIALIZED.load(Ordering::Relaxed) {
            sys::sntp_set_sync_status(0);
            if !sys::sntp_restart() {
                return Err("Failed to restart SNTP service");
            }
        } else {
            sys::esp_sntp_setoperatingmode(0);
            sys::esp_sntp_setservername(0, b"pool.ntp.org\0".as_ptr().cast_mut().cast());
            sys::sntp_set_sync_status(0);
            sys::esp_sntp_init();
            SNTP_INITIALIZED.store(true, Ordering::Relaxed);
        }
    }

    let deadline = unsafe { sys::esp_timer_get_time() }.saturating_add(120_000_000);
    loop {
        if unsafe { sys::sntp_get_sync_status() } as i32 == 1 {
            TIME_SYNCHRONIZED.store(true, Ordering::Relaxed);
            return Ok(());
        }
        if unsafe { sys::esp_timer_get_time() } >= deadline {
            return Err("NTP synchronization timed out after 120 seconds");
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

extern "C" {
    fn tzset();
}
