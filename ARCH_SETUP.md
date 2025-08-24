# Makima Setup Guide for Arch Linux

This guide provides step-by-step instructions to install and configure Makima on Arch Linux.

## Quick Setup (Automated)

### 1. Run the Main Setup Script

```bash
sudo ./setup-arch.sh
```

This script will:
- Install all required dependencies (Rust, Cargo, Clang, Ruby, etc.)
- Set up the Rust toolchain
- Build Makima from source
- Install the binary to `/usr/local/bin/makima`
- Configure udev rules and systemd service
- Create example configurations
- Add your user to the `input` group

### 2. Configure Your Devices

```bash
# List available input devices
./configure-device.sh --list

# Interactive device configuration
./configure-device.sh --device
```

### 3. Start Makima

```bash
# Start the service
sudo systemctl start makima

# Enable auto-start (optional)
sudo systemctl enable makima

# Check status
sudo systemctl status makima

# View logs
journalctl -u makima -f
```

---

## Manual Setup (Step by Step)

If you prefer to understand each step or need to troubleshoot:

### Step 1: Install Dependencies

```bash
# Update package database
sudo pacman -Sy

# Install required packages
sudo pacman -S --needed rust cargo git clang ruby base-devel systemd udev
```

### Step 2: Set Up Rust

```bash
# Install rustup if not already installed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source ~/.cargo/env

# Set stable toolchain
rustup default stable
rustup update
```

### Step 3: Build Makima

```bash
# Clone or navigate to makima directory
cd /path/to/makima

# Build release version
cargo build --release
```

### Step 4: Install Binary and Configuration

```bash
# Copy binary (requires sudo)
sudo cp target/release/makima /usr/local/bin/
sudo chmod +x /usr/local/bin/makima

# Copy udev rules
sudo cp 50-makima.rules /etc/udev/rules.d/

# Enable uinput module
echo "uinput" | sudo tee /etc/modules-load.d/uinput.conf
sudo modprobe uinput

# Create config directory
mkdir -p ~/.config/makima
```

### Step 5: Configure Systemd Service

```bash
# Create service file
sudo tee /etc/systemd/system/makima.service << EOF
[Unit]
Description=Makima remapping daemon
After=graphical-session.target

[Service]
Type=simple
Environment="MAKIMA_CONFIG=$HOME/.config/makima"
ExecStart=/usr/local/bin/makima
Restart=always
RestartSec=3
User=$USER
Group=input

[Install]
WantedBy=default.target
EOF

# Add user to input group
sudo usermod -a -G input $USER

# Reload systemd
sudo systemctl daemon-reload
sudo udevadm control --reload-rules
sudo udevadm trigger
```

---

## Device Configuration

### Finding Your Devices

```bash
# List all input devices
sudo evtest

# Or use the helper script
./configure-device.sh --list
```

### Creating Configuration Files

Configuration files should be named after your device and placed in `~/.config/makima/`:

**Example**: If your device is named "AT Translated Set 2 keyboard", create:
`~/.config/makima/AT Translated Set 2 keyboard.toml`

### Basic Configuration Example

```toml
# ~/.config/makima/AT Translated Set 2 keyboard.toml

[bindings.remap]
# Remap Caps Lock to Escape
KEY_CAPSLOCK = "KEY_ESC"

# Remap Right Alt to Right Ctrl
KEY_RIGHTALT = "KEY_RIGHTCTRL"

[bindings.commands]
# Volume controls
KEY_VOLUMEUP = "amixer set Master 5%+"
KEY_VOLUMEDOWN = "amixer set Master 5%-"

[settings]
LAYOUT_SWITCHER = "KEY_SCROLLLOCK"
NOTIFY_LAYOUT_SWITCH = false
```

### Ruby Scripting Configuration

For advanced input processing, you can use Ruby scripts:

1. **Set environment variable**:
   ```bash
   export MAKIMA_RUBY_SCRIPT="$HOME/.config/makima/scripts/my_script.rb"
   ```

2. **Create Ruby script**:
   ```ruby
   # ~/.config/makima/scripts/my_script.rb
   
   def handle(event)
     # Convert Caps Lock to Escape
     if event.key == 58 && event.key_down?  # KEY_CAPSLOCK
       Makima.press(1)  # KEY_ESC
       return nil  # Consume the event
     end
     
     # Default: pass through unchanged
   end
   ```

3. **Update systemd service** to include the environment variable:
   ```bash
   sudo systemctl edit makima
   ```
   Add:
   ```ini
   [Service]
   Environment="MAKIMA_RUBY_SCRIPT=/home/yourusername/.config/makima/scripts/my_script.rb"
   ```

---

## Commands Reference

### Service Management

```bash
# Start/stop service
sudo systemctl start makima
sudo systemctl stop makima
sudo systemctl restart makima

# Enable/disable auto-start
sudo systemctl enable makima
sudo systemctl disable makima

# Check status and logs
sudo systemctl status makima
journalctl -u makima -f
journalctl -u makima --since "1 hour ago"
```

### Configuration Management

```bash
# List devices
./configure-device.sh --list

# Interactive device setup
./configure-device.sh --device

# Test configuration (dry run)
sudo /usr/local/bin/makima  # Run in foreground to see output

# Edit config files
nano ~/.config/makima/your-device.toml
```

### Ruby Scripting

```bash
# Set Ruby script environment variable
export MAKIMA_RUBY_SCRIPT="/path/to/script.rb"

# Test Ruby script
MAKIMA_RUBY_SCRIPT="/path/to/script.rb" sudo systemctl restart makima

# Available Ruby examples
ls ~/.config/makima/examples/ruby_scripts/
```

---

## Troubleshooting

### Common Issues

1. **Permission denied errors**:
   ```bash
   # Add user to input group
   sudo usermod -a -G input $USER
   # Log out and back in
   ```

2. **Service won't start**:
   ```bash
   # Check logs
   journalctl -u makima -n 50
   
   # Test binary directly
   sudo /usr/local/bin/makima
   ```

3. **Device not detected**:
   ```bash
   # Check device permissions
   ls -la /dev/input/
   
   # Reload udev rules
   sudo udevadm control --reload-rules
   sudo udevadm trigger
   ```

4. **Build failures**:
   ```bash
   # Install clang (needed for Ruby integration)
   sudo pacman -S clang
   
   # Update Rust
   rustup update
   
   # Clean build
   cargo clean && cargo build --release
   ```

### Getting Help

- Check service status: `sudo systemctl status makima`
- View detailed logs: `journalctl -u makima -f`
- Test configuration: Run makima in foreground to see debug output
- Verify device names: Use `sudo evtest` to confirm device names match config files

---

## Uninstallation

To completely remove Makima:

```bash
# Stop and disable service
sudo systemctl stop makima
sudo systemctl disable makima

# Remove files
sudo rm /usr/local/bin/makima
sudo rm /etc/systemd/system/makima.service
sudo rm /etc/udev/rules.d/50-makima.rules
sudo rm /etc/modules-load.d/uinput.conf

# Remove configurations (optional)
rm -rf ~/.config/makima

# Remove user from input group (optional)
sudo gpasswd -d $USER input

# Reload systemd
sudo systemctl daemon-reload
sudo udevadm control --reload-rules
```

---

## Advanced Configuration

### Application-Specific Bindings

Create configs for specific applications:
- `device-name::firefox.toml` - Firefox-specific bindings
- `device-name::code.toml` - VS Code-specific bindings

### Layout Switching

Use multiple layouts:
- `device-name.toml` - Default layout
- `device-name::1.toml` - Layout 1
- `device-name::2.toml` - Layout 2

Switch with the `LAYOUT_SWITCHER` key (default: Scroll Lock).

### Controller/Gamepad Support

```toml
[settings]
LSTICK = "cursor"           # Left stick controls cursor
RSTICK = "scroll"          # Right stick controls scroll
LSTICK_SENSITIVITY = 100   # Adjust sensitivity
RSTICK_SENSITIVITY = 50
LSTICK_DEADZONE = 10       # Adjust deadzone
RSTICK_DEADZONE = 10
```
