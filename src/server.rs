use esp_idf_svc::sys;
use std::fs::File;
use std::io::Read;
use std::net::TcpListener;
use std::os::fd::IntoRawFd;
use std::sync::Mutex;
use std::time::Duration;

use crate::config::GeminiConfig;
use crate::gemini_response::GeminiStatus;

const QUEUE_CAPACITY: u32 = 4;
const WORKER_STACK_BYTES: u32 = 8192;
const WORKER_PRIORITY: u32 = 5;
const CORE_0: i32 = 0;
const CORE_1: i32 = 1;
const QUEUE_SEND_TO_BACK: i32 = 0;
const QUEUE_WAIT_FOREVER: u32 = u32::MAX;

struct RequestActivity {
    count: usize,
    pin: i32,
}

static REQUEST_ACTIVITY: Mutex<RequestActivity> =
    Mutex::new(RequestActivity { count: 0, pin: 2 });

struct WorkerContext {
    queue: sys::QueueHandle_t,
    tls_config: sys::esp_tls_cfg_server_t,
    hostname: &'static str,
    port: u16,
    handshake_timeout_seconds: u64,
    gemini_config: &'static GeminiConfig,
    request_buffer: Vec<u8>,
    response_buffer: Vec<u8>,
}

pub fn run(
    hostname: &str,
    port: u16,
    status_led: i32,
    certificate_path: &str,
    private_key_path: &str,
    handshake_timeout_seconds: u64,
    gemini_config: &GeminiConfig,
    boot_log_path: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    initialize_status_led(status_led)?;

    let tls_owner = crate::tls::ServerTlsConfig::load(certificate_path, private_key_path)?;

    let tls_owner: &'static crate::tls::ServerTlsConfig = Box::leak(Box::new(tls_owner));

    let hostname: &'static str = Box::leak(hostname.to_owned().into_boxed_str());

    let gemini_config: &'static GeminiConfig = Box::leak(Box::new(gemini_config.clone()));

    let listener = TcpListener::bind(("0.0.0.0", port))?;
    log::info!("TLS listener started on port {}", port);

    let queue =
        unsafe { sys::xQueueGenericCreate(QUEUE_CAPACITY, std::mem::size_of::<i32>() as u32, 0) };

    if queue.is_null() {
        return Err("Failed to create connection queue".into());
    }

    let request_capacity = gemini_config.request_line_max_bytes.max(2);
    let response_capacity = gemini_config.response_chunk_size.clamp(1, 8192);

    let mut created_tasks: Vec<(sys::TaskHandle_t, *mut WorkerContext)> = Vec::with_capacity(2);

    for (name, core_id) in [
        (b"gemini_core0\0".as_slice(), CORE_0),
        (b"gemini_core1\0".as_slice(), CORE_1),
    ] {
        let worker = WorkerContext {
            queue,
            tls_config: tls_owner.copy_config(),
            hostname,
            port,
            handshake_timeout_seconds,
            gemini_config,
            request_buffer: vec![0_u8; request_capacity],
            response_buffer: vec![0_u8; response_capacity],
        };

        let worker_ptr = Box::into_raw(Box::new(worker));
        let mut task_handle: sys::TaskHandle_t = std::ptr::null_mut();

        let result = unsafe {
            sys::xTaskCreatePinnedToCore(
                Some(worker_entry),
                name.as_ptr().cast(),
                WORKER_STACK_BYTES,
                worker_ptr.cast(),
                WORKER_PRIORITY,
                &mut task_handle,
                core_id,
            )
        };

        if result != 1 {
            unsafe {
                drop(Box::from_raw(worker_ptr));

                for (handle, context_ptr) in created_tasks.drain(..) {
                    sys::vTaskDelete(handle);
                    drop(Box::from_raw(context_ptr));
                }

                sys::vQueueDelete(queue);
            }

            return Err(format!("Failed to create worker for core {}", core_id).into());
        }

        created_tasks.push((task_handle, worker_ptr));
        log::info!("Gemini worker started on core {}", core_id);
    }

    if let Some(path) = boot_log_path.filter(|path| !path.trim().is_empty()) {
        if let Err(err) = crate::storage::append_boot_log(path) {
            log::error!("Failed to append boot log: {}", err);
        }
    }

    loop {
        let (stream, peer) = match listener.accept() {
            Ok(connection) => connection,
            Err(err) => {
                log::warn!("TCP accept failed: {}", err);
                continue;
            }
        };

        let socket_fd = stream.into_raw_fd();

        change_request_count(true);

        let result = unsafe {
            sys::xQueueGenericSend(
                queue,
                (&socket_fd as *const i32).cast(),
                0,
                QUEUE_SEND_TO_BACK,
            )
        };

        if result != 1 {
            change_request_count(false);
            unsafe {
                sys::close(socket_fd);
            }
            log::warn!("Connection from {} rejected: queue is full", peer);
        } else {
            log::debug!("Connection from {} queued", peer);
        }
    }
}

unsafe extern "C" fn worker_entry(arg: *mut std::ffi::c_void) {
    let worker = unsafe { &mut *(arg as *mut WorkerContext) };

    loop {
        let mut socket_fd: i32 = -1;

        let received = unsafe {
            sys::xQueueReceive(
                worker.queue,
                (&mut socket_fd as *mut i32).cast(),
                QUEUE_WAIT_FOREVER,
            )
        };

        if received != 1 {
            continue;
        }

        process_connection(worker, socket_fd);
        change_request_count(false);
    }
}


fn initialize_status_led(pin: i32) -> Result<(), Box<dyn std::error::Error>> {
    let reset_result = unsafe { sys::gpio_reset_pin(pin) };
    if reset_result != 0 {
        return Err(format!("Failed to reset status LED GPIO {}", pin).into());
    }

    let direction_result =
        unsafe { sys::gpio_set_direction(pin, sys::gpio_mode_t_GPIO_MODE_OUTPUT) };
    if direction_result != 0 {
        return Err(format!("Failed to configure status LED GPIO {} as output", pin).into());
    }

    let level_result = unsafe { sys::gpio_set_level(pin, 0) };
    if level_result != 0 {
        return Err(format!("Failed to turn off status LED GPIO {}", pin).into());
    }

    let mut activity = REQUEST_ACTIVITY
        .lock()
        .map_err(|_| "Failed to lock request activity state")?;
    activity.pin = pin;
    activity.count = 0;

    log::info!("Request status LED initialized on GPIO {}", pin);
    Ok(())
}

fn change_request_count(increment: bool) {
    let mut activity = match REQUEST_ACTIVITY.lock() {
        Ok(activity) => activity,
        Err(poisoned) => poisoned.into_inner(),
    };

    if increment {
        activity.count = activity.count.saturating_add(1);
    } else {
        activity.count = activity.count.saturating_sub(1);
    }

    let level = if activity.count > 0 { 1 } else { 0 };
    let result = unsafe { sys::gpio_set_level(activity.pin, level) };
    if result != 0 {
        log::error!("Failed to update status LED GPIO {}: {}", activity.pin, result);
    }
}

fn process_connection(worker: &mut WorkerContext, socket_fd: i32) {
    let tls = unsafe { sys::esp_tls_init() };

    if tls.is_null() {
        log::error!("Failed to allocate TLS session");
        unsafe {
            sys::close(socket_fd);
        }
        return;
    }

    let mut tls_established = false;

    let result = (|| -> Result<(), &'static str> {
        set_socket_timeout(socket_fd, worker.handshake_timeout_seconds)
            .map_err(|_| "Failed to configure TLS handshake timeout")?;

        let handshake_result =
            unsafe { sys::esp_tls_server_session_create(&mut worker.tls_config, socket_fd, tls) };

        if handshake_result != 0 {
            log::warn!("TLS handshake failed with error {}", handshake_result);
            return Ok(());
        }

        tls_established = true;

        let config = worker.gemini_config;
        let timeout = Duration::from_secs(config.request_timeout_seconds);

        set_socket_timeout(socket_fd, config.request_timeout_seconds)
            .map_err(|_| "Failed to configure request timeout")?;

        let request_line =
            match crate::gemini::read_request_line_into(tls, &mut worker.request_buffer, timeout) {
                Ok(line) => line,
                Err(err) => {
                    log::warn!("Failed to read Gemini request: {}", err);
                    send_error(tls, GeminiStatus::BadRequest, "Invalid request", timeout)?;
                    return Ok(());
                }
            };

        let uri = match crate::gemini_parser::parse_request_line(
            request_line,
            config.request_line_max_bytes,
        ) {
            Ok(uri) => uri,
            Err(err) => {
                log::warn!("Invalid Gemini request: {}", err);
                send_error(tls, GeminiStatus::BadRequest, "Invalid request", timeout)?;
                return Ok(());
            }
        };

        let request_path =
            match crate::gemini_parser::extract_request_path(&uri, worker.hostname, worker.port) {
                Ok(path) => path,
                Err(err) => {
                    log::warn!("Rejected Gemini request: {}", err);
                    send_error(
                        tls,
                        GeminiStatus::BadRequest,
                        "Invalid request URI",
                        timeout,
                    )?;
                    return Ok(());
                }
            };

        let resolved_path = match crate::content_path::resolve_url_path("/", &request_path) {
            Ok(path) => path,
            Err(err) => {
                log::warn!("Rejected URL path: {}", err);
                send_error(tls, GeminiStatus::BadRequest, "Invalid path", timeout)?;
                return Ok(());
            }
        };

        if resolved_path == "/status" {
            if config.statuspage == Some(true) {
                crate::gemini::write_all(
                    tls,
                    b"20 text/gemini; charset=utf-8\r\n",
                    timeout,
                )?;
                if let Err(err) = crate::status::write_status(
                    tls,
                    &mut worker.response_buffer,
                    timeout,
                ) {
                    log::warn!("Failed to generate status page: {}", err);
                }
            } else {
                send_error(tls, GeminiStatus::NotFound, "Not found", timeout)?;
            }
            return Ok(());
        }

        let file_path = match crate::content_path::content_file_path(&resolved_path) {
            Ok(path) => path,
            Err(err) => {
                log::warn!("Rejected content path: {}", err);
                send_error(tls, GeminiStatus::BadRequest, "Invalid path", timeout)?;
                return Ok(());
            }
        };

        let mut file = match File::open(&file_path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                send_error(tls, GeminiStatus::NotFound, "Not found", timeout)?;
                return Ok(());
            }
            Err(err) => {
                log::error!("Failed to open {}: {}", file_path, err);
                send_error(
                    tls,
                    GeminiStatus::TemporaryFailure,
                    "Unable to read content",
                    timeout,
                )?;
                return Ok(());
            }
        };

        let header = crate::gemini_response::success_header(&file_path, config)
            .map_err(|_| "Failed to construct Gemini response header")?;

        crate::gemini::write_all(tls, &header, timeout)?;

        loop {
            let count = match file.read(&mut worker.response_buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(err) => {
                    log::error!("Failed while reading {}: {}", file_path, err);
                    return Err("Content read failed");
                }
            };

            crate::gemini::write_all(tls, &worker.response_buffer[..count], timeout)?;
        }

        log::info!("Gemini response sent: URI={}, file={}", uri, file_path);
        Ok(())
    })();

    if let Err(err) = result {
        log::warn!("Connection processing failed: {}", err);
    }

    if tls_established {
        if let Err(err) = tls_close_notify(
            tls,
            Duration::from_secs(worker.gemini_config.request_timeout_seconds),
        ) {
            log::warn!("Failed to send TLS close_notify: {}", err);
        }
    }

    unsafe {
        sys::esp_tls_server_session_delete(tls);
        sys::close(socket_fd);
    }
}

fn send_error(
    tls: *mut sys::esp_tls_t,
    status: GeminiStatus,
    message: &str,
    timeout: Duration,
) -> Result<(), &'static str> {
    let header = status.response_header(message)?;
    crate::gemini::write_all(tls, &header, timeout)
}

fn set_socket_timeout(
    socket_fd: i32,
    timeout_seconds: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = sys::timeval {
        tv_sec: timeout_seconds.min(i64::MAX as u64) as i64,
        tv_usec: 0,
    };

    for option in [sys::SO_RCVTIMEO, sys::SO_SNDTIMEO] {
        let result = unsafe {
            sys::lwip_setsockopt(
                socket_fd,
                sys::SOL_SOCKET as i32,
                option as i32,
                (&timeout as *const sys::timeval).cast(),
                std::mem::size_of_val(&timeout) as sys::socklen_t,
            )
        };

        if result != 0 {
            return Err(format!("Failed to set socket timeout (option {option})").into());
        }
    }

    Ok(())
}

fn tls_close_notify(tls: *mut sys::esp_tls_t, timeout: Duration) -> Result<(), &'static str> {
    let ssl = unsafe { sys::esp_tls_get_ssl_context(tls) };

    if ssl.is_null() {
        return Err("Failed to get TLS SSL context");
    }

    let deadline = unsafe { sys::esp_timer_get_time() }
        .saturating_add(timeout.as_micros().min(i64::MAX as u128) as i64);

    loop {
        let result =
            unsafe { sys::mbedtls_ssl_close_notify(ssl.cast::<sys::mbedtls_ssl_context>()) };

        if result == 0 {
            return Ok(());
        }

        if result != sys::MBEDTLS_ERR_SSL_WANT_READ && result != sys::MBEDTLS_ERR_SSL_WANT_WRITE {
            log::warn!("TLS close_notify failed with error {}", result);
            return Err("TLS close_notify failed");
        }

        if unsafe { sys::esp_timer_get_time() } >= deadline {
            return Err("TLS close_notify timed out");
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}
