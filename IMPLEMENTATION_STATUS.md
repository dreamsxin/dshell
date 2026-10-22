# 实施状态

## 当前切片

从空仓库开始实现附件中的 GPUI 原生路线。当前交付是阶段一的交互终端切片：桌面连接表单、严格 SSH 主机密钥校验、远端 PTY shell、键盘输入映射、窗口尺寸同步，以及按 ANSI 单元格样式绘制的 GPUI 终端网格。

## 已落实

- UI 使用 GPUI 和原生输入控件；依赖树验证拒绝 WebView、Tauri、Wry 和 Electron。
- SSH I/O 在独立的 Tokio current-thread runtime 线程中运行，界面通过有界 Tokio channel 收发更新。
- russh 的 check_server_key 只接受当前用户 known_hosts 中已存在且匹配的密钥。未知密钥和变更密钥均失败关闭，不自动信任。
- 密码输入默认掩码，点击连接后清空输入框；应用读取出的临时副本由 `Zeroizing` 管理，移交给 russh 的认证字符串由库接管，应用无法保证其内部副本被清零。凭据不写入配置、数据库或日志。
- alacritty_terminal 负责解析远端 ANSI/VT 输出；GPUI 当前呈现解析后的字符网格。
- GPUI 键盘事件映射到文本、控制字符、方向键、导航键和 F1-F12 序列；方向键遵循远端 application-cursor 模式，支持常见修饰键组合。
- 窗口尺寸按终端区域和单元格尺寸换算为列/行；连接时用于创建 PTY，运行中同时调整本地 Term 和远端 PTY。
- ANSI 帧按颜色、粗体、斜体和下划线分组为单元格运行段，以 GPUI 原生元素绘制；SSH 输出以 16 ms 节拍合并后刷新。
- scripts/verify-hard-constraints.ps1 固化格式、类型检查、测试和依赖边界检查命令。

## 验证记录

2026-10-22，Windows/MSVC：`./scripts/verify-hard-constraints.ps1` 全部通过，包含格式检查、锁定依赖下的全目标类型检查、禁用依赖扫描及 6 项测试。测试覆盖键盘序列、终端尺寸、ANSI 样式/模式及已知/未知/变更 SSH 主机密钥。

Cargo 对传递依赖 `proc-macro-error2 2.0.1` 输出 future-incompatibility 警告；当前不影响检查或测试通过。

## 当前限制

- 键盘映射覆盖常见按键和修饰键，但尚未实现完整 IME 组合输入、终端鼠标协议、剪贴板、选择/滚动缓冲区及终端图像协议。
- 当前通过 GPUI flex 元素绘制样式化网格，尚未实现自定义 `TerminalElement`、字体 shaping、精确字素宽度/重排或渲染性能优化。
- 首期只接入密码认证；密钥/Agent 认证、SFTP、隧道、会话存储和录制尚未实现。
- 首次连接不会弹出信任确认；用户需先通过独立可信渠道验证服务器指纹并写入 OpenSSH known_hosts。
- 当前只有单会话视图；断开和应用关闭时的会话生命周期体验仍待完善。

## 下一段接力

下一步优先补齐交互细节：测试常见全屏终端程序，改善字素宽度/重排与滚动缓冲显示，再实现鼠标、选择和剪贴板交互。之后再进入阶段二的会话管理与 SFTP。保留严格主机密钥拒绝策略和不落盘凭据约束。
