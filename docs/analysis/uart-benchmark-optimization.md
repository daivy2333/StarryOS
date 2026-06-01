# UART 性能测试优化方案

> 项目：StarryOS
> 分支：feat/uart-async-bench
> 日期：2026-06-01

---

## 1. 当前测试问题分析

### 1.1 用户态测试问题

**问题 1：数据泄漏**
```
AAAAAAAAAAAA...
```
- write() 的数据被 QEMU 回显到终端
- 影响测试结果的可读性

**问题 2：延迟测试不准确**
```
Mismatch at iteration 0: sent A, got 
```
- read() 读取的是终端输入，不是 echo 回显
- 无法测量真正的 echo 延迟

**问题 3：吞吐量测量不准确**
```
Throughput: 46,296 KB/s
Line rate: 401,877%
```
- write() 不等待硬件，立即返回
- 测量的是 write() 系统调用速度，不是串口线速

### 1.2 内核态测试问题

**问题 1：缺少 CPU 占用测量**
- 当前只测量了 write() 速度
- 没有测量 CPU 占用率

**问题 2：中断频率统计不完善**
- 当前有 ISR_COUNT，但没有频率计算
- 没有 NAPI 效果对比

**问题 3：缺少不同场景测试**
- 只测试了固定数据大小
- 没有测试不同负载下的性能

---

## 2. 优化方案

### 2.1 CPU 占用测量

**方法 1：使用 RISC-V cycle 计数器**

```rust
/// 读取 RISC-V cycle 计数器
fn read_cycle() -> u64 {
    // RISC-V 的 cycle CSR
    let cycle: u64;
    unsafe {
        core::arch::asm!("csrr {}, cycle", out(reg) cycle);
    }
    cycle
}

/// 测量 CPU 周期数
pub fn measure_cpu_cycles<F: FnOnce()>(f: F) -> u64 {
    let start = read_cycle();
    f();
    let end = read_cycle();
    end - start
}
```

**方法 2：测量 idle 时间**

```rust
/// 测量 idle 时间占比
pub fn measure_idle_time() {
    let start = monotonic_time_nanos();

    // 执行测试
    // ...

    let end = monotonic_time_nanos();
    let total_time = end - start;

    // 计算 idle 时间
    // idle_time = total_time - busy_time
    // cpu_usage = busy_time / total_time * 100%
}
```

### 2.2 中断频率统计

**当前实现**：
```rust
static IRQ_COUNT: AtomicU64 = AtomicU64::new(0);

pub fn record_irq() {
    IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
}

pub fn get_irq_count() -> u64 {
    IRQ_COUNT.load(Ordering::Relaxed)
}
```

**优化方案**：
```rust
/// 中断频率统计
static IRQ_COUNT: AtomicU64 = AtomicU64::new(0);
static IRQ_START_TIME: AtomicU64 = AtomicU64::new(0);
static IRQ_END_TIME: AtomicU64 = AtomicU64::new(0);

/// 记录中断并计算频率
pub fn record_irq() {
    IRQ_COUNT.fetch_add(1, Ordering::Relaxed);
    IRQ_END_TIME.store(monotonic_time_nanos(), Ordering::Relaxed);
}

/// 计算中断频率
pub fn get_irq_frequency() -> f64 {
    let count = IRQ_COUNT.load(Ordering::Relaxed);
    let start = IRQ_START_TIME.load(Ordering::Relaxed);
    let end = IRQ_END_TIME.load(Ordering::Relaxed);

    if start == 0 || end == 0 || end <= start {
        return 0.0;
    }

    let elapsed_s = (end - start) as f64 / 1_000_000_000.0;
    count as f64 / elapsed_s
}

/// NAPI 效果对比
pub fn compare_napi_effect() {
    // 测试 1: 关闭 NAPI
    set_napi_threshold(u32::MAX);
    let irq_count_without_napi = measure_irq_count();

    // 测试 2: 开启 NAPI
    set_napi_threshold(16);
    let irq_count_with_napi = measure_irq_count();

    // 计算减少比例
    let reduction = (irq_count_without_napi - irq_count_with_napi) as f64
                    / irq_count_without_napi as f64 * 100.0;

    ax_println!("[BENCH] NAPI Effect:");
    ax_println!("  Without NAPI: {} IRQs", irq_count_without_napi);
    ax_println!("  With NAPI: {} IRQs", irq_count_with_napi);
    ax_println!("  Reduction: {:.1}%", reduction);
}
```

### 2.3 用户态测试优化

**优化 1：避免数据泄漏**

```c
// 使用不可见字符
memset(buf, 0, test_size);  // 使用 0x00 而不是 'A'

// 或者使用 /dev/null
int fd = open("/dev/null", O_WRONLY);
write(fd, buf, test_size);
```

**优化 2：正确的延迟测试**

```c
// 方法 1: 测量 write() 延迟（当前方法）
for (int i = 0; i < 100; i++) {
    clock_gettime(CLOCK_MONOTONIC, &start);
    write(fd, &tx, 1);
    clock_gettime(CLOCK_MONOTONIC, &end);
    latencies[i] = end - start;
}

// 方法 2: 测量 echo 延迟（需要终端支持）
// 设置终端为原始模式
struct termios raw;
tcgetattr(fd, &raw);
raw.c_lflag &= ~(ECHO | ICANON);
tcsetattr(fd, TCSAFLUSH, &raw);

for (int i = 0; i < 100; i++) {
    clock_gettime(CLOCK_MONOTONIC, &start);
    write(fd, "A", 1);
    read(fd, &rx, 1);  // 等待回显
    clock_gettime(CLOCK_MONOTONIC, &end);
    latencies[i] = end - start;
}
```

**优化 3：更准确的吞吐量测量**

```c
// 方法 1: 测量 write() 速度（当前方法）
// 结果：46,296 KB/s（不等待硬件）

// 方法 2: 测量实际吞吐量（需要等待发送完成）
// 使用 tcdrain() 等待数据发送
for (int i = 0; i < iterations; i++) {
    clock_gettime(CLOCK_MONOTONIC, &start);
    write(fd, buf, test_size);
    tcdrain(fd);  // 等待数据发送
    clock_gettime(CLOCK_MONOTONIC, &end);
    // 计算吞吐量
}

// 方法 3: 测量端到端吞吐量（需要接收端配合）
// 发送数据并等待接收确认
```

### 2.4 更全方位的测试

**测试 1：不同数据大小**

```c
void test_different_sizes() {
    int sizes[] = {1, 64, 256, 1024, 4096, 16384};

    for (int i = 0; i < 6; i++) {
        int size = sizes[i];
        printf("Testing size: %d bytes\n", size);

        // 执行测试
        test_throughput_with_size(size);
    }
}
```

**测试 2：不同负载**

```c
void test_different_loads() {
    int iterations[] = {1, 10, 100, 1000};

    for (int i = 0; i < 4; i++) {
        int iter = iterations[i];
        printf("Testing iterations: %d\n", iter);

        // 执行测试
        test_throughput_with_iterations(iter);
    }
}
```

**测试 3：并发测试**

```c
void test_concurrent() {
    // 创建多个线程同时写入
    pthread_t threads[4];

    for (int i = 0; i < 4; i++) {
        pthread_create(&threads[i], NULL, write_thread, NULL);
    }

    for (int i = 0; i < 4; i++) {
        pthread_join(threads[i], NULL);
    }
}
```

**测试 4：压力测试**

```c
void test_stress() {
    // 长时间持续写入
    int duration = 10;  // 10 秒
    time_t start = time(NULL);

    while (time(NULL) - start < duration) {
        write(fd, buf, test_size);
    }
}
```

---

## 3. 实现计划

### 3.1 内核态优化

**Step 1: 添加 CPU 占用测量**
```rust
// kernel/src/drivers/benchmark.rs
pub fn measure_cpu_cycles<F: FnOnce()>(f: F) -> u64 {
    // 使用 RISC-V cycle 计数器
}
```

**Step 2: 优化中断频率统计**
```rust
// kernel/src/drivers/uart_init.rs
pub fn get_irq_frequency() -> f64 {
    // 计算中断频率
}
```

**Step 3: 添加 NAPI 效果对比**
```rust
// kernel/src/drivers/benchmark.rs
pub fn compare_napi_effect() {
    // 对比 NAPI 开启/关闭的效果
}
```

### 3.2 用户态优化

**Step 1: 修复数据泄漏**
```c
// tests/benchmark.c
memset(buf, 0, test_size);  // 使用 0x00
```

**Step 2: 优化延迟测试**
```c
// 测量 write() 延迟（当前方法）
// 添加终端原始模式支持
```

**Step 3: 添加更多测试**
```c
// 测试不同数据大小
// 测试不同负载
// 测试并发
// 测试压力
```

### 3.3 文档更新

**Step 1: 更新测试方法论文档**
```markdown
// docs/analysis/uart-benchmark-methodology.md
// 添加新的测试方法和指标
```

**Step 2: 更新性能对比报告**
```markdown
// docs/uart-performance-comparison.md
// 添加新的测试结果
```

---

## 4. 预期结果

### 4.1 CPU 占用测量

| 指标 | Console | Async | 说明 |
|------|---------|-------|------|
| **CPU 周期数** | - | - | 待测量 |
| **CPU 占用率** | - | - | 待测量 |
| **Idle 时间** | - | - | 待测量 |

### 4.2 中断频率统计

| 指标 | 值 | 说明 |
|------|-----|------|
| **IRQ 频率** | - | 待测量 |
| **NAPI 减少** | - | 待测量 |
| **NAPI 阈值** | 16 | 当前配置 |

### 4.3 更全方位测试

| 测试项 | 结果 | 说明 |
|--------|------|------|
| **不同数据大小** | - | 待测试 |
| **不同负载** | - | 待测试 |
| **并发测试** | - | 待测试 |
| **压力测试** | - | 待测试 |

---

## 5. 实现状态

### 5.1 已实现

**内核态优化**：
- ✅ CPU 占用测量：使用 RISC-V cycle 计数器
- ✅ 中断频率统计：计算 IRQ/s
- ✅ NAPI 效果报告：显示 NAPI 配置和效果

**用户态优化**：
- ✅ 修复数据泄漏：使用 /dev/null 避免数据输出到终端
- ✅ 不同数据大小测试：测试 64、256、1024、4096 字节
- ✅ 压力测试：持续 2 秒的写入测试
- ✅ 优化输出格式：更清晰的测试结果

### 5.2 测试项目

**内核态测试**：
1. Ring Buffer 写入速度
2. CPU 周期数和占用率
3. 中断频率
4. NAPI 效果

**用户态测试**：
1. TX 吞吐量（不同数据大小）
2. write() 延迟
3. 数据完整性
4. 压力测试

### 5.3 使用方法

**编译**：
```bash
# 内核
make build MODE=release

# 用户态
/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc -static -o tests/benchmark tests/benchmark.c
```

**运行**：
```bash
# 添加到 rootfs
sudo ./scripts/add_benchmark_to_rootfs.sh

# 启动 QEMU
make run QEMU_ARGS="-monitor none -serial tcp::4444,server=on"

# 执行测试
cd /bin
./benchmark
```

---

## 6. 总结

### 6.1 优化完成

1. **CPU 占用测量**：✅ 已实现
2. **中断频率统计**：✅ 已实现
3. **用户态测试优化**：✅ 已实现
4. **更全方位测试**：✅ 已实现

### 6.2 预期收益

- **更准确的性能数据**：CPU 占用、中断频率
- **更全面的测试覆盖**：不同数据大小、压力测试
- **更可靠的测试结果**：修复数据泄漏问题

### 6.3 后续工作

1. **运行测试**：收集新的测试数据
2. **更新对比报告**：添加新的测试指标
3. **真板验证**：在 VisionFive2 上测试

---

**文档版本**：1.1
**最后更新**：2026-06-01
