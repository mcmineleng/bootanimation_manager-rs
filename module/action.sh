#!/system/bin/sh

# 用法: action.sh [秒数]
#   不传参数 = 无限时间，只听音量键
#   传秒数   = 到时间自动停止（按音量键仍可提前停止）

TIMEOUT="${1:-0}"   # 0 = 无限

# 启动 bootanim
setprop service.bootanim.exit 0
setprop ctl.start bootanim

STOPPED=0
stop_bootanim() {
    [ "$STOPPED" -eq 1 ] && return
    STOPPED=1
    setprop ctl.stop bootanim
    setprop service.bootanim.exit 1
}

# UUID 做文件名，防冲突
UUID=$(cat /proc/sys/kernel/random/uuid 2>/dev/null | tr -d '-')
[ -z "$UUID" ] && UUID="$$_$(date +%s)"
LOG="/data/local/tmp/.ge_${UUID}.txt"
: > "$LOG"

# getevent 后台追加写 txt
getevent -l >> "$LOG" &
GE_PID=$!

# 只有传了正数秒数才启用定时停止
if [ "$TIMEOUT" -gt 0 ] 2>/dev/null; then
    echo "[*] ${TIMEOUT} 秒后自动停止，按音量键可提前停止"
    (
        sleep "$TIMEOUT"
        stop_bootanim
        kill "$GE_PID" 2>/dev/null
        # 写哨兵行，唤醒主循环的 read
        echo "__GE_TIMEOUT__" >> "$LOG"
    ) &
else
    echo "[*] 无限时间模式，等待音量键..."
fi

# tail -f 读新增行
tail -f "$LOG" | while read -r line; do
    case "$line" in
        *KEY_VOLUMEUP*DOWN*)
            echo "[+] 检测到音量上键"
            stop_bootanim
            kill "$GE_PID" 2>/dev/null
            break
            ;;
        *KEY_VOLUMEDOWN*DOWN*)
            echo "[-] 检测到音量下键"
            stop_bootanim
            kill "$GE_PID" 2>/dev/null
            break
            ;;
        __GE_TIMEOUT__)
            echo "[*] 时间到，已自动停止"
            break
            ;;
    esac
done

rm -f "$LOG"