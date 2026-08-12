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
	APP_FEATURES += starry-kernel/memtrack
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

tests/ms02_guest_service: tests/ms02_guest_service.c
	$(BENCH_CC) -static -O2 -o $@ $<

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

# Early-console pure-logic host tests (no kernel compilation needed)
host-test:
	rustc --edition=2024 --test tests/early-console-host-harness.rs -o /tmp/early-console-test
	/tmp/early-console-test
	rustc --edition=2024 --test tests/memtrack-session-host-harness.rs -o /tmp/memtrack-session-test
	/tmp/memtrack-session-test
	rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test
	/tmp/ms03-irq-host-test
	rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test
	/tmp/ms04-async-rx-host-test
	cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c tests/ms04_rx_probe.c
	cc -std=c11 -Wall -Wextra -Werror tests/ms04_rx_probe_test.c -o /tmp/ms04-rx-probe-test
	/tmp/ms04-rx-probe-test
	python3 scripts/ms04_rx_stimulus.py --self-test
	python3 scripts/ms04_rx_stimulus.py --loopback-self-test

# MS16 network benchmark foundation tests (host, no QEMU needed)
network-benchmark-test:
	cc -std=c11 -Wall -Wextra -Werror \
		tests/network_benchmark_protocol_test.c \
		tests/network_benchmark_protocol.c \
		-o /tmp/network-benchmark-protocol-test
	/tmp/network-benchmark-protocol-test
	cc -std=c11 -Wall -Wextra -Werror \
		tests/network_benchmark_platform_test.c \
		tests/network_benchmark_platform.c \
		-o /tmp/network-benchmark-platform-test
	/tmp/network-benchmark-platform-test
	python3 -m unittest tests.test_network_benchmark_tools -v
	python3 -m unittest tests.test_network_benchmark_integration -v
	python3 scripts/network_benchmark_collect.py --self-test
	python3 scripts/network_benchmark_report.py --self-test
	python3 scripts/network_benchmark_evidence.py --self-test

# MS16 portable network benchmark — host binary
tests/network_benchmark-host: tests/network_benchmark.c \
		tests/network_benchmark_protocol.c tests/network_benchmark_protocol.h \
		tests/network_benchmark_platform.c tests/network_benchmark_platform.h
	cc -std=c11 -Wall -Wextra -Werror -O2 \
		-D_BSD_SOURCE -D_DEFAULT_SOURCE -DNB_HOST_BUILD \
		tests/network_benchmark.c \
		tests/network_benchmark_protocol.c \
		tests/network_benchmark_platform.c \
		-o $@

# MS16 portable network benchmark — RISC-V static guest binary
tests/network_benchmark: tests/network_benchmark.c \
		tests/network_benchmark_protocol.c tests/network_benchmark_protocol.h \
		tests/network_benchmark_platform.c tests/network_benchmark_platform.h
	$(BENCH_CC) -std=c11 -Wall -Wextra -Werror \
		-static -Os \
		tests/network_benchmark.c \
		tests/network_benchmark_protocol.c \
		tests/network_benchmark_platform.c \
		-o $@

# MS16 local integration quick smoke
network-benchmark-local-test: tests/network_benchmark-host
	python3 -m unittest tests.test_network_benchmark_integration.WorkloadIntegration.test_loopback_matrix_has_two_closed_ledgers -v

# MS16 workload integration tests (C host tests for benchmark logic)
network-benchmark-workload-test: tests/network_benchmark-host
	./tests/network_benchmark-host --self-test
	python3 -m unittest tests.test_network_benchmark_integration -v

# MS16 ASan/UBSan host build
tests/network_benchmark-host-asan: tests/network_benchmark.c \
		tests/network_benchmark_protocol.c tests/network_benchmark_protocol.h \
		tests/network_benchmark_platform.c tests/network_benchmark_platform.h
	cc -std=c11 -Wall -Wextra -Werror -O1 -g \
		-fsanitize=address,undefined -fno-omit-frame-pointer \
		-D_BSD_SOURCE -D_DEFAULT_SOURCE -DNB_HOST_BUILD \
		tests/network_benchmark.c \
		tests/network_benchmark_protocol.c \
		tests/network_benchmark_platform.c \
		-o $@

# MS03 IRQ probe (RISC-V static — user boundary, requires musl cross toolchain)
tests/ms03_irq_probe: tests/ms03_irq_probe.c
	$(BENCH_CC) -static -Os -o $@ $<

# MS04 async RX probe (RISC-V static — built automatically, run manually)
tests/ms04_rx_probe: tests/ms04_rx_probe.c
	$(BENCH_CC) -std=c11 -Wall -Wextra -Werror -static -no-pie -Os -o $@ $<

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

lichee-fullbench-command: benchmark-fullbench-elf
	$(MAKE) ARCH=riscv64 APP_FEATURES=lichee-d1-fullbench-command MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$(PWD)/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
	@echo "Packing Android boot image (fullbench command-entry)..."
	@python3 tools/android_boot_image.py pack \
		--kernel StarryOS_riscv64-lichee-d1.bin \
		--output starry-lichee-fullbench-command-boot.img
	@python3 tools/android_boot_image.py inspect starry-lichee-fullbench-command-boot.img

# MS16 calibration preflight (no QEMU automation)
network-benchmark-calibration-preflight: tests/network_benchmark-host tests/network_benchmark \
		.claude/runbooks/network-benchmark-platform-qualification.md
	@echo "=== MS16 Calibration Preflight ==="
	@file tests/network_benchmark-host tests/network_benchmark StarryOS_riscv64-qemu-virt.bin make/disk.img
	@sha256sum tests/network_benchmark-host tests/network_benchmark \
		StarryOS_riscv64-qemu-virt.bin make/disk.img
	@qemu-system-riscv64 --version | head -n 1
	@echo "machine=virt smp=1 memory_mb=1024 icount=n bus=mmio"
	@cat .claude/runbooks/network-benchmark-platform-qualification.md
	@echo "=== Preflight complete ==="

.PHONY: build run justrun debug disasm clean host-test network-benchmark-test network-benchmark-local-test network-benchmark-workload-test network-benchmark-calibration-preflight lichee lichee-kbench lichee-userbench lichee-fullbench-mem lichee-fullbench-command benchmark-userbench-elf benchmark-fullbench-elf
