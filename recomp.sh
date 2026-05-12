#!/bin/bash

# Exit immediately if a command exits with a non-zero status
set -e 
cd "$(dirname "$0")"

echo "==> 🧹 Cleaning up old artifacts..."
# PRO TIP: I commented out `cargo clean`. Cargo's caching is incredibly smart. 
# Unless you are adding/removing major dependencies in Cargo.toml, 
# leaving it commented out will make your UI iteration rebuilds take 2 seconds instead of 60!
# cargo clean 

rm -rf initramfs initramfs.img

echo "==> 📁 Building initramfs directory tree..."
mkdir -p initramfs/{bin,sbin,etc,proc,sys,dev,mnt,run/user/0}
mkdir -p initramfs/etc/fonts
mkdir -p initramfs/usr/{share/{fonts/custom,X11/xkb},lib/udev}
mkdir -p initramfs/tmp

echo "==> 🛠️ Copying host utilities..."
# These might ask for your password once during the script execution
sudo cp /usr/bin/mount initramfs/bin/
sudo cp /usr/bin/kexec initramfs/bin/
sudo cp /usr/bin/mkdir initramfs/bin/
sudo cp /usr/bin/udevadm initramfs/bin/
sudo cp /usr/lib/systemd/systemd-udevd initramfs/bin/udevd
cp -r /usr/share/libinput initramfs/usr/share/ 2>/dev/null || true

cp -r /usr/lib/udev/rules.d initramfs/usr/lib/udev/
cp /usr/lib/udev/hwdb.bin initramfs/usr/lib/udev/ 2>/dev/null || true

cp -r /usr/share/X11/xkb/* initramfs/usr/share/X11/xkb/

echo "==> 🧩 Injecting Btrfs & Input Kernel Modules..."
KERNEL_VER=$(file /boot/vmlinuz-linux-zen | grep -oP 'version \K[^ ]+')
TARGET_MOD_DIR="initramfs/lib/modules/$KERNEL_VER"

rm -rf "$TARGET_MOD_DIR"
mkdir -p "$TARGET_MOD_DIR/kernel"

# 1. Sledgehammer: Copy the entire functional families
cp -r /lib/modules/"$KERNEL_VER"/kernel/fs/btrfs "$TARGET_MOD_DIR/kernel/" 2>/dev/null || true
cp -r /lib/modules/"$KERNEL_VER"/kernel/drivers/input/evdev.ko* "$TARGET_MOD_DIR/kernel/" 2>/dev/null || true
cp -r /lib/modules/"$KERNEL_VER"/kernel/lib/{raid6,zstd} "$TARGET_MOD_DIR/kernel/" 2>/dev/null || true

# 2. Precision: Catch the VirtIO and specific library symbols
MODULE_PATTERNS=("virtio" "libcrc32c")
for pattern in "${MODULE_PATTERNS[@]}"; do
    find "/lib/modules/$KERNEL_VER" -name "*$pattern*.ko*" -exec cp {} "$TARGET_MOD_DIR/kernel/" \; 2>/dev/null || true
done

# 3. The "Index": Without this, modprobe is blind to everything you just copied
touch "$TARGET_MOD_DIR/modules.order" "$TARGET_MOD_DIR/modules.builtin" "$TARGET_MOD_DIR/modules.builtin.modinfo"
depmod -b initramfs "$KERNEL_VER"

# Copy the modprobe binary so our Rust code can use it
sudo cp /usr/bin/modprobe initramfs/bin/

echo "==> 🦀 Compiling ani-loader..."
cargo build --release
cp target/release/ani-loader initramfs/init

echo "==> 🔗 Resolving and copying dynamic libraries..."
cd initramfs

# Loop through our Rust init AND the host utilities we copied
for binfile in init bin/*; do
    # Suppress errors for static binaries, and -n prevents overwriting
    ldd "$binfile" 2>/dev/null | grep -o '/[^ ]*' | while read -r lib; do
        mkdir -p ".$(dirname "$lib")"
        cp -n "$lib" ".$lib" 2>/dev/null || true
    done
done

echo "==> 📝 Injecting Emergency Fallback Config..."
cat << 'EOF' > etc/ani-loader.toml
[[entries]]
name = "Arch [FALLBACK]"
os_type = "linux"
subvol = "@arch"
kernel = "/boot/vmlinuz-linux-zen"
initrd = ["/boot/intel-ucode.img", "/boot/initramfs-linux-zen.img"]
cmdline = "root=UUID=d5fba1e7-4d9c-4804-84da-3f19fa7b6f6e rw rootflags=subvol=@arch quiet splash"

[[entries]]
name = "Arch [Abgrund]"
os_type = "linux"
subvol = "@sec"
kernel = "/boot/vmlinuz-linux"
initrd = ["/boot/intel-ucode.img", "/boot/initramfs-linux.img"]
cmdline = "root=UUID=d5fba1e7-4d9c-4804-84da-3f19fa7b6f6e rw rootflags=subvol=@sec quiet splash"

[[entries]]
name = "Windows 11 [Atlas]"
os_type = "windows"
esp_uuid = "5662-BD8F"
loader_path = "/EFI/Microsoft/Boot/bootmgfw.efi"

[[entries]]
name = "UEFI Firmware Settings"
os_type = "uefi"
EOF

echo "==> 🔤 Configuring Fontconfig..."
cat << 'EOF' > etc/fonts/fonts.conf
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "urn:fontconfig:fonts.dtd">
<fontconfig>
  <dir>/usr/share/fonts</dir>
  <cachedir>/tmp</cachedir>
</fontconfig>
EOF

# Fixed the nested directory path here
find /usr/share/fonts -name "*.ttf" | head -n 1 | xargs -I {} cp {} usr/share/fonts/custom/

echo "==> 📦 Packaging initramfs.img..."
# Suppressing the cpio stderr just to keep the terminal output clean
find . -print0 | cpio --null --create --format=newc 2>/dev/null | gzip -9 > ../initramfs.img
cd ..

echo "==> 🚀 Launching QEMU sandbox..."
qemu-system-x86_64 \
    -enable-kvm \
    -m 2G \
    -kernel /boot/vmlinuz-linux-zen \
    -initrd initramfs.img \
    -drive file=/home/zaevo/mock_disk.raw,format=raw,if=virtio \
    -device virtio-vga,xres=1920,yres=1080 \
    -display sdl,gl=on \
    -serial stdio \
    -append "console=ttyS0"