# ESP32 Gemini Server (Rust)

A Gemini protocol server written in Rust for the classic ESP32, using ESP-IDF. The server serves Gemini resources from a LittleFS filesystem over TLS.

## Features

- Gemini protocol support over TLS.
- Wi-Fi connectivity with reconnection and exponential backoff.
- Two worker tasks, with one worker assigned to each ESP32 core.
- A bounded request queue for handling incoming connections while workers are busy.
- Configurable hostname, port, TLS settings, request limits, and response chunk size.
- Files served from LittleFS, including directory index files.
- MIME type detection and Gemini response status handling.
- Streaming file responses to limit memory usage.

## Server architecture

The server uses one listener thread and two worker tasks to handle incoming requests concurrently.

1. **Listener thread:** Accepts incoming connections and places requests in a bounded queue.
2. **Request queue:** Holds up to four requests while the workers are busy.
3. **Worker tasks:** Two workers process requests concurrently, with one worker running on each ESP32 core. When a worker becomes available, it takes the next queued request for processing.
4. **Queue full:** When both workers are occupied and all four queue slots are full, additional requests are rejected rather than queued.

The server can therefore accommodate **up to six requests at once**: two requests being processed by the workers and four waiting in the queue. This is the combined processing and queue capacity, not six worker threads.

## Hardware and partition layout

The project targets the classic ESP32. The partition table defines the following layout:

| Partition | Offset | Size |
|---|---:|---:|
| NVS | `0x9000` | `0x6000` |
| PHY initialization data | `0xF000` | `0x1000` |
| Factory application | `0x10000` | `0x200000` |
| LittleFS | `0x210000` | `0x1F0000` |

The LittleFS partition is separate from the firmware application. Firmware and filesystem images must be flashed to their respective locations.

## Repository layout

```text
.
├── Cargo.toml
├── Cargo.lock
├── build.rs
├── config.dist.toml
├── partitions.csv
├── sdkconfig.defaults
├── rust-toolchain.toml
├── INSTALL_TOOLCHAIN.md
├── src/
│   ├── main.rs
│   ├── config.rs
│   ├── content_path.rs
│   ├── gemini.rs
│   ├── gemini_parser.rs
│   ├── gemini_response.rs
│   ├── server.rs
│   ├── storage.rs
│   ├── tls.rs
│   └── wifi.rs
└── littlefs/
    └── gemini/
        ├── index.gmi
        ├── README.gmi
        ├── INSTALL_TOOLCHAIN.gmi
        └── LICENSE.txt
```

## Prerequisites

Install the Rust and ESP-IDF toolchain required by this project. The repository uses the ESP Rust toolchain specified in `rust-toolchain.toml`.

Follow [INSTALL_TOOLCHAIN.md](INSTALL_TOOLCHAIN.md) for the toolchain setup instructions.

You will also need the relevant ESP-IDF build tools, a supported ESP32 board, and the utilities used to generate and flash the LittleFS image.

## Cloning the repository

At first you need to clone the Repository including the sub-modules to your current working directory.

```
git clone --recurse-submodules https://github.com/redterminal-org/esp32-gemserv-rust
```

## Configuration

Start with the distributed configuration template, `config.dist.toml`, and create the configuration file expected by the application.

The configuration covers the following areas:

- **Wi-Fi:** SSID and password.
- **Server:** Gemini hostname, listening port, and the GPIO used for the request status LED.
- **TLS:** Certificate path, private-key path, and handshake timeout.
- **Gemini:** Request-line size limit, response chunk size, request timeout, and optional language metadata.

Example configuration:

```toml
[wifi]
ssid = "your-wifi-ssid"
password = "your-wifi-password"

[server]
hostname = "gemini.example.net"
port = 1965
status_led = 2

[tls]
certificate_path = "/littlefs/certs/server.crt"
private_key_path = "/littlefs/certs/server.key"
handshake_timeout_seconds = 10

[gemini]
request_line_max_bytes = 1024
response_chunk_size = 2048
request_timeout_seconds = 10
lang = "en"
```

Use the actual field names and supported values from `config.dist.toml` when creating your configuration. Keep credentials and private keys out of version control.

## TLS certificates

The Gemini protocol normally uses TLS on TCP port `1965`.

The firmware expects these files on LittleFS:

```text
/littlefs/certs/server.crt
/littlefs/certs/server.key
```

Create a local directory for certificate staging:

```bash
mkdir -p certs
```

Generate a self-signed certificate and private key for local testing:

```bash
openssl req -x509 -newkey ec \
  -pkeyopt ec_paramgen_curve:prime256v1 \
  -sha256 -days 3650 \
  -nodes \
  -keyout certs/server.key \
  -out certs/server.crt \
  -subj "/CN=esp32-gemini.local" \
  -addext "subjectAltName=DNS:esp32-gemini.local"
```

This creates an ECDSA certificate using the `prime256v1` curve.

The `-nodes` option leaves the private key unencrypted so the firmware can load it without an interactive password prompt. Protect the private key and never publish it.

A self-signed certificate is suitable for development, but clients will not automatically trust it. Configure trust in your Gemini client or use a certificate appropriate for your deployment.

If your client connects using a hostname, make sure the certificate identity matches that hostname. Some clients require a Subject Alternative Name (SAN); add the required DNS or IP SAN when generating the certificate for your environment.

## Preparing the LittleFS image

Place the content to be served under `littlefs/gemini/`. The initial example resource is:

```text
littlefs/gemini/index.gmi
```

The file contains a simple Gemini greeting. Add other `.gmi` documents and supported resources under this directory as needed.

The generated filesystem image must match the LittleFS partition defined in `partitions.csv`. The partition begins at `0x210000` and has a size of `0x1F0000`.

Use a compatible LittleFS image-generation tool. For example, where supported by your installed `mklittlefs` version, the image-generation command can be adapted from:

```bash
mklittlefs \
  -c littlefs \
  -b 4096 \
  -p 256 \
  -s 2031616 \
  littlefs.bin
```

Check the image-generation tool's options and the filesystem geometry expected by the project's LittleFS integration before relying on these parameters.

## Building

After installing and configuring the toolchain, build the firmware in release mode:

```bash
cargo build --release
```

The expected ESP32 target output is:

```text
target/xtensa-esp32-espidf/release/esp32-gemserv-rust
```

If the build fails, check the toolchain setup in `INSTALL_TOOLCHAIN.md`, the ESP-IDF environment, and the availability of the project's ESP-IDF component dependencies.

## Flashing

Flash the application using the ESP flashing tools. For example:

```bash
espflash flash \
  --monitor \
  --port /dev/ttyUSB0 \
  --chip esp32 \
  --partition-table partitions.csv \
  target/xtensa-esp32-espidf/release/esp32-gemserv-rust
```

Replace `/dev/ttyUSB0` with the serial port for your board.

The LittleFS image is flashed separately to the filesystem partition at offset `0x210000`. Use a flashing command appropriate for your installed tools and verify the partition table and image size before writing to the device.

## Request handling

The server follows the Gemini protocol and processes requests using the listener-and-worker architecture described above.

- The listener accepts connections and queues requests for processing.
- Two worker tasks handle requests concurrently.
- The request queue holds at most four waiting requests.
- Requests that cannot be queued because the queue is full are rejected.
- Request lines are parsed and validated before the requested resource is opened.
- Requests must use the `gemini://` scheme and a valid authority.
- User information and URI fragments are rejected.
- A missing path is treated as `/`.
- Query parameters are handled separately from the filesystem path.
- Directory requests are resolved to an index document.
- Files are served from the configured Gemini content directory.
- File data is streamed in bounded chunks instead of being loaded entirely into memory.

## Gemini responses and MIME types

The server builds Gemini response headers and maps supported file extensions to MIME types.

Text responses use UTF-8 charset metadata where appropriate. Gemtext responses can include language metadata when the corresponding configuration option is set.

The implementation includes Gemini protocol status handling for successful responses, missing resources, temporary failures, and other request or server errors.

## Content paths

Content is served from the Gemini content tree on LittleFS. For example, a document stored at:

```text
/littlefs/gemini/docs/index.gmi
```

can be requested using a corresponding Gemini URL such as:

```text
gemini://gemini.example.net/docs/
```

Directory paths are resolved to index documents. Resource paths are validated and resolved within the content tree rather than being used as unrestricted filesystem paths.

## Wi-Fi behavior

The device connects using the configured Wi-Fi credentials. The Wi-Fi module includes reconnection handling with exponential backoff, starting at one second and increasing up to thirty seconds.

Check the configured network credentials and the device's serial output if it cannot connect.

## Security considerations

- Protect the TLS private key and Wi-Fi credentials.
- Do not commit real credentials or private keys to the repository.
- Use a certificate appropriate for the server's hostname.
- Keep firmware, partition layout, and filesystem image sizes consistent.
- The bounded request queue limits the number of waiting requests; requests arriving when the queue is full are rejected.
- Expose the service only on networks where the device and its resource content can be appropriately secured.

## Troubleshooting

**The firmware does not build**

Verify the ESP Rust toolchain and ESP-IDF setup described in `INSTALL_TOOLCHAIN.md`. Check that all required ESP-IDF components and dependencies are available.

**Wi-Fi does not connect**

Check the configured SSID and password, Wi-Fi coverage, and serial output.

**TLS initialization fails**

Check that the certificate and private key exist at the configured paths, are readable, and correspond to each other.

**A resource returns a not-found response**

Verify the requested URL, the resource's location in the Gemini content tree, and the filesystem image flashed to the device. Directory requests should resolve to an index document.

**Requests are rejected under load**

The server has two active workers and four queue slots. When both workers are busy and the queue is full, new requests are rejected. This is expected behavior for the bounded queue.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
