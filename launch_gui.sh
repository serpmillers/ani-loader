#!/bin/bash

# Just runs the GUI without recompiling everything
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