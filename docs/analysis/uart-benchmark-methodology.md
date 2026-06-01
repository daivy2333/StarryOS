# UART 串口性能测试方法论

> 项目：StarryOS
> 日期：2026-06-01
> 目的：设计精确的性能测试方法，对比 Console 阻塞串口和 Async 异步串口

---

## 1. 两种串口架构对比

### 1.1 Console 阻塞串口

```
用户态 write()
    ↓
axhal::console::write_bytes()
    ↓
逐字节处理（获取锁）
    ↓
uart.send_raw()
    ↓
retry_until_ok!(try_send_raw())
    ↓
轮询等待 THR_EMPTY
    ↓
写入 THR 寄存器
```

**特点**：
- 同步阻塞
- 逐字节发送
- CPU 忙等（轮询）
- 无缓冲区
- 每个字节都获取锁

### 1.2 Async 异步串口

```
用户态 write()
    ↓
Tty::write_at()
    ↓
AsyncUartWriter::write()
    ↓
Ring Buffer push（无锁快速路径）
    ↓
TX copier 任务被唤醒
    ↓
批量发送（send_bytes）
    ↓
FIFO 满时 enable_tx_intr()
    ↓
ISR 中断唤醒 copier 继续
```

**特点**：
- 异步非阻塞
- 批量发送
- 中断驱动
- 128 KB 缓冲区（RX/TX 各 64 KB）
- 单次锁操作

---

## 2. 性能指标定义

### 2.1 吞吐量（Throughput）

**定义**：单位时间内成功传输的数据量

**测量方法**：
- **TX 吞吐量**：从用户态写入 N 字节到 `/dev/console`，测量总时间
- **RX 吞吐量**：从外部发送 N 字节到串口，测量用户态接收时间

**单位**：KB/s 或 Mbps

**理论极限**：
- 115200 bps = 11.52 KB/s（10 bits/byte）
- 230400 bps = 23.04 KB/s
- 921600 bps = 92.16 KB/s

### 2.2 延迟（Latency）

**定义**：从发送请求到数据实际到达的时间

**测量方法**：
- **单字节延迟**：发送单个字节，测量从 write() 到数据出现在串口的时间
- **往返延迟**：发送字节并等待回显，测量往返时间

**单位**：微秒（µs）或毫秒（ms）

**组成**：
- 软件延迟（系统调用、驱动处理）
- 硬件延迟（FIFO、传输时间）
- 传播延迟（线缆、距离）

### 2.3 内存占用（Memory Usage）

**定义**：驱动使用的内存空间

**测量方法**：
- **静态内存**：代码段、数据段大小
- **动态内存**：Ring Buffer、锁、任务栈等

**单位**：字节（B）或千字节（KB）

### 2.4 CPU 占用（CPU Usage）

**定义**：驱动处理占用的 CPU 时间比例

**测量方法**：
- **忙等时间**：轮询等待的时间
- **中断时间**：ISR 处理时间
- **任务时间**：copier 任务执行时间

**单位**：百分比（%）

### 2.5 中断频率（IRQ Frequency）

**定义**：单位时间内的中断次数

**测量方法**：在 ISR 中递增计数器，统计单位时间内的中断次数

**单位**：次/秒（IRQ/s）

---

## 3. 测试场景设计

### 3.1 场景 1：TX 吞吐量测试

**目标**：测量从用户态到硬件的完整 TX 路径吞吐量

**方法**：
```c
// 用户态测试程序
int fd = open("/dev/console", O_WRONLY);
char buf[1024];
memset(buf, 'A', sizeof(buf));

clock_gettime(CLOCK_MONOTONIC, &start);
for (int i = 0; i < 100; i++) {
    write(fd, buf, sizeof(buf));
}
clock_gettime(CLOCK_MONOTONIC, &end);

elapsed = end - start;
throughput = (100 * 1024) / elapsed;
```

**测量内容**：
- 总数据量：100 KB
- 总耗时
- 平均吞吐量

**预期结果**：
- Console：受限于串口线速（~11.52 KB/s）
- Async：受限于串口线速（~11.52 KB/s）
- 两者应该相近，因为都受限于硬件

### 3.2 场景 2：RX 吞吐量测试

**目标**：测量从硬件到用户态的完整 RX 路径吞吐量

**方法**：
```c
// 用户态测试程序
int fd = open("/dev/console", O_RDONLY);
char buf[1024];

clock_gettime(CLOCK_MONOTONIC, &start);
size_t total = 0;
while (total < 102400) {
    ssize_t n = read(fd, buf, sizeof(buf));
    if (n > 0) total += n;
}
clock_gettime(CLOCK_MONOTONIC, &end);

elapsed = end - start;
throughput = total / elapsed;
```

**测量内容**：
- 总数据量：100 KB
- 总耗时
- 平均吞吐量

**预期结果**：
- Console：受限于串口线速（~11.52 KB/s）
- Async：受限于串口线速（~11.52 KB/s）

### 3.3 场景 3：延迟测试

**目标**：测量单字节 echo 的端到端延迟

**方法**：
```c
// 用户态测试程序
int fd = open("/dev/console", O_RDWR);

for (int i = 0; i < 100; i++) {
    clock_gettime(CLOCK_MONOTONIC, &start);
    write(fd, "A", 1);
    read(fd, &rx, 1);
    clock_gettime(CLOCK_MONOTONIC, &end);
    latencies[i] = end - start;
}

// 计算统计值
sort(latencies);
p50 = latencies[50];
p95 = latencies[95];
p99 = latencies[99];
```

**测量内容**：
- 100 次单字节 echo
- 计算 P50、P95、P99 延迟

**预期结果**：
- Console：较低延迟（直接轮询）
- Async：较高延迟（中断 + 任务调度）

### 3.4 场景 4：内存占用测试

**目标**：测量驱动的内存占用

**方法**：
```rust
// 内核态测量
fn memory_usage() {
    let rx_buf = BUF_SIZE; // 64 KB 或 0
    let tx_buf = BUF_SIZE; // 64 KB 或 0
    let driver = size_of::<Driver>();
    let total = rx_buf + tx_buf + driver;

    ax_println!("RX Buffer: {} KB", rx_buf / 1024);
    ax_println!("TX Buffer: {} KB", tx_buf / 1024);
    ax_println!("Driver: {} bytes", driver);
    ax_println!("Total: {} KB", total / 1024);
}
```

**测量内容**：
- RX Buffer 大小
- TX Buffer 大小
- 驱动结构体大小
- 总内存占用

**预期结果**：
- Console：0 KB（无缓冲区）
- Async：128 KB（RX/TX 各 64 KB）

### 3.5 场景 5：CPU 占用测试

**目标**：测量驱动的 CPU 占用

**方法**：
- **Console**：测量轮询等待时间
- **Async**：测量 ISR + copier 任务时间

**测量内容**：
- 忙等时间占比
- 中断处理时间
- 任务调度时间

**预期结果**：
- Console：高 CPU 占用（忙等）
- Async：低 CPU 占用（中断驱动）

### 3.6 场景 6：中断频率测试

**目标**：测量中断频率

**方法**：
```rust
// 内核态测量
static IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

fn record_irq() {
    IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
}

fn get_irq_count() -> u64 {
    IRQ_COUNT.load(Ordering::Relaxed)
}
```

**测量内容**：
- 启动时中断次数
- 数据传输时中断频率
- NAPI 效果

**预期结果**：
- Console：无中断（轮询）
- Async：有中断（中断驱动）

---

## 4. 测试实现方案

### 4.1 统一测试框架

**目标**：在两个分支上使用相同的测试方法

**实现**：
1. 创建统一的用户态测试程序（C 语言）
2. 编译为静态链接的 ELF
3. 添加到 rootfs
4. 在 Shell 中执行

**测试程序结构**：
```c
// tests/uart_benchmark.c
#include <stdio.h>
#include <fcntl.h>
#include <unistd.h>
#include <time.h>

int main() {
    printf("UART Benchmark\n");
    test_throughput_tx();
    test_throughput_rx();
    test_latency();
    test_memory();
    return 0;
}
```

### 4.2 内核态统计模块

**目标**：收集内核态性能数据

**实现**：
```rust
// kernel/src/drivers/benchmark.rs
pub struct BenchmarkStats {
    pub tx_bytes: AtomicU64,
    pub rx_bytes: AtomicU64,
    pub irq_count: AtomicU64,
    pub start_time: AtomicU64,
}

impl BenchmarkStats {
    pub fn start(&self) { ... }
    pub fn stop(&self) { ... }
    pub fn record_tx(&self, bytes: u64) { ... }
    pub fn record_rx(&self, bytes: u64) { ... }
    pub fn record_irq(&self) { ... }
    pub fn report(&self) { ... }
}
```

### 4.3 自动化测试脚本

**目标**：自动执行测试并收集结果

**实现**：
```bash
#!/bin/bash
# scripts/benchmark.sh

# 构建
make build MODE=release

# 启动 QEMU
make run QEMU_ARGS="-monitor none -serial tcp::4444,server=on" &

# 等待启动
sleep 3

# 连接并执行测试
nc localhost 4444 << EOF
/benchmark
exit
EOF

# 收集结果
parse_results()
```

---

## 5. 测试结果对比表

### 5.1 预期对比

| 指标 | Console (阻塞) | Async (异步) | 差异原因 |
|------|---------------|--------------|----------|
| **TX 吞吐量** | ~11.52 KB/s | ~11.52 KB/s | 都受限于硬件线速 |
| **RX 吞吐量** | ~11.52 KB/s | ~11.52 KB/s | 都受限于硬件线速 |
| **单字节延迟** | 低 | 中 | Async 有中断+调度开销 |
| **内存占用** | 0 KB | 128 KB | Async 有 Ring Buffer |
| **CPU 占用** | 高 | 低 | Console 轮询忙等 |
| **中断频率** | 0 | ~823/s | Async 使用中断驱动 |
| **NAPI 效果** | N/A | 减少 90%+ IRQ | 高吞吐时切轮询 |
| **批量传输** | 逐字节 | 批量 | Async 有 Ring Buffer |

### 5.2 实际测量值（待填写）

| 指标 | Console | Async | 测量方法 |
|------|---------|-------|----------|
| TX 吞吐量 | | | 用户态 write 100KB |
| RX 吞吐量 | | | 用户态 read 100KB |
| P50 延迟 | | | 100 次 echo |
| P95 延迟 | | | 100 次 echo |
| P99 延迟 | | | 100 次 echo |
| 内存占用 | | | 内核态统计 |
| CPU 占用 | | | 待实现 |
| ISR Count | | | 内核态统计 |

---

## 6. 测试注意事项

### 6.1 QEMU 限制

- QEMU 的串口模拟不完全等同于真实硬件
- 时钟精度有限，延迟测量可能有误差
- 建议在真板（VisionFive2）上验证关键指标

### 6.2 测试环境

- 确保 QEMU 使用 `-nographic` 模式
- 使用 TCP 串口连接便于自动化
- 测试期间避免其他进程干扰

### 6.3 结果解读

- 吞吐量目标 90% 线速是合理的（考虑协议开销）
- 延迟 P99 < 2ms 是可接受的（考虑 QEMU 调度）
- 内存占用 128KB 是固定的（Ring Buffer 配置）

### 6.4 公平对比

**确保对比公平**：
1. 相同的测试数据
2. 相同的测量方法
3. 相同的测试环境
4. 相同的统计指标

**避免偏差**：
1. 不要比较不同层次的指标（如 ring buffer vs 硬件）
2. 要比较相同路径的指标（如用户态到硬件）
3. 记录测量方法和环境

---

## 7. 实现状态

### 7.1 已实现

**内核态测试**：
- ✅ Console polling TX 测试
- ✅ Async Ring Buffer 写入测试
- ✅ 内存占用统计
- ✅ 硬件理论极限说明

**用户态测试**：
- ✅ 测试程序代码（tests/uart_benchmark.c）
- ✅ RISC-V 静态编译（tests/benchmark）
- ✅ 添加到 rootfs 脚本（scripts/add_benchmark_to_rootfs.sh）

### 7.2 使用方法

**编译测试程序**：
```bash
/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc -static -o tests/benchmark tests/uart_benchmark.c
```

**添加到 rootfs**：
```bash
sudo ./scripts/add_benchmark_to_rootfs.sh
```

**运行测试**：
```bash
# 启动 QEMU
make run QEMU_ARGS="-monitor none -serial tcp::4444,server=on"

# 连接串口
nc localhost 4444

# 执行测试
/benchmark
```

### 7.3 测试项目

**用户态测试程序包含**：
1. **TX 吞吐量**：write() 100KB 到 /dev/console
2. **RX 吞吐量**：从 /dev/console 读取数据（需要外部注入）
3. **延迟测试**：100 次单字节 echo，计算 P50/P95/P99
4. **数据完整性**：验证 256 字节传输的正确性

---

## 8. 结论

### 8.1 测试方法总结

**内核态测试**（已实现）：
1. polling TX 速度（Console）
2. Ring Buffer 写入速度（Async）
3. 内存占用统计

**用户态测试**（已实现）：
1. 吞吐量：通过 write/read 测量完整路径
2. 延迟：通过 echo 测量往返时间
3. 数据完整性：验证传输正确性

### 8.2 预期结论

- **吞吐量**：两者相近（都受限于硬件）
- **延迟**：Console 可能更低（轮询）
- **内存**：Console 更省内存
- **CPU**：Async 更省 CPU
- **中断**：Async 使用中断驱动

### 8.3 选择建议

- **低延迟场景**：Console 可能更优
- **高吞吐场景**：Async 更优（批量处理）
- **低 CPU 场景**：Async 更优（中断驱动）
- **低内存场景**：Console 更优（无缓冲区）

---

**文档版本**：1.1
**最后更新**：2026-06-01
