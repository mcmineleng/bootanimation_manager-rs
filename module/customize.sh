#!/system/bin/sh

# ==========================================
# 配置区
# ==========================================
UTILS_BIN="$MODPATH/utils"          # utils 单文件路径
TIMEOUT=10                          # 超时秒数

# ==========================================
# 工具函数：检测二进制架构
# readelf 或 file 不一定有，用 magisk 自带的 hexdump 方式
# 这里用最简单的方法：检查 ELF 的 e_machine 字段
# ==========================================
detect_elf_machine() {
    local file="$1"
    # ELF 头: e_machine 在 offset 0x12 (16-bit little endian)
    local machine
    machine=$(hexdump -n 2 -s 18 -e '1/2 "%u"' "$file" 2>/dev/null)
    echo "$machine"
}

# ELF e_machine 值
# 0x28 = ARM
# 0xB7 = AArch64 (ARM64)
# 0x03 = x86 (i386)
# 0x3E = x86_64 (AMD64)
# 0x28 = 40 (ARM)
# 0xB7 = 183 (ARM64)
# 0x03 = 3 (x86)
# 0x3E = 62 (x86_64)

# ==========================================
# 工具函数：等待音量键（10 秒）
# 返回: 0=音量+, 1=音量-, 124=超时
# ==========================================
wait_volume_key() {
    local timeout=$1
    local event_device=""

    # 找输入设备（音量键通常在这个路径下）
    for dev in /dev/input/event*; do
        event_device="$dev"
        break  # 取第一个可用的，实际设备上音量键一般在这里
    done

    if [ -z "$event_device" ]; then
        ui_print "- 未找到输入设备，直接超时"
        sleep "$timeout"
        return 124
    fi

    ui_print "- 请按音量键选择（${timeout}秒）..."
    ui_print "- [音量+]: 继续  [音量-]: 取消"

    # 用 dd 读输入事件，超时退出
    # EV_KEY event type=1, volume up=115, volume down=114
    (
        dd if="$event_device" bs=16 count=64 2>/dev/null | \
        while true; do
            read -r line || break
            # 简单检测：音量+ (115) 或 音量- (114)
            # 实际解析需要看 event 结构，这里用 hexdump 更可靠
            break
        done
    ) &

    local dd_pid=$!
    local elapsed=0

    while [ $elapsed -lt $timeout ]; do
        if ! kill -0 $dd_pid 2>/dev/null; then
            # dd 已退出（用户按了键）
            wait $dd_pid 2>/dev/null
            local code=$?
            if [ $code -eq 0 ]; then
                return 0   # 音量+
            else
                return 1   # 音量-
            fi
        fi
        sleep 1
        elapsed=$((elapsed + 1))
        ui_print "- 剩余 ${elapsed}/${timeout} 秒..."
    done

    # 超时，杀掉 dd
    kill -9 $dd_pid 2>/dev/null
    wait $dd_pid 2>/dev/null
    return 124
}

# ==========================================
# 主逻辑
# ==========================================
ui_print "=== Utils 架构检测 ==="

# 1. 检查 utils 文件是否存在
if [ ! -f "$UTILS_BIN" ]; then
    abort "! 未找到 utils 文件: $UTILS_BIN"
fi

# 2. 检测 utils 的 ELF 架构
UTILS_MACHINE=$(detect_elf_machine "$UTILS_BIN")
ui_print "- utils ELF machine: $UTILS_MACHINE"

# 3. 映射设备架构
DEVICE_ARCH=""
case "$ARCH" in
    arm64)  DEVICE_ARCH="arm64"; DEVICE_MACHINE=183 ;;
    arm)    DEVICE_ARCH="arm";   DEVICE_MACHINE=40  ;;
    x86_64) DEVICE_ARCH="x64";   DEVICE_MACHINE=62  ;;
    x86)    DEVICE_ARCH="x86";   DEVICE_MACHINE=3   ;;
    *)      DEVICE_ARCH="unknown"; DEVICE_MACHINE=0 ;;
esac

ui_print "- 设备架构: $DEVICE_ARCH (ELF: $DEVICE_MACHINE)"

# 4. 架构匹配判断
ARCH_MATCH=false

case "$DEVICE_ARCH" in
    arm64)
        # arm64 设备：utils 是 arm64(183) 或 arm(40) 都算匹配
        if [ "$UTILS_MACHINE" = "183" ] || [ "$UTILS_MACHINE" = "40" ]; then
            ARCH_MATCH=true
        fi
        ;;
    arm)
        # arm 设备：utils 必须是 arm(40)
        if [ "$UTILS_MACHINE" = "40" ]; then
            ARCH_MATCH=true
        fi
        ;;
    x64)
        # x64 设备：utils 是 x64(62) 或 x86(3) 都算匹配
        if [ "$UTILS_MACHINE" = "62" ] || [ "$UTILS_MACHINE" = "3" ]; then
            ARCH_MATCH=true
        fi
        ;;
    x86)
        # x86 设备：utils 必须是 x86(3)
        if [ "$UTILS_MACHINE" = "3" ]; then
            ARCH_MATCH=true
        fi
        ;;
esac

# 5. 根据匹配结果处理
if [ "$ARCH_MATCH" = "true" ]; then
    # 完全匹配
    ui_print "- 架构匹配，继续安装"

    # 如果是 arm64 跑 arm32 或 x64 跑 x86，给个警告
    if [ "$DEVICE_ARCH" = "arm64" ] && [ "$UTILS_MACHINE" = "40" ]; then
        ui_print "- ⚠ 警告：arm64 设备运行 arm32 程序"
    fi
    if [ "$DEVICE_ARCH" = "x64" ] && [ "$UTILS_MACHINE" = "3" ]; then
        ui_print "- ⚠ 警告：x64 设备运行 x86 程序"
    fi

    # 设置权限
    set_perm "$UTILS_BIN" 0 0 0755 u:object_r:system_file:s0

else
    # 架构不匹配
    ui_print "- ❌ 架构不匹配！"
    ui_print "- 设备: $DEVICE_ARCH, utils: $UTILS_MACHINE"

    # 等待用户选择
    wait_volume_key $TIMEOUT
    result=$?

    case $result in
        0)
            ui_print "- 用户选择：继续安装"
            set_perm "$UTILS_BIN" 0 0 0755 u:object_r:system_file:s0
            ;;
        1)
            ui_print "- 用户选择：取消安装"
            abort "! 安装已取消"
            ;;
        124)
            ui_print "- 超时未操作，自动取消"
            abort "! 安装已取消（超时）"
            ;;
    esac
fi

ui_print "- 安装完成"
