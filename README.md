# omen-fan

A simple utility to manually control the fans of HP Omen laptops (and some Victus models).  
Includes a systemd service for automatic, temperature-based fan control—because the default BIOS control is often insufficient.

- **Tested on:** Omen 16-c0140AX, Omen 16-n0xxx series, Omen 15-dc10xxxx, and some Victus models.
- **Written in Rust.**
- **Supports:** Manual and automatic fan control, BIOS mode switching, and boost mode via sysfs.

---

## ⚠️ WARNING

- Forcing this program to run on incompatible laptops may cause hardware damage. **Use at your own risk.**
- Max fan speeds are set based on the "Boost" state. Increasing them is not recommended and offers minimal thermal benefit.

---

## Features

- Custom fan curves for multiple modes (`default`, `performance`, `cool`) via `fan_config.toml`
- Automatic mode switching and live config reload
- BIOS mode control (Default, Performance, Cool)
- Direct EC register access for precise control
- Systemd service integration for background operation
- Easy installation with provided script and prebuilt releases

---

## Installation

### 1. **Download the latest release**

Go to [Releases](https://github.com/alou-S/omen-fan/releases) and download the latest tarball for your architecture.

### 2. **Extract and install**

```sh
tar -xzf omen-fan-x86_64-unknown-linux-gnu.tar.gz
cd omen-fan
sudo ./install.sh
```

This will:
- Copy the binary and config to `/usr/local/bin/`
- Install the systemd service to `/etc/systemd/system/`
- Enable and start the `omen-fan` service

### 3. **Configure**

Edit `/usr/local/bin/fan_config.toml` to adjust your fan curves or switch modes.  
The daemon will reload changes automatically.

---

## Usage

- **Command-line:**  
  Run `omen-fan --help` to see available options (currently only `--config` to specify the config file path).

- **Service:**  
  The systemd service runs in the background and manages fan speeds automatically.

---

## Building from Source

1. **Install Rust** from [the official website](https://rustup.rs/).
2. **Clone the repository:**
   ```sh
   git clone https://github.com/alou-S/omen-fan.git
   cd omen-fan
   ```
3. **Build:**
   ```sh
   cargo build --release
   ```
   Or, to enable the `acpi_ec` feature:
   ```sh
   cargo build --release --features acpi_ec
   ```

4. **Install manually (if not using the install script):**
   ```sh
   sudo cp target/release/omen-fan /usr/local/bin/
   sudo cp src/fan_config.toml /usr/local/bin/
   sudo cp src/omen-fan.service /etc/systemd/system/
   sudo systemctl daemon-reload
   sudo systemctl enable omen-fan.service
   sudo systemctl start omen-fan.service
   ```

---

## Documentation

- Use `omen-fan --help` for CLI options.
- EC Probe documentation: [docs/probes.md](https://github.com/alou-S/omen-fan/blob/main/docs/probes.md)

---

## Notes

- The install script and release tarball now provide all necessary files and set up the service automatically.
- You no longer need to manually copy files or edit service paths.

---

**Enjoy full control over your Omen’s cooling!**
