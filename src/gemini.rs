use esp_idf_svc::sys;
use std::time::Duration;

pub fn read_request_line_into<'a>(
    tls: *mut sys::esp_tls_t,
    buffer: &'a mut [u8],
    timeout: Duration,
) -> Result<&'a [u8], &'static str> {
    if buffer.len() < 2 {
        return Err("Request line buffer too small");
    }

    let deadline = unsafe { sys::esp_timer_get_time() }
        .saturating_add(timeout.as_micros().min(i64::MAX as u128) as i64);

    let mut used = 0;
    let mut byte = [0_u8; 1];

    loop {
        if unsafe { sys::esp_timer_get_time() } >= deadline {
            return Err("Request timed out");
        }

        let result = unsafe { sys::esp_tls_conn_read(tls, byte.as_mut_ptr().cast(), byte.len()) };

        if result == 1 {
            if used >= buffer.len() {
                return Err("Request line too long");
            }

            buffer[used] = byte[0];
            used += 1;

            if used >= 2 && &buffer[used - 2..used] == b"\r\n" {
                return Ok(&buffer[..used]);
            }

            if used == buffer.len() {
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

pub fn write_all(
    tls: *mut sys::esp_tls_t,
    data: &[u8],
    timeout: Duration,
) -> Result<(), &'static str> {
    let deadline = unsafe { sys::esp_timer_get_time() }
        .saturating_add(timeout.as_micros().min(i64::MAX as u128) as i64);

    let mut written = 0;

    while written < data.len() {
        if unsafe { sys::esp_timer_get_time() } >= deadline {
            return Err("TLS write timed out");
        }

        let result = unsafe {
            sys::esp_tls_conn_write(tls, data[written..].as_ptr().cast(), data.len() - written)
        };

        if result > 0 {
            written += result as usize;
        } else if result == sys::ESP_TLS_ERR_SSL_WANT_READ as isize
            || result == sys::ESP_TLS_ERR_SSL_WANT_WRITE as isize
        {
            std::thread::sleep(Duration::from_millis(10));
        } else if result == sys::ESP_TLS_ERR_SSL_TIMEOUT as isize {
            return Err("TLS write timed out");
        } else {
            return Err("TLS write failed");
        }
    }

    Ok(())
}
