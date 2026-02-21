#!/bin/bash
set -e

QEMU_PID=""
cleanup_qemu() {
    if [ -n "${QEMU_PID}" ] && kill -0 "${QEMU_PID}" 2>/dev/null; then
        kill "${QEMU_PID}" 2>/dev/null || true
        wait "${QEMU_PID}" 2>/dev/null || true
    fi
}
trap cleanup_qemu EXIT INT TERM

timestamp() { date +"%H:%M:%S"; }
step() { echo "[$(timestamp)] $*"; }

step "Building Minux (dev)..."

COMMON_CARGO_FLAGS=(
    "+nightly"
    "-Zbuild-std=core,compiler_builtins"
    "-Zbuild-std-features=compiler-builtins-mem"
    "-Zjson-target-spec"
)

KERNEL_RUSTFLAGS=(
    "-C" "link-arg=-T"
    "-C" "link-arg=kernel/linker.ld"
    "-C" "link-arg=-z"
    "-C" "link-arg=max-page-size=0x1000"
    "-C" "code-model=kernel"
)

USER_RUSTFLAGS=(
    "-C" "link-arg=-T"
    "-C" "link-arg=userspace/linker.ld"
    "-C" "link-arg=-z"
    "-C" "link-arg=max-page-size=0x1000"
)

USER_PKGS=(
    "elf_loader"
    "init"
)
PAYLOAD_PKGS=(
    "ramfs"
    "vfs"
    "vesa_driver"
    "gfx_service"
    "input_service"
    "console_service"
    "shell"
    "snake"
    "x11_server"
    "x11_demo"
)

# Build kernel (dev) and all userspace payloads (release) in two cargo invocations.
step "Build kernel (dev): start"
time cargo "${COMMON_CARGO_FLAGS[@]}" build --package minux --target x86_64-minux.json --config "target.x86_64-minux.rustflags=[\"${KERNEL_RUSTFLAGS[0]}\",\"${KERNEL_RUSTFLAGS[1]}\",\"${KERNEL_RUSTFLAGS[2]}\",\"${KERNEL_RUSTFLAGS[3]}\",\"${KERNEL_RUSTFLAGS[4]}\",\"${KERNEL_RUSTFLAGS[5]}\",\"${KERNEL_RUSTFLAGS[6]}\",\"${KERNEL_RUSTFLAGS[7]}\",\"${KERNEL_RUSTFLAGS[8]}\",\"${KERNEL_RUSTFLAGS[9]}\"]"
step "Build kernel (dev): done"

step "Build userspace (release): start"
time cargo "${COMMON_CARGO_FLAGS[@]}" build --release --target x86_64-minux.json \
    --config "target.x86_64-minux.rustflags=[\"${USER_RUSTFLAGS[0]}\",\"${USER_RUSTFLAGS[1]}\",\"${USER_RUSTFLAGS[2]}\",\"${USER_RUSTFLAGS[3]}\",\"${USER_RUSTFLAGS[4]}\",\"${USER_RUSTFLAGS[5]}\",\"${USER_RUSTFLAGS[6]}\",\"${USER_RUSTFLAGS[7]}\"]" \
    --package "${USER_PKGS[0]}" \
    --package "${USER_PKGS[1]}" \
    --package "${PAYLOAD_PKGS[0]}" \
    --package "${PAYLOAD_PKGS[1]}" \
    --package "${PAYLOAD_PKGS[2]}" \
    --package "${PAYLOAD_PKGS[3]}" \
    --package "${PAYLOAD_PKGS[4]}" \
    --package "${PAYLOAD_PKGS[5]}" \
    --package "${PAYLOAD_PKGS[6]}" \
    --package "${PAYLOAD_PKGS[7]}" \
    --package "${PAYLOAD_PKGS[8]}" \
    --package "${PAYLOAD_PKGS[9]}"
step "Build userspace (release): done"

step "Create ISO layout"
mkdir -p isodir/boot/grub isodir/boot/modules

# Copy binaries
cp target/x86_64-minux/debug/minux isodir/boot/kernel.bin
for pkg in "${USER_PKGS[@]}"; do
    cp "target/x86_64-minux/release/${pkg}" "isodir/boot/modules/"
done

step "Pack bootfs payload image"
python3 - <<'PY'
import struct
from pathlib import Path

root = Path("target/x86_64-minux/release")
out = Path("isodir/boot/modules/bootfs")
bin_pkgs = [
    "ramfs",
    "vfs",
    "vesa_driver",
    "gfx_service",
    "input_service",
    "console_service",
    "shell",
    "snake",
    "x11_server",
    "x11_demo",
]
extra_files = [
    ("usr/share/kbd/consolefonts/ter-u16n.bdf", Path("userspace/assets/fonts/ter-u16n.bdf")),
]

blob = bytearray(b"MINUXFS1")
entries = []
for name in bin_pkgs:
    entries.append((name, root / name))
for name, path in extra_files:
    entries.append((name, path))

for name, path in entries:
    data = path.read_bytes()
    nb = name.encode("ascii")
    blob += struct.pack("<HI", len(nb), len(data))
    blob += nb
    blob += data

out.write_bytes(blob)
print(f"bootfs entries={len(entries)} size={len(blob)}")
PY

step "Write GRUB config"
cat > isodir/boot/grub/grub.cfg << 'EOF_GRUB'
set timeout=3
set default=0

menuentry "Minux L4 Microkernel" {
    echo "Loading Minux..."
    multiboot2 /boot/kernel.bin
    echo "Loading bootstrap modules..."
    module2 /boot/modules/elf_loader elf_loader
    module2 /boot/modules/init init
    module2 /boot/modules/bootfs bootfs
    echo "Booting..."
    boot
}
EOF_GRUB

# Check for GRUB modules
if [ ! -d /usr/lib/grub/i386-pc ] && [ ! -d /usr/lib/grub2/i386-pc ]; then
    echo "Error: GRUB BIOS modules not found!"
    echo "Install with: sudo dnf install grub2-pc grub2-pc-modules"
    exit 1
fi

# Create bootable ISO with GRUB
step "Create ISO with grub2-mkrescue"
if grub2-mkrescue -o minux.iso isodir 2>&1 | tee grub.log; then
    step "ISO created successfully"
else
    echo "grub2-mkrescue failed, trying manual method..."
    echo "Installing GRUB manually..."
    mkdir -p isodir/boot/grub/i386-pc

    # Find GRUB files
    if [ -d /usr/lib/grub/i386-pc ]; then
        cp -r /usr/lib/grub/i386-pc/* isodir/boot/grub/i386-pc/
    elif [ -d /usr/share/grub2/i386-pc ]; then
        cp -r /usr/share/grub2/i386-pc/* isodir/boot/grub/i386-pc/
    fi

    # Create boot image
    grub2-mkimage -o isodir/boot/grub/i386-pc/eltorito.img -O i386-pc-eltorito \
        biosdisk iso9660 multiboot2

    # Create ISO
    xorriso -as mkisofs \
        -b boot/grub/i386-pc/eltorito.img \
        -no-emul-boot \
        -boot-load-size 4 \
        -boot-info-table \
        -o minux.iso \
        isodir
fi

step "Sanity checks"
# ls -lh target/x86_64-minux/debug/minux
if command -v grub2-file >/dev/null 2>&1; then
    if ! grub2-file --is-x86-multiboot2 target/x86_64-minux/debug/minux; then
        echo "Error: kernel is not recognized as Multiboot2 by grub2-file"
        exit 1
    fi
elif command -v grub-file >/dev/null 2>&1; then
    if ! grub-file --is-x86-multiboot2 target/x86_64-minux/debug/minux; then
        echo "Error: kernel is not recognized as Multiboot2 by grub-file"
        exit 1
    fi
fi
echo ""
echo "Checking for multiboot2 header in first 32KB..."
dd if=target/x86_64-minux/debug/minux bs=1 count=32768 2>/dev/null | hexdump -C | grep "d6 50 52 e8" && echo "Found!" || echo "NOT FOUND in first 32KB!"
echo ""
echo "Checking all sections..."
readelf -S target/x86_64-minux/debug/minux | grep -E "multiboot|boot"
echo ""
echo "Checking ISO..."
# ls -lh minux.iso

# Run
step "Booting Minux in QEMU"
rm -f qemu.log
QEMU_ARGS=(
    -boot d
    -cdrom minux.iso
    -smp 4
    -no-reboot
    -no-shutdown
    -d int,cpu_reset,guest_errors
    -D qemu.log
    -chardev stdio,id=serial0,signal=off
    -serial chardev:serial0
)

if [ "${DEBUG_GDB:-0}" = "1" ]; then
    step "QEMU gdbstub enabled on tcp::1234 (waiting for debugger)"
    QEMU_ARGS+=(-s -S)
fi

qemu-system-x86_64 "${QEMU_ARGS[@]}" &
QEMU_PID=$!
wait "${QEMU_PID}"
