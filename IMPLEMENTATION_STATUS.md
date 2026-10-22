# 实施状态

## 当前切片

从空仓库开始实现附件中的 GPUI 原生路线。当前交付是阶段一的交互终端切片：桌面连接表单、严格 SSH 主机密钥校验、远端 PTY shell、键盘输入映射、窗口尺寸同步、ANSI 样式网格、滚动缓冲区浏览、鼠标文本选择和系统剪贴板复制/粘贴。

## 已落实

- UI 使用 GPUI 和原生输入控件；依赖树验证拒绝 WebView、Tauri、Wry 和 Electron。
- SSH I/O 在独立的 Tokio current-thread runtime 线程中运行，界面通过有界 Tokio channel 收发更新。
- russh 的 check_server_key 只接受当前用户 known_hosts 中已存在且匹配的密钥。未知密钥和变更密钥均失败关闭，不自动信任。
- 密码输入默认掩码，点击连接后清空输入框；应用读取出的临时副本由 `Zeroizing` 管理，移交给 russh 的认证字符串由库接管，应用无法保证其内部副本被清零。凭据不写入配置、数据库或日志。
- alacritty_terminal 负责解析远端 ANSI/VT 输出；GPUI 当前呈现解析后的字符网格。
- GPUI 键盘事件映射到文本、控制字符、方向键、导航键和 F1-F12 序列；方向键遵循远端 application-cursor 模式，支持常见修饰键组合。
- 窗口尺寸按终端区域和单元格尺寸换算为列/行；连接时用于创建 PTY，运行中同时调整本地 Term 和远端 PTY。
- ANSI 帧按颜色、粗体、斜体和下划线分组为单元格运行段，以 GPUI 原生元素绘制；SSH 输出以 16 ms 节拍合并后刷新。
- 鼠标滚轮可浏览 alacritty_terminal 保留的历史行；向上滚动显示较早内容，向下滚动回到实时屏幕，并保留不足一行的像素滚动余量。
- alacritty_terminal 的鼠标跟踪模式驱动远端鼠标输入：支持点击、按键拖动和全运动报告，以及 legacy、UTF-8、SGR 编码；报告模式下鼠标按下/释放/移动与滚轮转发给 SSH PTY，无报告模式保留本地左键选择和历史滚动。
- 终端网格适配保留中文/emoji 双宽单元格和组合标记；Unicode 输入按 UTF-8 原样写入远端，终端缩放后的行列数由自动化测试覆盖。
- 会话状态明确区分连接中、已连接、断开中和已断开；连接期间可取消握手，已连接会话可主动断开，远端退出/连接失败/事件流关闭会清空旧终端帧。应用释放取消发送端时，SSH 任务会在握手或会话循环中收尾。
- 鼠标左键拖动可选择终端网格文本，选中单元格有独立高亮；`Ctrl+Shift+C` 复制非空选择，`Ctrl+Shift+V` 从系统剪贴板粘贴，并遵循远端 bracketed-paste 模式。
- scripts/verify-hard-constraints.ps1 固化格式、类型检查、测试和依赖边界检查命令。
- scripts/test-docker-ssh.ps1 创建临时 Ubuntu 24.04 OpenSSH 服务，以容器实际 Ed25519 公钥和隔离的临时 known_hosts 执行应用 SSH/PTY 集成测试，测试后清理容器和临时密钥文件。

## 验证记录

2026-10-22，Windows/MSVC：`./scripts/verify-hard-constraints.ps1` 全部通过，包含格式检查、锁定依赖下的全目标类型检查、禁用依赖扫描及 17 项通过的单元测试（另有 2 项 Docker 专用测试由独立脚本运行）。测试覆盖握手取消/取消发送端释放、键盘序列与 Unicode UTF-8 输入、双宽字符/组合标记网格宽度、终端 resize 行列、ANSI 样式/模式、滚动历史、文本选择/bracketed paste、鼠标跟踪模式与 legacy/UTF-8/SGR 编码，以及已知/未知/变更 SSH 主机密钥。

`./scripts/test-docker-ssh.ps1` 已通过：连接临时 Ubuntu 24.04 SSH 服务，验证 host-key 检查、密码认证、PTY resize（远端 `stty size` 返回 `30 100`）、终端输出与选择、粘贴输入、鼠标报告字节到达远端 PTY、远端 `exit` 和用户主动断开；测试凭据使用随机值，未写入仓库，且测试不读取或更改个人 known_hosts。Ubuntu 首次安装 OpenSSH 可能受包源速度影响，脚本等待窗口已延长。

Cargo 对传递依赖 `proc-macro-error2 2.0.1` 输出 future-incompatibility 警告；当前不影响检查或测试通过。

2026-10-22，Windows 桌面烟测尝试：临时 Ubuntu 24.04 SSH 容器和 dshell 窗口均成功启动，SSH 主机密钥保存在隔离的临时 `USERPROFILE/.ssh/known_hosts` 中，个人 known_hosts 未修改。当前桌面自动化环境禁用了原生窗口控制，无法实际操作窗口；临时容器仅安装了 `vim-tiny`，tmux/htop 尚未验证。容器和应用进程已清理，vim、tmux、htop、Unicode 字形像素对齐及 IME/焦点交互仍待 Windows 桌面人工烟测。

## 当前限制

- 键盘映射覆盖常见按键和修饰键，但尚未验证完整 IME 组合输入；文本选择仍按字符范围处理，终端图像协议未实现。
- 当前通过 GPUI flex 元素绘制样式化网格，尚未实现自定义 `TerminalElement`、字体 shaping、精确字素宽度/重排或渲染性能优化。
- 首期只接入密码认证；密钥/Agent 认证、SFTP、隧道、会话存储和录制尚未实现。
- 首次连接不会弹出信任确认；用户需先通过独立可信渠道验证服务器指纹并写入 OpenSSH known_hosts。
- 当前只有单会话视图；Windows 桌面中的真实 IME/焦点、全屏程序和 Unicode 字形像素对齐尚未人工烟测。

## 下一段接力

完整路线、阶段依赖和验收条件见 [ROADMAP.md](ROADMAP.md)。下一步在 Windows 桌面烟测 vim、tmux、htop，全屏下的实际 Unicode 字形布局，以及 IME/焦点交互；据此校准字体宽度与重排。阶段 1 桌面验收后进入阶段 2 的主机资料和多会话工作区。继续遵守严格主机密钥拒绝策略和不落盘凭据约束。
