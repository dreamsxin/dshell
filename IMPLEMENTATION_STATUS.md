# 实施状态

## 当前切片

从空仓库开始实现附件中的 GPUI 原生路线。当前交付是阶段一的首个可验证切片：桌面连接表单、严格 SSH 主机密钥校验、远端 PTY shell、ANSI 状态机和逐行命令输入。

## 已落实

- UI 使用 GPUI 和原生输入控件；依赖树验证拒绝 WebView、Tauri、Wry 和 Electron。
- SSH I/O 在独立的 Tokio current-thread runtime 线程中运行，界面通过有界 Tokio channel 收发更新。
- russh 的 check_server_key 只接受当前用户 known_hosts 中已存在且匹配的密钥。未知密钥和变更密钥均失败关闭，不自动信任。
- 密码输入默认掩码，点击连接后清空输入框；应用读取出的临时副本由 `Zeroizing` 管理，移交给 russh 的认证字符串由库接管，应用无法保证其内部副本被清零。凭据不写入配置、数据库或日志。
- alacritty_terminal 负责解析远端 ANSI/VT 输出；GPUI 当前呈现解析后的字符网格。
- scripts/verify-hard-constraints.ps1 固化格式、类型检查、测试和依赖边界检查命令。

## 验证记录

2026-10-22，Windows/MSVC：`scripts/verify-hard-constraints.ps1` 全部通过，包括格式检查、锁定依赖下的全目标类型检查、依赖树禁用项检查和单元测试。4 项测试通过，覆盖 ANSI 光标状态及已知/未知/变更 SSH 主机密钥。

Cargo 对传递依赖 `proc-macro-error2 2.0.1` 输出 future-incompatibility 警告；当前不影响检查或测试通过。

## 当前限制

- 输入暂为逐行命令，不是完整按键到终端字节的映射；vim、tmux 等全屏应用还不能正确交互。
- 终端以固定 80x24 网格启动，尚未把窗口 resize 同步到本地网格和远端 PTY。
- 首期只接入密码认证；密钥/Agent 认证、SFTP、隧道、会话存储和录制尚未实现。
- 首次连接不会弹出信任确认；用户需先通过独立可信渠道验证服务器指纹并写入 OpenSSH known_hosts。

## 下一段接力

先运行 .\scripts\verify-hard-constraints.ps1。然后优先实现 GPUI 键盘事件到终端字节的映射、窗口尺寸到 Term 与远端 window_change 的同步，再把字符快照替换为按单元格样式绘制的 TerminalElement。保留严格主机密钥拒绝策略和不落盘凭据约束。
