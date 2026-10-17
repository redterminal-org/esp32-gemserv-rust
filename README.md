# ESP32 Gemini Server (Rust)

A Gemini protocol server for the classic ESP32, written in Rust and
built on ESP-IDF. The project uses LittleFS for configuration, TLS
credentials, and Gemini content.

> **Development status:** The current firmware mounts LittleFS, reads
> its configuration and TLS files, and connects to Wi-Fi. The Gemini
> protocol server is not implemented yet. Do not treat this revision
> as a working Gemini server.

## Features and design goals

- Target: classic ESP32 with 4 MiB flash
- Rust application using ESP-IDF
- Wi-Fi client with automatic reconnection and exponential backoff
- LittleFS filesystem for configuration, certificates, and public content
- TLS using a certificate and private key stored in LittleFS
- Planned Gemini server on TCP/TLS port 1965
- Planned two worker tasks pinned to separate CPU cores
- Planned queue for up to four pending connections
- Planned streaming of content in bounded chunks

Items marked as planned are not implemented unless stated otherwise.

## Hardware

The current target is a classic ESP32 board with 4 MiB of flash.

The firmware is configured to run the CPU at 240 MHz. Actual frequency
and available memory depend on the chip, board, and ESP-IDF configuration.

## Repository layout

```text
.
├── Cargo.toml
├── Cargo.lock
├── sdkconfig.defaults
├── partitions.csv
├── config.dist.toml
├── src/
│   ├── main.rs
│   └── config.rs
├── certs/                  # Local certificate staging; do not commit keys
└── .build/
    └── littlefs/           # Local LittleFS image staging
```

Some generated files and directories may be ignored by Git.

## Prerequisites

Set up the Rust and ESP-IDF build environment by following
[INSTALL_TOOLCHAIN.md](INSTALL_TOOLCHAIN.md).

You will also need:

- Git
- A USB data cable and a supported USB-to-UART interface
- Access to the ESP32 serial device
- OpenSSL to generate a self-signed TLS certificate
- A LittleFS image creation tool compatible with the project's build process

On Linux, the serial device is commonly `/dev/ttyUSB0` or `/dev/ttyACM0`.
Your device name may differ.

## 1. Clone the repository

```bash
git clone https://github.com/redterminal-org/esp32-gemserv-rust.git
cd esp32-gemserv-rust
```

## 2. Configure the project

Create a private local configuration from the distributed template:

```bash
cp config.dist.toml config.toml
```

Edit `config.toml` and set your Wi-Fi credentials and desired server settings.
Never commit your real configuration or Wi-Fi password.

The firmware currently reads the configuration from:

```text
/littlefs/config.toml
```

The exact configuration keys and supported values are defined in
`src/config.rs` and `config.dist.toml`. Keep the two files consistent.

## 3. Generate a TLS certificate

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

The `-nodes` option leaves the private key unencrypted so the firmware
can load it without an interactive password prompt. Protect the private
key and never publish it.

A self-signed certificate is suitable for development, but clients will
not automatically trust it. Configure trust in your Gemini client or
use a certificate appropriate for your deployment.

If your client connects using a hostname, make sure the certificate
identity matches that hostname. Some clients require a Subject
Alternative Name (SAN); add the required DNS or IP SAN when generating
the certificate for your environment.

## 4. Prepare the LittleFS contents

The firmware needs at least the following files inside its filesystem:

```text
config.toml
certs/
├── server.crt
└── server.key
gemini/
└── index.gmi
```

Create a local staging directory:

```bash
mkdir -p .build/littlefs/certs
mkdir -p .build/littlefs/gemini
```

Copy the local configuration and certificates into the staging directory:

```bash
cp config.toml .build/littlefs/config.toml
cp certs/server.crt .build/littlefs/certs/server.crt
cp certs/server.key .build/littlefs/certs/server.key
```

Create a basic Gemini home page:

```bash
cat > .build/littlefs/gemini/index.gmi <<'EOF'
# My ESP32 Gemini server

Welcome to my Gemini site.

=> /docs/ Documentation
EOF
```

The staging directory is local build input. Do not commit private
configuration, private keys, or generated filesystem images.

### Add your own Gemini content

Put public files under:

```text
.build/littlefs/gemini/
```

For example:

```text
.build/littlefs/gemini/
├── index.gmi
├── about.gmi
├── docs/
│   ├── index.gmi
│   └── setup.gmi
└── images/
    └── diagram.png
```

Gemini documents normally use the `.gmi` extension and Gemtext syntax.
Links can be written like this:

```text
=> /about.gmi About this site
=> /docs/ Documentation
```

Only publish files you intend to make publicly accessible. Never place
private keys, Wi-Fi credentials, or other secrets in the public content
directory.

The intended URL mapping is:

- `/` → `gemini/index.gmi`
- `/docs` and `/docs/` → `gemini/docs/index.gmi`
- `/docs/setup.gmi` → `gemini/docs/setup.gmi`

This mapping describes the planned server behavior; it will only work
once the Gemini request handler has been implemented.

### Build the LittleFS image

The LittleFS partition starts at `0x210000` and has size `0x1F0000`
(1,966,080 bytes).

Create a LittleFS image from `.build/littlefs/` using the image tool and
parameters expected by this project's ESP-IDF/LittleFS integration.
The output is typically named `littlefs.bin`.

For example, if the project's selected `mklittlefs` version uses the
usual 256-byte read/program size and 4096-byte block size, the command
may look like this:

```bash
# Create littlfs.bin image
mklittlefs -c .build/littlefs \
  -p 256 -b 4096 -s 0x1F0000 \
  littlefs.bin

# Flash littlefs.bin to your ESP32
espflash write-bin --port /dev/ttyUSB0 \
  --chip esp32 0x210000 littlefs.bin
```

Verify these geometry parameters against the LittleFS tool and ESP-IDF
component actually used by the project before relying on this command.
Do not use a filesystem image larger than the partition.

## 5. Build the firmware

Load the environment described in `INSTALL_TOOLCHAIN.md`, then run:

```bash
cargo build --release
```

The release binary is expected at:

```text
target/xtensa-esp32-espidf/release/esp32-gemserv-rust
```

Warnings about currently unused server, TLS, or Gemini configuration
fields are expected while those parts of the implementation are pending.

## 6. Flash the firmware

Check the serial device name first. The following example uses
`/dev/ttyUSB0`:

```bash
espflash flash \
  --monitor \
  --port /dev/ttyUSB0 \
  --chip esp32 \
  --partition-table partitions.csv \
  target/xtensa-esp32-espidf/release/esp32-gemserv-rust
```

**Important:** Always supply `--partition-table partitions.csv` when
flashing this project. The custom partition layout includes the LittleFS
partition. Do not replace it with an unrelated default partition table.

The command above flashes the firmware. Flash the generated LittleFS
image to the `littlefs` partition using a method compatible with your
installed `espflash` version, or use the project's documented flashing
script if one is added. Verify the destination offset is `0x210000`;
never flash the filesystem image at the application offset.

After flashing, the serial monitor should show messages similar to:

```text
LittleFS mounted successfully
Configuration loaded successfully
TLS certificate file is readable
TLS private key file is readable
WiFi reconnect task started
WiFi started
WiFi connected
```

Exact log messages may change as development continues.

## Security notes

- Never commit Wi-Fi passwords or private keys.
- Keep `config.toml`, `certs/server.key`, and generated images out of Git.
- Use a self-signed certificate only where its trust implications are
  understood.
- Publish only intentionally public content under `gemini/`.
- Review the partition table before flashing.
- Do not assume the Gemini server is available until its request
  handling and TLS listener have been implemented and tested.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
