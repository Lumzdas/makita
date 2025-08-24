#!/bin/bash

# Makima Setup Script for Arch Linux
# Run this script to install all dependencies and set up Makima on your system

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
USER_NAME="${SUDO_USER:-$USER}"
USER_HOME="/home/$USER_NAME"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    echo -e "${BLUE}[INFO]${NC} $1"
}

print_success() {
    echo -e "${GREEN}[SUCCESS]${NC} $1"
}

print_warning() {
    echo -e "${YELLOW}[WARNING]${NC} $1"
}

print_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

check_root() {
    if [ "$EUID" -ne 0 ]; then
        print_error "Please run this script with sudo."
        print_status "Usage: sudo ./setup-arch.sh"
        exit 1
    fi
}

install_dependencies() {
    print_status "Installing system dependencies..."

    # Update package database
    pacman -Sy --noconfirm

    # Install base dependencies
    pacman -S --needed --noconfirm \
        rust \
        cargo \
        git \
        clang \
        ruby \
        base-devel \
        systemd \
        udev

    print_success "System dependencies installed"
}

setup_rust() {
    print_status "Setting up Rust toolchain for user $USER_NAME..."

    # Switch to user context for Rust setup
    sudo -u "$USER_NAME" bash << 'EOF'
        # Ensure rustup is properly configured
        if ! command -v rustup &> /dev/null; then
            curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
            source ~/.cargo/env
        fi

        # Make sure we have the stable toolchain
        rustup default stable
        rustup update
EOF

    print_success "Rust toolchain configured"
}

build_makima() {
    print_status "Building Makima from source..."

    # Build as user
    sudo -u "$USER_NAME" bash << EOF
        cd "$SCRIPT_DIR"
        source ~/.cargo/env || true
        export PATH="\$HOME/.cargo/bin:\$PATH"
        cargo build --release
EOF

    if [ ! -f "$SCRIPT_DIR/target/release/makima" ]; then
        print_error "Build failed - makima binary not found"
        exit 1
    fi

    print_success "Makima built successfully"
}

install_binary_and_configs() {
    print_status "Installing binary and configuration files..."

    # Copy binary
    cp "$SCRIPT_DIR/target/release/makima" /usr/local/bin/
    chmod +x /usr/local/bin/makima

    # Copy udev rules
    cp "$SCRIPT_DIR/50-makima.rules" /etc/udev/rules.d/

    # Enable uinput module
    echo "uinput" > /etc/modules-load.d/uinput.conf
    modprobe uinput

    # Create user config directory
    mkdir -p "$USER_HOME/.config/makima"
    chown -R "$USER_NAME:$USER_NAME" "$USER_HOME/.config/makima"

    print_success "Binary and configuration files installed"
}

setup_systemd_service() {
    print_status "Setting up systemd service..."

    # Create systemd service file
    cat > /etc/systemd/system/makima.service << EOF
[Unit]
Description=Makima remapping daemon
After=graphical-session.target

[Service]
Type=simple
Environment="MAKIMA_CONFIG=$USER_HOME/.config/makima"
ExecStart=/usr/local/bin/makima
Restart=always
RestartSec=3
User=$USER_NAME
Group=input

[Install]
WantedBy=default.target
EOF

    # Add user to input group
    usermod -a -G input "$USER_NAME"

    # Reload and enable service
    systemctl daemon-reload
    udevadm control --reload-rules
    udevadm trigger

    print_success "Systemd service configured"
}

setup_example_configs() {
    print_status "Setting up example configurations..."

    # Create examples directory
    mkdir -p "$USER_HOME/.config/makima/examples"

    # Copy Ruby script examples if they exist
    if [ -d "$SCRIPT_DIR/examples/ruby_scripts" ]; then
        cp -r "$SCRIPT_DIR/examples/ruby_scripts" "$USER_HOME/.config/makima/examples/"
    fi

    # Create a basic keyboard config example
    cat > "$USER_HOME/.config/makima/examples/example-keyboard.toml" << 'EOF'
# Example keyboard configuration
# Rename this file to match your device name (e.g., "AT Translated Set 2 keyboard.toml")
# Find your device name by running: sudo evtest

[bindings.remap]
# Example: Remap Caps Lock to Escape
KEY_CAPSLOCK = "KEY_ESC"

# Example: Remap Right Alt to Right Ctrl
KEY_RIGHTALT = "KEY_RIGHTCTRL"

[settings]
# Optional settings
LAYOUT_SWITCHER = "KEY_SCROLLLOCK"
NOTIFY_LAYOUT_SWITCH = true
EOF

    # Create a Ruby script example config
    cat > "$USER_HOME/.config/makima/examples/ruby-example.toml" << 'EOF'
# Example configuration using Ruby scripts
# Set MAKIMA_RUBY_SCRIPT environment variable to use Ruby scripting

[settings]
# Ruby script path (alternative to MAKIMA_RUBY_SCRIPT env var)
# RUBY_SCRIPT = "/home/user/.config/makima/examples/ruby_scripts/eat_input.rb"
EOF

    chown -R "$USER_NAME:$USER_NAME" "$USER_HOME/.config/makima"

    print_success "Example configurations created"
}

print_instructions() {
    print_success "Makima setup completed!"
    echo
    print_status "Next steps:"
    echo "1. Find your input device names:"
    echo "   sudo evtest"
    echo
    echo "2. Create config files in ~/.config/makima/ named after your devices:"
    echo "   Example: 'AT Translated Set 2 keyboard.toml'"
    echo
    echo "3. Check example configs in ~/.config/makima/examples/"
    echo
    echo "4. Start the service:"
    echo "   sudo systemctl start makima"
    echo
    echo "5. Enable auto-start (optional):"
    echo "   sudo systemctl enable makima"
    echo
    echo "6. Check service status:"
    echo "   sudo systemctl status makima"
    echo
    echo "7. View logs:"
    echo "   journalctl -u makima -f"
    echo
    print_status "For Ruby scripting, set environment variable:"
    echo "   export MAKIMA_RUBY_SCRIPT=/path/to/your/script.rb"
    echo
    print_warning "Note: You may need to log out and back in for group changes to take effect."
}

main() {
    echo "=== Makima Setup for Arch Linux ==="
    echo

    check_root
    install_dependencies
    setup_rust
    build_makima
    install_binary_and_configs
    setup_systemd_service
    setup_example_configs
    print_instructions
}

main "$@"
