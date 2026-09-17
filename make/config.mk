# Config generation

# PLIC MMIO window override applied only on the default RISC-V QEMU virt board.
# QEMU's device tree declares the PLIC at 0x0c00_0000 with size 0x600000, while
# the shipped platform config only maps 0x21_0000. SMP=16 boots hart 11, whose
# supervisor context registers sit beyond the old window and page-fault during
# `init_percpu` (before scheduler init, masked as an uninitialized current-task
# panic). The override rewrites only the PLIC entry of `devices.mmio-ranges`;
# the user `EXTRA_CONFIG` value, when present, is merged before this and so this
# `-w` argument is applied last, matching "overlay corrects the platform fact".
QEMU_OVERLAY_ARG :=
ifeq ($(strip $(PLAT_NAME)), riscv64-qemu-virt)
  QEMU_OVERLAY_ARG := -w 'devices.mmio-ranges=[[0x0010_1000, 0x1000], [0x0c00_0000, 0x60_0000], [0x1000_0000, 0x1000], [0x1000_1000, 0x8000], [0x3000_0000, 0x1000_0000], [0x4000_0000, 0x4000_0000]]'
endif

config_args := \
  defconfig.toml $(PLAT_CONFIG) $(EXTRA_CONFIG) \
  $(QEMU_OVERLAY_ARG) \
  -w 'arch="$(ARCH)"' \
  -w 'platform="$(PLAT_NAME)"' \
  -o "$(OUT_CONFIG)"

ifneq ($(MEM),)
  config_args += -w 'plat.phys-memory-size=$(shell ./strtosz.py $(MEM))'
else
  MEM := $(shell axconfig-gen $(PLAT_CONFIG) -r plat.phys-memory-size 2>/dev/null | tr -d _ | xargs printf "%dB")
endif

ifneq ($(KERNEL_BASE_PADDR),)
  config_args += -w 'plat.kernel-base-paddr=$(KERNEL_BASE_PADDR)'
endif

ifneq ($(SMP),)
  config_args += -w 'plat.max-cpu-num=$(SMP)'
else
  SMP := $(shell axconfig-gen $(PLAT_CONFIG) -r plat.max-cpu-num 2>/dev/null)
  ifeq ($(SMP),)
    $(error "`plat.max-cpu-num` is not defined in the platform configuration file, \
      this option must be specified even for platforms with runtime CPU detection.")
  endif
endif

define defconfig
  $(call run_cmd,axconfig-gen,$(config_args))
endef

ifeq ($(wildcard $(OUT_CONFIG)),)
  define oldconfig
    $(call defconfig)
  endef
else
  define oldconfig
    $(if $(filter "$(PLAT_NAME)",$(shell axconfig-gen "$(OUT_CONFIG)" -r platform)),\
         $(call run_cmd,axconfig-gen,$(config_args) -c "$(OUT_CONFIG)"),\
         $(error "ARCH" or "MYPLAT" has been changed, please run "make defconfig" again))
  endef
endif
