use esp_idf_svc::sys;
use std::time::Duration;

pub fn read_request_line(
    tls: *mut sys::esp_tls_t,
    max_bytes: usize,
    timeout: Duration,
) -> Result<Vec<u8>, &'static str> {
    let deadline = unsafe { sys::esp_timer_get_time() }
        .saturating_add(timeout.as_micros().min(i64::MAX as u128) as i64);

    let mut line = Vec::with_capacity(max_bytes.min(128));
    let mut byte = [0_u8; 1];

    loop {
        if unsafe { sys::esp_timer_get_time() } >= deadline {
            return Err("Request timed out");
        }

        let result = unsafe { sys::esp_tls_conn_read(tls, byte.as_mut_ptr().cast(), byte.len()) };

        if result == 1 {
            line.push(byte[0]);

            if line.ends_with(b"\r\n") {
                return Ok(line);
            }

            if line.len() >= max_bytes {
                return Err("Request line too long");
            }
        } else if result == 0 {
            return Err("Connection closed while reading request");
        } else if result == sys::ESP_TLS_ERR_SSL_WANT_READ as isize
            || result == sys::ESP_TLS_ERR_SSL_WANT_WRITE as isize
        {
            std::thread::sleep(Duration::from_millis(10));
        } else if result == sys::ESP_TLS_ERR_SSL_TIMEOUT as isize {
            return Err("TLS read timed out");
        } else {
            return Err("TLS read failed");
        }
    }
}
