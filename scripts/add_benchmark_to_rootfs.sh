#!/bin/bash
# 添加 benchmark 测试程序到 rootfs
#
# 使用方法：
#   sudo ./scripts/add_benchmark_to_rootfs.sh

set -e

DISK_IMG="make/disk.img"
MOUNT_POINT="/tmp/rootfs_mount"
BENCHMARK_BIN="tests/benchmark"

# 检查是否以 root 运行
if [ "$EUID" -ne 0 ]; then
    echo "请使用 sudo 运行此脚本"
    exit 1
fi

# 检查文件是否存在
if [ ! -f "$DISK_IMG" ]; then
    echo "错误：$DISK_IMG 不存在"
    exit 1
fi

if [ ! -f "$BENCHMARK_BIN" ]; then
    echo "错误：$BENCHMARK_BIN 不存在，请先编译"
    exit 1
fi

# 创建挂载点
mkdir -p "$MOUNT_POINT"

# 挂载磁盘镜像
echo "挂载磁盘镜像..."
mount -o loop "$DISK_IMG" "$MOUNT_POINT"

# 复制 benchmark 程序
echo "复制 benchmark 程序..."
cp "$BENCHMARK_BIN" "$MOUNT_POINT/bin/"
chmod +x "$MOUNT_POINT/bin/benchmark"

# 卸载
echo "卸载磁盘镜像..."
umount "$MOUNT_POINT"

echo "完成！benchmark 程序已添加到 rootfs"
echo "在 StarryOS Shell 中运行：/benchmark"
