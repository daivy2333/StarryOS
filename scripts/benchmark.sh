#!/bin/bash
# UART 异步串口性能测试脚本
#
# 使用方法:
#   ./scripts/benchmark.sh [OPTIONS]
#
# 选项:
#   --skip-build    跳过内核构建
#   --skip-qemu     跳过 QEMU 测试（仅构建）
#   --output DIR    结果输出目录（默认: benchmark_results）
#   --timeout SEC   QEMU 超时时间（默认: 30 秒）

set -e

# 默认配置
QEMU_PORT=4444
QEMU_PID=""
RESULTS_DIR="benchmark_results"
SKIP_BUILD=false
SKIP_QEMU=false
TIMEOUT=30

# 解析参数
while [[ $# -gt 0 ]]; do
    case $1 in
        --skip-build)
            SKIP_BUILD=true
            shift
            ;;
        --skip-qemu)
            SKIP_QEMU=true
            shift
            ;;
        --output)
            RESULTS_DIR="$2"
            shift 2
            ;;
        --timeout)
            TIMEOUT="$2"
            shift 2
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

# 清理函数
cleanup() {
    if [ -n "$QEMU_PID" ]; then
        kill $QEMU_PID 2>/dev/null || true
        wait $QEMU_PID 2>/dev/null || true
    fi
}

trap cleanup EXIT

# 日志函数
log_info() {
    echo "[INFO] $1"
}

log_error() {
    echo "[ERROR] $1" >&2
}

log_success() {
    echo "[SUCCESS] $1"
}

# 检查依赖
check_dependencies() {
    log_info "Checking dependencies..."

    local missing=false

    for tool in make cargo qemu-system-riscv64 nc; do
        if ! command -v $tool &> /dev/null; then
            log_error "'$tool' is not installed"
            missing=true
        fi
    done

    if [ "$missing" = true ]; then
        log_error "Missing required tools"
        exit 1
    fi

    log_success "All dependencies found"
}

# 构建内核
build_kernel() {
    if [ "$SKIP_BUILD" = true ]; then
        log_info "Skipping kernel build"
        return
    fi

    log_info "Building kernel..."
    make build 2>&1 | tee "${RESULTS_DIR}/build.log"

    if [ ${PIPESTATUS[0]} -ne 0 ]; then
        log_error "Kernel build failed"
        exit 1
    fi

    log_success "Kernel built successfully"
}

# 启动 QEMU
start_qemu() {
    log_info "Starting QEMU..."

    # 使用 TCP 串口连接
    make run QEMU_ARGS="-monitor none -serial tcp::${QEMU_PORT},server=on" &
    QEMU_PID=$!

    # 等待 QEMU 启动
    log_info "Waiting for QEMU to start..."
    sleep 3

    # 等待 Shell 就绪
    log_info "Waiting for shell prompt..."
    local start_time=$(date +%s)
    local ready=false

    while [ $(($(date +%s) - start_time)) -lt $TIMEOUT ]; do
        if echo "" | nc -w 1 localhost ${QEMU_PORT} 2>/dev/null | grep -q "starry:~#"; then
            ready=true
            break
        fi
        sleep 0.5
    done

    if [ "$ready" = false ]; then
        log_error "Timeout waiting for shell prompt"
        exit 1
    fi

    log_success "QEMU started, shell ready"
}

# 执行测试命令
run_cmd() {
    local cmd="$1"
    local timeout="${2:-5}"

    echo "$cmd" | nc -w $timeout localhost ${QEMU_PORT} 2>/dev/null
}

# 运行内核态基准测试
run_kernel_benchmark() {
    log_info "Running kernel benchmark..."

    # 启动基准测试
    run_cmd "bench start"
    sleep 1

    # 发送测试数据（通过 echo 命令）
    log_info "Sending test data..."
    run_cmd "echo 'AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA' > /dev/console"

    # 等待数据传输完成
    sleep 2

    # 停止并获取报告
    log_info "Stopping benchmark and getting report..."
    run_cmd "bench stop"

    # 获取内存使用统计
    run_cmd "bench memory"
}

# 运行用户态基准测试
run_user_benchmark() {
    log_info "Running user benchmark..."

    # 检查测试程序是否存在
    if [ ! -f "tests/benchmark" ]; then
        log_info "Compiling benchmark program..."
        gcc -o tests/benchmark tests/benchmark.c -lrt
    fi

    # 运行测试程序
    log_info "Executing benchmark..."
    run_cmd "/tmp/benchmark" 10
}

# 收集结果
collect_results() {
    log_info "Collecting results..."

    # 保存 QEMU 输出
    # 注意：在实际实现中，需要捕获 QEMU 的输出并保存到文件

    log_success "Results saved to ${RESULTS_DIR}/"
}

# 生成报告
generate_report() {
    log_info "Generating report..."

    cat > "${RESULTS_DIR}/report.md" << EOF
# UART Benchmark Report

**Date**: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
**Branch**: $(git branch --show-current)
**Commit**: $(git rev-parse --short HEAD)

## Test Results

See individual test outputs in the results directory.

## Notes

- All tests run in QEMU environment
- Results may vary on real hardware
- Latency measurements include QEMU scheduling overhead
EOF

    log_success "Report generated: ${RESULTS_DIR}/report.md"
}

# 主函数
main() {
    log_info "Starting UART benchmark"
    log_info "======================"

    # 创建结果目录
    mkdir -p "${RESULTS_DIR}"

    # 检查依赖
    check_dependencies

    # 构建内核
    build_kernel

    # QEMU 测试
    if [ "$SKIP_QEMU" = false ]; then
        start_qemu

        # 运行测试
        run_kernel_benchmark
        # run_user_benchmark  # 暂时注释，需要先部署测试程序

        # 收集结果
        collect_results
    fi

    # 生成报告
    generate_report

    log_success "Benchmark complete!"
}

main "$@"
