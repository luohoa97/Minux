#!/bin/bash
set -e

echo "Building Minux (production)..."

COMMON_CARGO_FLAGS=(
    "+nightly"
    "-Zbuild-std=core,compiler_builtins"
    "-Zbuild-std-features=compiler-builtins-mem"
    "-Zjson-target-spec"
)

USER_PKGS=(
    "elf_loader"
    "init"
)
PAYLOAD_PKGS=(
    "ramfs"
    "vesa_driver"
    "gfx_service"
    "input_service"
    "console_service"
    "shell"
    "snake"
    "x11_server"
    "x11_demo"
)

# Build kernel + all userspace payloads in one release invocation.
cargo "${COMMON_CARGO_FLAGS[@]}" build --release \
    --package minux \
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
    --package "${PAYLOAD_PKGS[8]}"

# Create ISO structure
mkdir -p isodir/boot/grub isodir/boot/modules

# Copy binaries
cp target/x86_64-minux/release/minux isodir/boot/kernel.bin
for pkg in "${USER_PKGS[@]}"; do
    cp "target/x86_64-minux/release/${pkg}" "isodir/boot/modules/"
done

# Create single bootfs payload image
python3 - <<'PY'
import struct
from pathlib import Path

root = Path("target/x86_64-minux/release")
out = Path("isodir/boot/modules/bootfs")
pkgs = [
    "ramfs",
    "vesa_driver",
    "gfx_service",
    "input_service",
    "console_service",
    "shell",
    "snake",
    "x11_server",
    "x11_demo",
]

blob = bytearray(b"MINUXFS1")
for name in pkgs:
    data = (root / name).read_bytes()
    nb = name.encode("ascii")
    blob += struct.pack("<HI", len(nb), len(data))
    blob += nb
    blob += data
out.write_bytes(blob)
print(f"bootfs entries={len(pkgs)} size={len(blob)}")
PY

# GRUB config
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

# Build ISO
echo "Creating ISO with grub2-mkrescue..."
grub2-mkrescue -o minux.iso isodir 2>&1 | tee grub.log

echo "Production ISO ready: minux.iso"
