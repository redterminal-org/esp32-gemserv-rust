use esp_idf_svc::sys;
use std::ffi::CStr;
use std::fmt::{self, Write as FmtWrite};
use std::mem;
use std::time::Duration;

use crate::clock;

const TABLE_NAME_WIDTH: usize = 20;
const MALLOC_CAPS_DEFAULT: u32 = sys::MALLOC_CAP_DEFAULT;
const MALLOC_CAPS_FREERTOS: u32 = sys::MALLOC_CAP_INTERNAL;

pub fn write_status(
    tls: *mut sys::esp_tls_t,
    buffer: &mut [u8],
    timeout: Duration,
) -> Result<(), &'static str> {
    let task_count = unsafe { sys::uxTaskGetNumberOfTasks() } as usize;
    if task_count == 0 {
        return Err("FreeRTOS returned an empty task list");
    }

    let mut tasks: Vec<sys::TaskStatus_t> = Vec::new();
    tasks.try_reserve_exact(task_count)
        .map_err(|_| "Not enough memory to collect task status")?;
    tasks.resize_with(task_count, || unsafe { mem::zeroed() });

    let count = unsafe {
        sys::uxTaskGetSystemState(tasks.as_mut_ptr(), task_count as u32, std::ptr::null_mut())
    } as usize;
    if count == 0 {
        return Err("Failed to collect FreeRTOS task status");
    }
    tasks.truncate(count);

    let mut widths = [5_usize, 8, 5, 3, 4];
    for task in &tasks {
        widths[1] = widths[1].max(decimal_width(task.uxCurrentPriority));
        widths[2] = widths[2].max(decimal_width(task.usStackHighWaterMark));
        widths[3] = widths[3].max(decimal_width(task.xTaskNumber));
        let core_id = unsafe { sys::xTaskGetAffinity(task.xHandle) };
        widths[4] = widths[4].max(signed_decimal_width(core_id));
    }

    let mut output = StatusWriter::new(tls, buffer, timeout);
    writeln!(&mut output, "# esp32-gemserv-rust - Server status")
        .map_err(|_| "Status response write failed")?;

    let mut date = [0_u8; 40];
    if let Some(length) = clock::format_local_time(&mut date) {
        let date = std::str::from_utf8(&date[..length]).unwrap_or("unavailable");
        writeln!(&mut output, "* Date: {date}").map_err(|_| "Status response write failed")?;
    } else {
        writeln!(&mut output, "* Date: unavailable (NTP not synchronized)")
            .map_err(|_| "Status response write failed")?;
    }

    let seconds = clock::uptime_ms() / 1_000;
    writeln!(
        &mut output,
        "* Time running: {}d{}h{}m{}s",
        seconds / 86_400,
        (seconds / 3_600) % 24,
        (seconds / 60) % 60,
        seconds % 60
    )
    .map_err(|_| "Status response write failed")?;

    let heap_size = unsafe { sys::heap_caps_get_total_size(MALLOC_CAPS_DEFAULT) };
    let free_heap = unsafe { sys::heap_caps_get_free_size(MALLOC_CAPS_DEFAULT) };
    let min_free_heap = unsafe { sys::heap_caps_get_minimum_free_size(MALLOC_CAPS_DEFAULT) };
    let max_alloc_heap = unsafe { sys::heap_caps_get_largest_free_block(MALLOC_CAPS_DEFAULT) };
    let freertos_free_heap = unsafe { sys::heap_caps_get_free_size(MALLOC_CAPS_FREERTOS) };
    let freertos_min_free_heap =
        unsafe { sys::heap_caps_get_minimum_free_size(MALLOC_CAPS_FREERTOS) };

    write!(&mut output, "\n## ESP32 Heap\n")
        .map_err(|_| "Status response write failed")?;
    writeln!(&mut output, "* ESP32 Heap Size: {heap_size}")
        .map_err(|_| "Status response write failed")?;
    writeln!(&mut output, "* ESP32 Free Heap: {free_heap}")
        .map_err(|_| "Status response write failed")?;
    writeln!(&mut output, "* ESP32 Min Free Heap: {min_free_heap}")
        .map_err(|_| "Status response write failed")?;
    writeln!(&mut output, "* ESP32 Max Alloc Heap: {max_alloc_heap}")
        .map_err(|_| "Status response write failed")?;

    write!(&mut output, "\n## FreeRTOS\n")
        .map_err(|_| "Status response write failed")?;
    writeln!(&mut output, "* FreeRTOS Free Heap (internal): {freertos_free_heap}")
        .map_err(|_| "Status response write failed")?;
    writeln!(
        &mut output,
        "* FreeRTOS Min Free Heap (internal): {freertos_min_free_heap}"
    )
    .map_err(|_| "Status response write failed")?;

    write!(&mut output, "\n## Running Tasks\n\n```text\n")
        .map_err(|_| "Status response write failed")?;
    write_table_border(&mut output, widths, '┌', '┬', '┐')
        .map_err(|_| "Status response write failed")?;
    write_table_header(&mut output, widths).map_err(|_| "Status response write failed")?;
    write_table_border(&mut output, widths, '├', '┼', '┤')
        .map_err(|_| "Status response write failed")?;

    for task in &tasks {
        let name = unsafe { CStr::from_ptr(task.pcTaskName.cast()) }
            .to_str()
            .unwrap_or("<invalid>");
        let core_id = unsafe { sys::xTaskGetAffinity(task.xHandle) };
        write!(&mut output, "│ ").map_err(|_| "Status response write failed")?;
        write_name(&mut output, name).map_err(|_| "Status response write failed")?;
        write!(
            &mut output,
            " │ {:>state_width$} │ {:>priority_width$} │ {:>stack_width$} │ {:>num_width$} │ {:>core_width$} │\n",
            task_state(task.eCurrentState as i32),
            task.uxCurrentPriority,
            task.usStackHighWaterMark,
            task.xTaskNumber,
            core_id,
            state_width = widths[0],
            priority_width = widths[1],
            stack_width = widths[2],
            num_width = widths[3],
            core_width = widths[4],
        )
        .map_err(|_| "Status response write failed")?;
    }

    write_table_border(&mut output, widths, '└', '┴', '┘')
        .map_err(|_| "Status response write failed")?;
    write!(&mut output, "```\n").map_err(|_| "Status response write failed")?;
    output.finish()
}

fn write_table_header(output: &mut StatusWriter<'_>, widths: [usize; 5]) -> fmt::Result {
    write!(
        output,
        "│ {:<name_width$} │ {:>state_width$} │ {:>priority_width$} │ {:>stack_width$} │ {:>num_width$} │ {:>core_width$} │\n",
        "Name",
        "State",
        "Priority",
        "Stack",
        "Num",
        "Core",
        name_width = TABLE_NAME_WIDTH,
        state_width = widths[0],
        priority_width = widths[1],
        stack_width = widths[2],
        num_width = widths[3],
        core_width = widths[4],
    )
}

fn write_table_border(
    output: &mut StatusWriter<'_>,
    widths: [usize; 5],
    left: char,
    junction: char,
    right: char,
) -> fmt::Result {
    output.write_char(left)?;
    write_border_segment(output, TABLE_NAME_WIDTH)?;
    output.write_char(junction)?;
    for (index, width) in widths.iter().enumerate() {
        write_border_segment(output, *width)?;
        output.write_char(if index == widths.len() - 1 { right } else { junction })?;
    }
    output.write_char('\n')
}

fn write_border_segment(output: &mut StatusWriter<'_>, width: usize) -> fmt::Result {
    for _ in 0..width + 2 {
        output.write_char('─')?;
    }
    Ok(())
}

fn write_name(output: &mut StatusWriter<'_>, name: &str) -> fmt::Result {
    let mut used = 0;
    for character in name.chars() {
        if used == TABLE_NAME_WIDTH {
            break;
        }
        output.write_char(character)?;
        used += 1;
    }
    for _ in used..TABLE_NAME_WIDTH {
        output.write_char(' ')?;
    }
    Ok(())
}

fn task_state(state: i32) -> char {
    match state {
        0 => 'X',
        1 => 'R',
        2 => 'B',
        3 => 'S',
        4 => 'D',
        _ => '?',
    }
}

fn decimal_width(mut value: u32) -> usize {
    let mut width = 1;
    while value >= 10 {
        value /= 10;
        width += 1;
    }
    width
}

fn signed_decimal_width(value: i32) -> usize {
    if value < 0 {
        decimal_width(value.unsigned_abs()) + 1
    } else {
        decimal_width(value as u32)
    }
}

struct StatusWriter<'a> {
    tls: *mut sys::esp_tls_t,
    buffer: &'a mut [u8],
    used: usize,
    timeout: Duration,
    failed: bool,
}

impl<'a> StatusWriter<'a> {
    fn new(tls: *mut sys::esp_tls_t, buffer: &'a mut [u8], timeout: Duration) -> Self {
        Self {
            tls,
            buffer,
            used: 0,
            timeout,
            failed: false,
        }
    }

    fn flush_buffer(&mut self) -> Result<(), &'static str> {
        if self.failed {
            return Err("Status response write failed");
        }
        if self.used == 0 {
            return Ok(());
        }
        crate::gemini::write_all(self.tls, &self.buffer[..self.used], self.timeout)?;
        self.used = 0;
        Ok(())
    }

    fn finish(mut self) -> Result<(), &'static str> {
        self.flush_buffer()
    }
}

impl FmtWrite for StatusWriter<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        if self.failed {
            return Err(fmt::Error);
        }
        let mut remaining = text.as_bytes();
        while !remaining.is_empty() {
            if self.used == self.buffer.len() {
                if self.flush_buffer().is_err() {
                    self.failed = true;
                    return Err(fmt::Error);
                }
            }
            let count = remaining.len().min(self.buffer.len() - self.used);
            self.buffer[self.used..self.used + count].copy_from_slice(&remaining[..count]);
            self.used += count;
            remaining = &remaining[count..];
        }
        Ok(())
    }
}
