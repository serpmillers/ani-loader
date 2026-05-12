use std::ffi::CString;
use std::ptr;

use slint::{ComponentHandle, ModelRc, VecModel};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use serde::Deserialize;
use std::fs;

slint::include_modules!();

// ── TOML Configuration Structs ───────────────────────────────────────
#[derive(Deserialize, Debug, Clone)]
struct Config {
    entries: Vec<BootEntry>,
}

#[derive(Deserialize, Debug, Clone)]
struct BootEntry {
    name: String,
    os_type: String, // "linux", "windows", or "uefi"

    // Linux specific
    kernel: Option<String>,
    initrd: Option<Vec<String>>,
    cmdline: Option<String>,
    subvol: Option<String>,

    // Windows specific
    esp_uuid: Option<String>,
    loader_path: Option<String>,
}

fn main() {
    // ── 0. Bootstrap Kernel Virtual Filesystems ───────────────────────
    println!("[ANI-LOADER] Bootstrapping VFS (devtmpfs, sysfs, proc)...");
    unsafe {
        let dev = CString::new("/dev").unwrap();
        let dev_fstype = CString::new("devtmpfs").unwrap();
        libc::mount(
            ptr::null(),
            dev.as_ptr(),
            dev_fstype.as_ptr(),
            0,
            ptr::null(),
        );

        let sys = CString::new("/sys").unwrap();
        let sys_fstype = CString::new("sysfs").unwrap();
        libc::mount(
            ptr::null(),
            sys.as_ptr(),
            sys_fstype.as_ptr(),
            0,
            ptr::null(),
        );

        let proc = CString::new("/proc").unwrap();
        let proc_fstype = CString::new("proc").unwrap();
        libc::mount(
            ptr::null(),
            proc.as_ptr(),
            proc_fstype.as_ptr(),
            0,
            ptr::null(),
        );
    }

    println!("[ANI-LOADER] VFS mounted. Initializing DRM/KMS graphics backend...");
    // 1. Load all essential kernel modules into memory
    let modules = ["virtio_gpu", "virtio_input", "evdev", "btrfs"];
    for module in modules {
        match Command::new("/bin/modprobe").arg(module).status() {
            Ok(status) if status.success() => println!("[ANI-LOADER] Module loaded: {}", module),
            Ok(status) => println!(
                "[WARNING] modprobe {} failed with exit code: {:?}",
                module,
                status.code()
            ),
            Err(e) => println!("[ERROR] Could not execute modprobe for {}: {}", module, e),
        }
    }

    // 2. Start the UDEV Daemon in the background
    println!("[ANI-LOADER] Starting udev device manager...");
    let _ = Command::new("/bin/udevd").arg("--daemon").status();

    // 3. Tell udev to trigger hardware detection and wait for it to finish
    let _ = Command::new("/bin/udevadm")
        .args(["trigger", "--action=add"])
        .status();
    let _ = Command::new("/bin/udevadm").arg("settle").status();

    println!("[ANI-LOADER] Hardware initialized. KMS graphics and libinput ready.");

    // ── Mount the Physical Drive ──────────────────────────────────────
    println!("[ANI-LOADER] Mounting physical boot drive...");

    let _ = fs::create_dir_all("/mnt");

    // Mount the Btrfs partition (QEMU mock drive is /dev/vda1)
    let mount_status = Command::new("/bin/mount")
        .args(["/dev/vda1", "/mnt"])
        .status()
        .expect("Failed to execute mount command");

    if !mount_status.success() {
        println!("[WARNING] Failed to mount /dev/vda1. UI will be empty.");
    }

    // ── 1. Read and parse the TOML config ─────────────────────────────
    println!("[ANI-LOADER] Reading configuration...");

    let primary_path = "/mnt/@arch/boot/ani-loader.toml";
    let fallback_path = "/etc/ani-loader.toml"; // Our embedded RAM disk config

    // Attempt to read from the physical drive first
    let mut config_content = fs::read_to_string(primary_path);

    if config_content.is_err() {
        println!("[WARNING] Could not read from drive. Falling back to internal emergency config.");
        config_content = fs::read_to_string(fallback_path);
    }

    let mut boot_targets: Vec<BootEntry> = Vec::new();

    if let Ok(config_str) = config_content {
        match toml::from_str::<Config>(&config_str) {
            Ok(parsed_config) => {
                boot_targets = parsed_config.entries;
                println!("[ANI-LOADER] Loaded {} boot entries.", boot_targets.len());
            }
            Err(e) => println!("[ERROR] Failed to parse TOML: {}", e),
        }
    } else {
        println!("[FATAL] No configuration files found anywhere.");
    }

    // ── 2. Initialize Slint UI ────────────────────────────────────────
    let ui = match MainWindow::new() {
        Ok(window) => window,
        Err(e) => {
            println!("\n[FATAL ERROR] Slint UI failed to load:");
            println!("{:?}", e);
            println!("Hanging system to preserve logs...");
            loop {}
        }
    };

    let target_width = 1920.0;
    let target_height = 1080.0;
    ui.set_window_width(target_width);
    ui.set_window_height(target_height);

    // ── 3. Map Dynamic TOML Data to Slint UI Structs ──────────────────
    let os_items: Vec<OSItem> = boot_targets
        .iter()
        .map(|entry| {
            // Dynamically assign Catppuccin themes based on the OS name
            let (subtitle, color, arch, bootloader) = match entry.name.as_str() {
                "Arch [Erde]" => (
                    "Main Development OS",
                    slint::Color::from_rgb_u8(0x89, 0xb4, 0xfa),
                    "x86_64",
                    "kexec (Btrfs)",
                ),
                "Arch [Abgrund]" => (
                    "Debian Lab / Cybersecurity",
                    slint::Color::from_rgb_u8(0xcb, 0xa6, 0xf7),
                    "x86_64",
                    "kexec (Btrfs)",
                ),
                "Windows 11 [Atlas]" => (
                    "AtlasOS Optimized Gaming",
                    slint::Color::from_rgb_u8(0x74, 0xc7, 0xec),
                    "x86_64",
                    "chainloader (EFI)",
                ),
                "UEFI Firmware Settings" => (
                    "Motherboard Settings",
                    slint::Color::from_rgb_u8(0xa6, 0xe3, 0xa1),
                    "System",
                    "fwsetup",
                ),
                _ => (
                    "Operating System",
                    slint::Color::from_rgb_u8(0x93, 0x99, 0xb2),
                    "x86_64",
                    "unknown",
                ),
            };

            // Extract display name for kernel (e.g. "/@arch/boot/vmlinuz-linux-zen" -> "vmlinuz-linux-zen")
            let kernel_display = entry
                .kernel
                .as_deref()
                .and_then(|k| k.split('/').last())
                .unwrap_or(if entry.os_type == "windows" {
                    "ntoskrnl.exe"
                } else {
                    "Firmware NVRAM"
                });

            OSItem {
                name: entry.name.clone().into(),
                subtitle: subtitle.into(),
                accent_color: color,
                kernel: kernel_display.into(),
                arch: arch.into(),
                bootloader: bootloader.into(),
            }
        })
        .collect();

    let model = Rc::new(VecModel::from(os_items));
    ui.set_os_list(ModelRc::from(model));

    // ── 4. Smooth Panel Re-Animation Hook ─────────────────────────────
    let ui_weak = ui.as_weak();
    ui.on_os_selected(move |index| {
        if let Some(ui) = ui_weak.upgrade() {
            ui.set_selected_os(index);
            ui.set_panel_visible(false);

            let ui_delay_handle = ui_weak.clone();
            slint::Timer::single_shot(Duration::from_millis(40), move || {
                if let Some(ui) = ui_delay_handle.upgrade() {
                    ui.set_panel_visible(true);
                }
            });
        }
    });

    // ── 5. Dynamic Boot Execution Callback ────────────────────────────
    let boot_targets_clone = boot_targets.clone();

    ui.on_boot_into(move |name| {
        if let Some(target) = boot_targets_clone.iter().find(|t| t.name == name.as_str()) {
            match target.os_type.as_str() {
                "linux" => {
                    let kernel = target.kernel.as_deref().unwrap_or("");
                    // Use the explicit subvol from TOML, or fallback to the old guess
                    let subvol = target
                        .subvol
                        .as_deref()
                        .unwrap_or_else(|| kernel.split('/').nth(1).unwrap_or(""));

                    let initrds: Vec<&str> = target
                        .initrd
                        .as_ref()
                        .map(|v| v.iter().map(AsRef::as_ref).collect())
                        .unwrap_or_default();

                    execute_linux_boot(
                        subvol,
                        kernel,
                        &initrds,
                        target.cmdline.as_deref().unwrap_or(""),
                    );
                }
                "windows" => {
                    let esp_uuid = target.esp_uuid.as_deref().unwrap_or("");
                    let loader = target.loader_path.as_deref().unwrap_or("");
                    execute_windows_boot(esp_uuid, loader);
                }
                "uefi" => {
                    execute_uefi_reboot();
                }
                _ => println!("[ERROR] Unknown OS Type: {}", target.os_type),
            }
        }
    });

    if let Err(e) = ui.run() {
        println!("\n[FATAL ERROR] Slint run loop crashed:");
        println!("{:?}", e);
        loop {}
    }
}

// ── Low-Level Boot Handoff Implementations ───────────────────────────
fn execute_linux_boot(subvol: &str, kernel_path: &str, initrd_paths: &[&str], cmdline: &str) {
    let mount_dir = "/mnt/boot_source";
    let device = "/dev/vda1";

    println!("[SYSTEM] Preparing to boot from subvolume: '{}'", subvol);

    let _ = Command::new("umount").args(["-l", mount_dir]).status();
    let _ = fs::create_dir_all(mount_dir);

    let mount_status = Command::new("mount")
        .args(["-o", &format!("subvol={}", subvol), device, mount_dir])
        .status();

    if mount_status.map(|s| s.success()).unwrap_or(false) {
        println!("[SYSTEM] Subvolume '{}' mounted. Loading kexec...", subvol);

        let target_kernel = format!("{}{}", mount_dir, kernel_path);
        println!("[DEBUG] Kexec Kernel: {}", target_kernel);

        let mut kexec_cmd = Command::new("kexec");
        kexec_cmd.arg("-l").arg(&target_kernel);

        for path in initrd_paths {
            let full_initrd_path = format!("{}{}", mount_dir, path);
            println!("[DEBUG] Adding Initrd: {}", full_initrd_path);
            kexec_cmd.arg(format!("--initrd={}", full_initrd_path));
        }

        kexec_cmd.arg(format!("--command-line={}", cmdline));

        let load_status = kexec_cmd.status();

        match load_status {
            Ok(s) if s.success() => {
                println!("[SUCCESS] Handoff ready. See you on the other side!");
                let _ = Command::new("kexec").arg("-e").status();
            }
            Ok(s) => eprintln!(
                "[ERROR] Kexec failed (Code {:?}). Is the kernel file valid?",
                s.code()
            ),
            Err(e) => eprintln!("[ERROR] Spawn error: {}", e),
        }
    } else {
        eprintln!(
            "[ERROR] Failed to mount device {} with subvol {}",
            device, subvol
        );
    }
}

fn execute_windows_boot(esp_uuid: &str, loader_path: &str) {
    println!(
        "[SYSTEM] Targeting ESP UUID: {} via {}",
        esp_uuid, loader_path
    );
    let win_boot_num = "0001";

    let status = Command::new("efibootmgr")
        .args(["-n", win_boot_num])
        .status();

    if status.map(|s| s.success()).unwrap_or(false) {
        println!("[SYSTEM] BootNext set successfully. Rebooting to launch Windows!");
        let _ = Command::new("reboot").status();
    } else {
        eprintln!("[ERROR] Failed to set EFI BootNext variable.");
    }
}

fn execute_uefi_reboot() {
    println!("[SYSTEM] Instructing firmware to boot into UEFI Settings on next startup...");
    let status = Command::new("systemctl")
        .args(["reboot", "--firmware-setup"])
        .status();

    if !status.map(|s| s.success()).unwrap_or(false) {
        let _ = Command::new("efibootmgr").args(["-t", "0"]).status();
    }
}
