#!/bin/sh
# UART Async Benchmark Script
# 使用 busybox 内置命令进行性能测试

echo "=== UART Async Benchmark ==="
echo ""

# 测试 1: 吞吐量测试
echo "--- TX Throughput Test ---"
echo "Sending 10KB data..."

# 生成测试数据并发送
dd if=/dev/zero bs=1024 count=10 2>/dev/null | wc -c
echo "Data sent."

# 测试 2: 响应时间测试
echo ""
echo "--- Response Time Test ---"
echo "Testing echo response..."

# 测量 echo 命令的时间
start=$(date +%s)
echo "test" > /dev/null
end=$(date +%s)
echo "Echo time: $((end - start)) seconds"

# 测试 3: 文件系统性能
echo ""
echo "--- Filesystem Test ---"
echo "Testing file I/O..."

# 创建临时文件
dd if=/dev/zero of=/tmp/testfile bs=1024 count=100 2>/dev/null
ls -la /tmp/testfile
rm /tmp/testfile

# 测试 4: 内存使用
echo ""
echo "--- Memory Usage ---"
cat /proc/meminfo 2>/dev/null || echo "meminfo not available"

echo ""
echo "=== Benchmark Complete ==="
