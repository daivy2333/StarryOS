# MIG-20260720 Legacy Spec Migration — Coverage Checklist

## Migration Summary

| Legacy Source | Lines | Hash | Target(s) | Status |
|---|---|---|---|---|
| `openspec/specs/architecture/spec.md` | 1053 | `5b054d98` | project-model (M01-M40) + decisions (D01-D21) | mapped ✅ |
| `openspec/specs/learned/spec.md` | 1162 | `f09d4cae` | knowledge (K01-K27) + references (R28-R34) | mapped ✅ |
| `openspec/specs/optimization/spec.md` | 439 | `2ffa3af2` | improvements (I01-I10) | mapped ✅ |

## Coverage Statistics

- **Total legacy lines**: 2654
- **Total information units**: 130 (40 ADRs + 70 learned entries + 20 optimization entries)
- **Mapped units**: 130
- **Unmapped**: 0
- **Skipped**: 0
- **Coverage**: 100%

## Numbering Map

### Architecture → Project Model (M)
| Legacy ADR | New M ID | Description |
|---|---|---|
| A001 | M01 | Async runtime selection |
| A003 | M02 | VFS interface (DeviceOps) |
| A004 | M03 | Buffer strategy |
| A005 | M04 | termios support |
| A006 | M05 | Hardware abstraction |
| A012 | M06 | DMA strategy |
| A013 | M07 | Kernel log sync constraint |
| A024 | M08 | MMIO permissions |
| A026 | M09 | NS16550 stride |
| A030 | M10 | Console/Async coexistence |
| A031 | M11 | RX test methodology |
| A033/A036 | M12/M14 | uart_16550 crate / 2-trait OS abstraction |
| A034 | M13 | LTO deferred |
| A037 | M15 | TxCompletion 4-stage drain |
| A038 | M16 | TtyWrite short write contract |
| A039 | M17 | Incremental reintegration |
| A044 | M18 | Platform descriptor |
| A045 | M19 | D1 axplat boot |
| A046 | M20 | D1 C906 PTE flags |
| A047 | M21 | Q19B embedded benchmark |
| A048 | M22 | D1 platform UartPort |
| A049 | M23 | D1 userbench runtime |
| A050 | M24 | D1 feature separation |
| A051 | M25 | D1 THRE edge loss |
| A052 | M26 | Q19C memory-root path |
| A053 | M27 | D1 P99 tail |
| A054 | M28 | Q19C-M1 FS API |
| A055 | M29 | Q19C-M2 command-entry |
| A057 | M30 | Q20 benchmark only |
| A058 | M31 | Q21/Q22 cancelled |
| A059 | M32 | lint/test gate layered |
| A060 | M33 | io_uring homology |
| A061 | M34 | UART backpressure phased |
| A062 | M35 | TX/RX concurrency separation |
| A063 | M36 | Async NIC layered |
| A040 | M37 | PLIC/Clock trust-u-boot |
| A041 | M38 | PLIC defensive design |
| A042 | M39 | SMP atomic ordering |
| A043 | M40 | Lichee RV Dock boot chain |

### Architecture → Decisions (D)
| Legacy ADR | New D ID |
|---|---|
| A001 | D01 |
| A003 | D02 |
| A004 | D03 |
| A005 | D04 |
| A006/A033/A036 | D05 |
| A012 | D06 |
| A013 | D07 |
| A024/A026 | D08 |
| A029/A030 | D09 |
| A033 | D10 |
| A034 | D11 |
| A037 | D12 |
| A038 | D13 |
| A039 | D14 |
| A044-A046 | D15 |
| A047-A055 | D16 |
| A057/A058 | D17 |
| A060 | D18 |
| A061/A062 | D19 |
| A063 | D20 |
| A040/A041 | D21 |

### Optimization → Improvements (I)
| Legacy O/Milestone | New I ID | Status |
|---|---|---|
| O77 | I01 | 🟡 active |
| O82 | I02 | 🧊 deferred |
| O85 | I03 | 🧊 deferred |
| O86 | I04 | 🧊 deferred |
| O63 | I05 | ⚠️ partial |
| O64-O66/O69/O71 | I06 | ⏳ waiting |
| O17/OE1-OE5 | I07 | ❌ rejected |
| O1/O5/O32/O36/O37/O54/O55 | I08 | 远期 |
| O58-O60 | I09 | 探索中 |
| Q26/O48-O50 | I10 | ✅ completed |

## Tombstoned Legacy Entries

All tombstoned ADRs and learned entries are preserved in their respective archive carriers:
- ARC-202607021648, ARC-202607021535, ARC-202607031929, ARC-202607081429, ARC-202607111510, arc-202607152005

## Verification

- `openspec validate --specs`: 25 passed, 0 failed ✅
- `unmapped = 0` ✅
- `skipped = 0` ✅
- Coverage = 100% ✅
- Legacy source hashes unchanged ✅
