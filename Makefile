# Build Options
export ARCH := riscv64
export LOG := warn
export DWARF := y
export MEMTRACK := n

# QEMU Options
export BLK := y
export NET := y
export VSOCK := n
export BUS := mmio
export MEM := 1G
export ICOUNT := n

# Generated Options
export A := $(PWD)
export NO_AXSTD := y
export AX_LIB := axfeat
export APP_FEATURES := qemu

ifeq ($(MEMTRACK), y)
	APP_FEATURES += starry-api/memtrack
endif

default: build

ROOTFS_URL = https://github.com/Starry-OS/rootfs/releases/download/20260214
ROOTFS_IMG = rootfs-$(ARCH).img
BENCH_CC ?= riscv64-linux-musl-gcc
BENCH_CFLAGS ?= -static -no-pie -fno-pie -Os -s

rootfs:
	@if [ ! -f $(ROOTFS_IMG) ]; then \
		echo "Image not found, downloading..."; \
		curl -f -L $(ROOTFS_URL)/$(ROOTFS_IMG).xz -O; \
		xz -d $(ROOTFS_IMG).xz; \
	fi
	@cp $(ROOTFS_IMG) make/disk.img

img:
	@echo -e "\033[33mWARN: The 'img' target is deprecated. Please use 'rootfs' instead.\033[0m"
	@$(MAKE) --no-print-directory rootfs

tests/benchmark: tests/benchmark.c
	$(BENCH_CC) $(BENCH_CFLAGS) \
		-DBENCH_TARGET_MODE='"qemu-rootfs"' \
		-DBENCH_STARTUP_CHAIN='"/bin/sh -c init.sh -> /bin/benchmark"' \
		-DBENCH_ROOT_PROVIDER='"qemu-virtio-ext4-rootfs"' \
		-o $@ $<

kernel/resources/benchmark.elf: benchmark-userbench-elf

benchmark-userbench-elf: tests/benchmark.c
	$(BENCH_CC) $(BENCH_CFLAGS) \
		-DBENCH_TARGET_MODE='"lichee-d1-userbench"' \
		-DBENCH_STARTUP_CHAIN='"android-boot-image -> embedded benchmark.elf"' \
		-DBENCH_ROOT_PROVIDER='"d1-memory-root-embedded-payload"' \
		-DBENCH_D1_DIAG \
		-o kernel/resources/benchmark.elf $<

benchmark-fullbench-elf: tests/benchmark.c
	$(BENCH_CC) $(BENCH_CFLAGS) \
		-DBENCH_TARGET_MODE='"lichee-d1-fullbench"' \
		-DBENCH_STARTUP_CHAIN='"android-boot-image -> memory-root /bin/benchmark -> eager_elf_mapping"' \
		-DBENCH_ROOT_PROVIDER='"d1-memory-root-path"' \
		-DBENCH_D1_DIAG \
		-o kernel/resources/benchmark.elf $<

defconfig justrun clean:
	@$(MAKE) -C make $@

build run debug disasm: defconfig
	@$(MAKE) -C make $@

ci-test:
	./scripts/ci-test.py $(ARCH)

# Aliases
rv:
	$(MAKE) ARCH=riscv64 run

la:
	$(MAKE) ARCH=loongarch64 run

vf2:
	$(MAKE) ARCH=riscv64 APP_FEATURES=vf2 MYPLAT=axplat-riscv64-visionfive2 BUS=mmio build

lichee:
	$(MAKE) ARCH=riscv64 APP_FEATURES=lichee-d1 MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$(PWD)/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
	@echo "Packing Android boot image (smoke)..."
	@python3 tools/android_boot_image.py pack \
		--kernel StarryOS_riscv64-lichee-d1.bin \
		--output starry-lichee-boot.img
	@python3 tools/android_boot_image.py inspect starry-lichee-boot.img

lichee-kbench:
	$(MAKE) ARCH=riscv64 APP_FEATURES=lichee-d1-kbench MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$(PWD)/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
	@echo "Packing Android boot image (kbench)..."
	@python3 tools/android_boot_image.py pack \
		--kernel StarryOS_riscv64-lichee-d1.bin \
		--output starry-lichee-kbench-boot.img
	@python3 tools/android_boot_image.py inspect starry-lichee-kbench-boot.img

lichee-userbench: benchmark-userbench-elf
	$(MAKE) ARCH=riscv64 APP_FEATURES=lichee-d1-userbench MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$(PWD)/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
	@echo "Packing Android boot image (userbench)..."
	@python3 tools/android_boot_image.py pack \
		--kernel StarryOS_riscv64-lichee-d1.bin \
		--output starry-lichee-userbench-boot.img
	@python3 tools/android_boot_image.py inspect starry-lichee-userbench-boot.img

lichee-fullbench-mem: benchmark-fullbench-elf
	$(MAKE) ARCH=riscv64 APP_FEATURES=lichee-d1-fullbench MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$(PWD)/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
	@echo "Packing Android boot image (fullbench memory-root)..."
	@python3 tools/android_boot_image.py pack \
		--kernel StarryOS_riscv64-lichee-d1.bin \
		--output starry-lichee-fullbench-mem-boot.img
	@python3 tools/android_boot_image.py inspect starry-lichee-fullbench-mem-boot.img

.PHONY: build run justrun debug disasm clean lichee lichee-kbench lichee-userbench lichee-fullbench-mem benchmark-userbench-elf benchmark-fullbench-elf
