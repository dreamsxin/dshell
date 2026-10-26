# 实施状态

## 当前切片

2026-10-27，Windows/MSVC：可共享的 Quick commands 支持按目标会话展开连接变量，版本提升至 `v0.1.40`。编辑器可在光标处插入 `{{host}}`、`{{port}}`、`{{user}}`；运行或广播时逐个目标展开，并按 POSIX shell 单引号规则引用值。未知/不完整占位符原样保留，口令等秘密不可用作变量；JSON 命令包格式不变。新增引用、未知 token、非递归替换测试。依据 G2 Xshell 用户评价对多会话、同步命令、复制会话和团队共享短命令的正面反馈，补齐跨目标可复用的命令模板能力。`cargo fmt --all` 与 `scripts\verify-hard-constraints.ps1` 通过（187 项通过、9 项 Docker 专项忽略）；`scripts\test-docker-ssh.ps1` 通过（9/9）。未在用户活动桌面启动 GUI；隔离截图、DPI/IME/字形与命令编辑器交互仍待人工验收。下一步：实现 Quick commands 分组，继续按用户评价筛选高频工作流。

## 当前切片

2026-10-27，Windows/MSVC：终端标签新增运行时输入锁，版本提升至 `v0.1.39`。标签右键菜单可锁定/解锁，锁定状态显示图标和悬停说明；锁定只拦截按键、终端鼠标、剪贴板粘贴及秘密文本发送，终端输出、窗口尺寸和服务器控制请求仍可用。活动标签锁定时同步输入整体停止发送；没有会话接收 Compose 草稿时保留草稿。锁状态仅驻留当前运行会话，关闭标签即清除。新增输入类别、控制请求和同步目标测试。`cargo fmt --all` 与 `scripts\verify-hard-constraints.ps1` 通过（184 项通过、9 项 Docker 专项忽略）；`scripts\test-docker-ssh.ps1` 通过（9/9）。同一 SSH 目标最近本地与远端 SFTP 目录记忆已于 `v0.1.33` 交付，按主机、端口和用户名恢复。未在用户活动桌面启动 GUI；隔离截图、DPI/IME/字形及标签交互仍待人工验收。下一步：继续评估 SFTP 文件面板布局与传输交互。

## 当前切片

2026-10-27，Windows/MSVC：已保存会话支持自定义标签标题和主题适配颜色，版本提升至 `v0.1.38`。标题留空时回退到会话名称；重复会话继续保留唯一后缀，新建副本使用当前标签标题作为名称。主机资料格式升至版本 7，导出格式升至版本 3，旧资料默认使用会话名和主题强调色。新增标题长度/控制字符校验、迁移和中英文标签测试。`cargo fmt --all` 与 `scripts\verify-hard-constraints.ps1` 通过（182 项通过、9 项 Docker 专项忽略，包含全目标类型检查和硬约束检查）；Docker SSH 未运行，本切片没有改动 SSH 传输、认证、SFTP、转发、日志或 PTY 行为。最近本地与远端 SFTP 目录记忆属于 `v0.1.33` 已交付能力，按主机、端口和用户名恢复。未在用户活动桌面启动 GUI；隔离 Windows 截图、DPI/IME/字形及标签交互手工验收仍待执行。下一步为隔离桌面验收。

## 当前切片

2026-10-27，Windows/MSVC：分组会话批量启动完成，版本提升至 `v0.1.37`。分组标题可启动自身及嵌套组中的已保存会话，也支持未分组；已连接和重复目标会跳过。连接前逐个收集缺失凭据，随后串行启动；认证失败时可重试、跳过当前主机或停止剩余队列。暂存凭据使用零化内存，只在认证成功后进入进程内缓存。新增候选筛选、队列结果、停止及认证提示期间跳过状态测试，并补充中英文文案。`cargo fmt --all`、`scripts\verify-hard-constraints.ps1` 通过（178 项通过、9 项 Docker 专项忽略）；`scripts\test-docker-ssh.ps1` 通过（9/9）。SFTP 最近本地与远端目录记忆继续按主机、端口和用户名恢复，属于 `v0.1.33` 已交付能力。本轮未启动用户活动桌面的 GUI；分组启动手工交互、隔离 Windows 截图、DPI/IME/字形检查仍待人工验收。下一步为隔离桌面验收。

## 当前切片

2026-10-27，Windows/MSVC：嵌套会话分组已实现，最多 8 层；支持创建子组、拖拽主机归组、折叠层级和同级排序。重命名/删除作用于整个子树；若目标路径会与现有分组树合并则拒绝，超出路径长度或深度也会提示。旧版扁平分组和 UI 设置继续兼容，新增路径、冲突、子树重命名/删除、资料校验及设置往返测试。`cargo fmt --all`、`cargo check --locked --all-targets`、`cargo test --locked` 与 `scripts\verify-hard-constraints.ps1` 均通过（175 项通过、9 项 Docker 专项忽略）。此功能不触及 SSH，Docker SSH 验证不适用。隔离 Windows GUI 截图、DPI/IME/字形验收仍待人工执行；未在用户活动桌面启动 GUI。下一步为隔离桌面验收。

## 当前切片

2026-10-26，Windows/MSVC：提示响应式终端触发器完成，版本提升至 `v0.1.36`。版本化 `triggers.json` 保存引擎开关及非预设文本规则；预设文本触发器只在当前运行保留，永不写入配置。新增“询问并发送”动作：关键词匹配后显示掩码响应框，确认后通过零化文本/字节缓冲发送到对应 SSH 会话并回车；取消不会断开会话。认证和触发器提示共用排队弹窗，避免覆盖，断开/关闭会话清除对应排队项。增加响应标签校验、存储排除秘密文本和简体中文文案。`cargo fmt --all`、`cargo check --locked --all-targets`、`cargo test --locked` 与 `scripts\verify-hard-constraints.ps1` 均通过（168 项通过、9 项 Docker 专项忽略）；`scripts\test-docker-ssh.ps1` 通过（9/9）。隔离 Windows 掩码输入、触发/取消交互、DPI/IME/字形与截图仍待人工验收；未在用户活动桌面启动 GUI。SFTP 最近目录已按主机、端口、用户名记忆并在重连恢复，包含本地文件夹复用；该能力已随 `v0.1.33` 发布。下一步为隔离 Windows GUI 手工验收。

## 当前切片

2026-10-26，Windows/MSVC：SFTP 资源管理器拖放上传已随 `v0.1.35` 发布。Windows 资源管理器拖入文件或文件夹即可进入当前会话已有 FIFO 上传队列；文件选择器和文件夹选择器也共用 `symlink_metadata` 校验，只接受普通文件/目录并拒绝符号链接和特殊路径。面板提供英文/简体中文拖放提示和错误反馈。最近目录行为继续按主机、端口和用户记住远端目录与本地文件夹，拖放沿用当前远端目录。新增常规文件、目录和缺失路径分类测试，符号链接拒绝测试仅在 Unix 平台编译。`cargo fmt --all`、`scripts\verify-hard-constraints.ps1` 通过（166 项通过、9 项 Docker 专项忽略），`scripts\test-docker-ssh.ps1` 通过（9/9）。隔离 Windows GUI 拖放实测与截图仍待人工验收；未在用户活动桌面启动 GUI。下一步为隔离桌面验收或继续评估 SFTP 面板布局。

## 当前切片

2026-10-26，Windows/MSVC：SSH 意外断线自动重连已完成，准备发布 `v0.1.34`。主机资料可单独启用重连（默认关闭），间隔为 1–300 秒、最多 20 次；只重试首次成功连接后的传输错误，每次重试仍严格校验既有 `known_hosts`，认证和主机密钥失败立即停止。断线期间拒绝终端、SFTP 与转发请求并丢弃已排队操作；重连后建立新 shell、恢复已保存转发并刷新记住的 SFTP 目录。正常 shell `exit` 不触发重连；旧 shell、远端进程、活动传输和日志不会恢复。旧主机资料迁移后默认关闭，新增重连边界、请求门禁、SFTP 恢复与主机密钥失败测试及中文文案。`cargo fmt --all`、硬约束验证和离线单测通过（165 项通过、9 项 Docker 专项忽略），`scripts\test-docker-ssh.ps1` 9/9 通过。隔离 Windows GUI 截图及实际网络断线/恢复手工烟测仍待验收；本轮未在用户活动桌面启动 GUI。

## 当前切片

2026-10-26，Windows/MSVC：SFTP 最近目录记忆已实现并准备发布 `v0.1.33`。最近连接记录按主机、端口和用户记住本地文件夹及远端目录；新连接恢复最后远端目录，下载复用本地文件夹，SFTP 面板的文件夹图标可更改本地目录。远端记忆路径失败时回退服务器 home。最近连接 JSON 从版本 4 自动迁移至 5，旧文档路径默认为空；主机资料分享包不含本机最近目录。新增迁移、读写往返和路径边界测试及中文标签。`cargo fmt --all` 与 `cargo test --locked --offline`（158 项通过、9 项 Docker 专项忽略）通过；`scripts\verify-hard-constraints.ps1` 通过（158 项通过、9 项忽略），`scripts\test-docker-ssh.ps1` 的 SSH/SFTP 集成测试 9/9 通过。隔离 Windows GUI 截图、DPI/IME/字形检查和真实远端手工验证仍待人工验收；本轮未在用户活动桌面启动 GUI。

## 当前切片

2026-10-26，Windows/MSVC：Quick Commands 团队分享命令包完成，准备发布 `v0.1.32`。根据 Xshell 用户评价中对快捷命令选项的认可和团队共享短命令的需求，快捷命令面板新增版本化 JSON 命令包导入/导出；导入按名称忽略大小写合并、跳过重名项，超出 200 条时整批拒绝，文件上限 1 MiB，并校验版本、UTF-8 和记录字段。导入只写入本机快捷命令，不会自动执行。新增往返、缺省发送行为、格式/大小拒绝、去重和数量上限单测及中文文案。`cargo fmt --all`、硬约束验证（155 项通过、9 项 Docker 专项跳过）、`cargo test --locked`（155 项通过）和 `cargo build --locked` 均通过；Docker SSH 未运行，因为本切片没有触及 SSH 或其相关路径。隔离 Windows GUI 截图、DPI/IME/字形检查和团队双机手工交换仍待人工验收；本轮未在用户活动桌面启动 GUI。

## 当前切片

2026-10-26，Windows/MSVC：会话复制与凭据缓存修正完成，准备发布 `v0.1.31`。已保存主机的标签菜单新增 Duplicate session，每次复制建立独立 SSH 连接并分配唯一标签名；无已保存资料的临时连接不启用此操作。密码和私钥口令只在本次进程的 Zeroizing 缓存中复用，不写入主机资料；旧版资料升级时忽略并清理遗留凭据字段。新增中文文案和缓存/标签名测试。按用户选定的进程列表与终止能力已存在于更早版本：检查器可刷新最多 50 条远端进程并在确认后发送 SIGTERM，成功后刷新列表。`cargo fmt --all`、硬约束验证（152 项通过、9 项 Docker 专项跳过）和 Docker SSH 集成（9/9）均已通过。隔离 Windows GUI 截图、DPI/IME/字形检查和真实远端手工操作仍待人工验收；本轮未在用户活动桌面启动 GUI。

## 当前切片

2026-10-25，Windows/MSVC：服务器登录会话巡检完成并准备发布 `v0.1.30`。检查器新增 Logged-in sessions 分组和刷新入口，通过独立 SSH exec channel 使用 `who` 读取最多 32 个登录用户、终端、登录时间和来源；响应限制为 64 KiB、3 秒超时，空结果、失败和加载状态保留在当前会话，不写入终端、日志或主机资料。新增登录会话协议解析、缺少标记、非法行和数量边界单测；`cargo fmt --all`、硬约束验证（150 项通过、9 项 Docker 专项跳过）和 Docker SSH（9/9）均已通过。Logged-in sessions 分组的隔离 Windows 界面截图和真实远端会话人工烟测仍需用户手工复核。

## 当前切片

2026-10-25，Windows/MSVC：服务器监听端口巡检完成并准备发布 `v0.1.29`。检查器新增 Listening ports 分组和刷新入口，优先通过 `ss -H -lntup`，不可用时回退 `netstat -lntup`，读取最多 64 个 TCP/UDP 监听项的协议、本地地址和进程信息；响应限制为 64 KiB、3 秒超时，空结果、失败和加载状态保留在当前会话，不写入终端、日志或主机资料。新增监听端口协议解析、缺少标记、非法行和数量边界单测；`cargo fmt --all`、硬约束验证（148 项通过、9 项 Docker 专项跳过）和 Docker SSH（9/9）均已通过。Listening ports 分组的隔离 Windows 界面截图和真实远端监听端口人工烟测仍需用户手工复核。

## 当前切片

2026-10-25，Windows/MSVC：服务器存储巡检完成并准备发布 `v0.1.28`。检查器新增 Storage 分组和刷新入口，通过独立 SSH exec channel 使用 POSIX `df -P -k` 读取最多 32 个挂载点的文件系统、总量、已用、可用和使用率；响应限制为 64 KiB、3 秒超时，按 75%/90% 显示警告/危险色。空结果、失败和加载状态保留在当前会话，不写入终端、日志或主机资料。新增存储协议解析、百分比/容量边界、超量拒绝和中文/格式化单测；`cargo fmt --all`、硬约束验证（146 项通过、9 项 Docker 专项跳过）和 Docker SSH（9/9）均已通过。Storage 分组的隔离 Windows 界面截图和真实远端挂载点人工烟测仍需用户手工复核。

## 当前切片

2026-10-25，Windows/MSVC：服务器网络接口巡检完成并准备发布 `v0.1.27`。检查器新增 Network 分组和刷新入口，通过独立 SSH exec channel 读取最多 32 个接口的名称、运行状态和地址；优先使用 `ip -o -brief addr show`，不可用时回退 `/sys/class/net`。响应限制为 64 KiB、3 秒超时，空结果、失败和加载状态保留在当前会话，不写入终端、日志或主机资料。新增网络协议解析、缺少标记、非法行和边界单测；`cargo fmt --all`、硬约束验证（常规测试 143 项通过、9 项 Docker 专项跳过）和 Docker SSH（9/9）均已通过。Network 分组的隔离 Windows 界面截图和真实远端接口人工烟测仍需用户手工复核。

## 当前切片

2026-10-25，Windows/MSVC：服务器告警历史完成并准备发布 `v0.1.26`。Server health 检查器在指标或进程刷新后记录新出现或数值变化的 CPU、内存和根分区告警，最多保留 32 条，显示观察时间、当前值和阈值；新增清除图标，历史只保存在当前会话，连接失败或断开时随会话状态清空，不写入配置、日志或远端。新增阈值观察与时间格式单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（141 项通过、9 项 Docker 专项跳过）已完成。硬约束、Docker SSH、Windows 中文界面截图和告警历史人工烟测待本切片收尾。

## 当前切片

2026-10-25，Windows/MSVC：服务日志查看完成并准备发布 `v0.1.25`。Services 分组每行新增查看日志图标，远端优先读取 `journalctl -u` 最近 80 行，缺少 systemd 时回退 `service status`；结果只保留在当前会话检查器，不写入终端、会话日志或主机资料。服务日志读取通过独立 SSH exec channel，限制 3 秒和 64 KiB，服务名沿用安全字符校验，失败、超时和不完整响应会显示明确反馈。指标协议标记改为更稳健的 POSIX `printf` 输出；新增日志协议解析、状态失败和边界单测。`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（139 项通过、9 项 Docker 专项跳过）、硬约束检查和 Docker SSH（9/9）均已通过；Services 日志入口的隔离 Windows 中文界面截图和远程日志人工烟测仍待用户桌面复核。

## 当前切片

2026-10-25，Windows/MSVC：服务器电源控制补齐并发布 `v0.1.24`。Server health 分组新增确认后的 Reboot server 和 Shut down server 图标入口，动作通过独立 SSH exec channel 发送固定命令，限制 3 秒和 64 KiB，并将成功、失败和超时反馈保留在检查器中；非 systemd 主机回退到后台 reboot/shutdown 调用，避免把重启动作写入终端。新增电源动作协议解析、状态码和简体中文文案单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（137 项通过、9 项 Docker 专项跳过）、硬约束检查和 Docker SSH（9/9）均已通过。Windows 手工验证确认中文电源确认弹窗，测试后选择取消，未触发远端重启或关机。

## 当前切片

2026-10-25，Windows/MSVC：服务器进程管理补齐并发布 `v0.1.23`。Processes 分组每行新增 Terminate 图标和确认弹窗，远端只接受大于 1 的数字 PID，通过独立 SSH exec channel 发送 `SIGTERM`，成功后自动刷新进程列表，失败、拒绝和超时会保留在检查器消息中。新增进程动作协议解析、PID 边界和结果处理单测；当前单元测试为 136 项通过、9 项 Docker 专项通过。已完成 `cargo fmt --all`、`cargo build --locked --offline`、`cargo check --locked --all-targets --offline`、硬约束检查和 Docker SSH 验证；Windows 手工界面验证确认中文检查器、9 条远端进程记录、每行终止图标，以及保存口令后的侧栏直接连接流程。

## 当前切片

2026-10-25，Windows/MSVC：认证凭据持久化与服务控制完成并准备发布 `v0.1.22`。已保存主机资料现在保存密码、私钥口令和跳板机凭据；从侧栏连接会优先使用已保存值，首次认证成功后自动回写资料，错误凭据会清除运行期缓存并重新弹出认证窗口，取消会结束失败会话。主机资料导出仍会移除所有凭据。服务器检查器 Services 分组新增 Start、Stop、Restart 图标入口，操作前有确认弹窗，服务名只允许安全字符，远端优先使用 systemd，缺少 systemd 时回退 service，成功后自动刷新列表。新增配置往返、服务动作解析和安全校验单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（135 项通过、9 项 Docker 专项跳过）已完成。硬约束、Docker SSH 和本轮中文界面/认证保存路径仍需收尾验证；当前本地手工 SSH 服务为 `127.0.0.1:2222`，用户 `dshelltest`。

## 当前切片

2026-10-25，Windows/MSVC：服务器管理常用功能继续完成并准备发布 `v0.1.21`。检查器新增 Services 分组，优先解析 systemd，缺少 systemd 时回退 `service --status-all`；Settings 新增并持久化 CPU、内存和根分区告警百分比，健康检查会对超过阈值的指标显示警告。服务查询同样使用独立 SSH exec channel，限制 3 秒和 64 KiB，不写入终端、日志或配置。新增服务协议解析、阈值校验、旧 settings.json 默认兼容和中文文案测试；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（134 项通过、9 项 Docker 专项跳过）、硬约束检查和 Docker SSH（9/9）均已通过。服务命令已在 `127.0.0.1:2222` 手工容器中验证；DPI/IME、字形对齐和本轮 Services/告警界面截图仍需隔离 Windows 桌面人工检查。

2026-10-25，Windows/MSVC：服务器检查器进程列表完成并准备发布 `v0.1.20`。新增 Processes 折叠分组和刷新按钮，已连接会话通过独立 SSH exec channel 获取最多 50 条远端进程记录，显示 PID、用户、CPU、内存、状态和命令；响应限制为 64 KiB、3 秒超时，失败、空结果和加载状态均保留在当前会话内存，不混入终端、日志或配置。新增进程协议解析、缺少标记、非法行和边界单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline` 和 `cargo test --locked --offline`（131 项通过、9 项 Docker 专项跳过）已完成。硬约束、Docker SSH 和中文界面截图仍在本切片收尾验证。

2026-10-25，Windows/MSVC：保存主机认证交互修正并准备发布 `v0.1.19`。侧栏主机行点击和连接图标统一进入自动连接流程：本次运行已有凭据时直接复用，没有凭据时只弹出密码、私钥口令或跳板凭据窗口，编辑仍由设置图标进入属性。认证失败会清除错误缓存并重新弹窗，取消会删除失败会话，不再留下“未连接”条目或回到属性窗口。凭据仍只保存在 `Zeroizing` 内存中，不写入主机资料、设置或日志。`cargo fmt --all`、`cargo check --locked --all-targets --offline` 和 `cargo test --locked --offline`（129 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）已完成。新构建中文认证弹窗截图为 `artifacts/dshell-v0.1.19-auth-dialog-cn.png`；密码输入、错误重试和成功后缓存复用仍需用户手工操作，DPI/IME/字形像素对齐继续按策略人工检查。

2026-10-25，Windows/MSVC：服务器健康检查完成并准备发布 `v0.1.18`。检查器新增 Server health 分组和刷新按钮，已连接会话通过独立 SSH exec channel 按需读取操作系统、内核、1/5/15 分钟负载、内存总量/可用量、根分区容量/使用率和运行时间；结果只保留在当前会话内存，不混入交互终端、日志或主机资料。命令输出限制为 64 KiB，3 秒超时，缺少兼容工具或非零退出会显示错误并保持终端可用。新增指标解析、边界和错误路径单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（129 项通过、9 项 Docker 专项跳过）已完成。硬约束、Docker SSH 和 Windows 中文界面截图仍在本切片收尾验证。

2026-10-25，Windows/MSVC：保存主机认证流程完成并准备发布 `v0.1.17`。双击或连接图标打开保存主机时，若本次运行没有可复用凭据，只显示认证弹窗；密码、键盘交互密码、私钥口令和跳板机凭据都可在同一弹窗中确认。成功连接后凭据仅以 `Zeroizing` 形式保存在本次进程内存中，下一次连接自动复用；认证失败会清除旧缓存并重新弹出输入，取消会清空输入并结束连接流程。左侧已连接主机继续直接切换会话，不打开属性窗口。新增缓存归一化、空凭据和中文认证文案单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（126 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。新鲜构建中文工作区截图为 `artifacts/dshell-v0.1.17-chinese-workspace.png`；认证输入、DPI、IME、字形像素对齐和连接后的 SFTP/隧道交互仍需手工烟测。

2026-10-25，Windows/MSVC：简体中文覆盖补齐并准备发布 `v0.1.16`。隧道与 SOCKS5、关键词高亮、快捷命令、组合输入、键盘交互认证、同步输入、标签菜单、检查器和连接校验反馈全部接入现有翻译表；用户提供的主机名、命令、远端路径和终端输出保持原样。新增近期面板和认证校验的本地化单测；`cargo fmt --all`、`cargo check --locked --all-targets --offline`、`cargo test --locked --offline`（121 项通过、9 项 Docker 专项跳过）和 `scripts\verify-hard-constraints.ps1` 已完成。新鲜 Windows 构建的中文工作区截图为 `artifacts/dshell-v0.1.16-chinese-fresh.png`。手工 SSH 服务继续绑定 `127.0.0.1:2222`，真实 DPI、IME、字形像素对齐和连接后的 SFTP/隧道交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：密钥管理补齐完成并准备发布 `v0.1.15`。Keys 面板支持导入 OpenSSH 私钥到 `~/.ssh`、导出经校验的私钥和匹配公钥、修改口令、复制公钥，以及带显式确认的密钥对删除；目标文件不会被覆盖，缺失 `.pub` 时从私钥派生公钥，非普通文件不会被删除。新增密钥传输、删除、格式校验和不覆盖失败路径单测；`cargo fmt --all`、`cargo test --locked --offline`（121 项通过、9 项 Docker 专项跳过）、`cargo check --locked --all-targets --offline`、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。新构建已完成，但真实 Windows Keys 面板的图标点击、文件选择器和删除确认因本轮 Computer Use 被物理 `Esc` 中止，仍待手工验证。

2026-10-25，Windows/MSVC：主机资料备份与迁移完成并准备发布 `v0.1.14`。Session 图标菜单新增 dshell 主机资料 JSON 的导入/导出；导出只包含版本化主机资料，不包含最近连接、密码、口令或私钥材料；导入校验版本和全部资料约束，遇到重复名称时跳过且不覆盖已有资料。新增导入/导出往返、未知版本、重复名称、合并去重和无秘密字段单测；`cargo fmt --all`、`cargo test --locked --offline`（119 项通过、9 项 Docker 专项跳过）和 `scripts\verify-hard-constraints.ps1` 均已完成。真实 Windows 烟测已完成：中文 More 菜单显示导入/导出入口，原生文件选择器成功导出并重新选择 `artifacts/manual-export.json`，界面和 Toast 均确认导入 0 个、跳过 1 个重复项；证据截图为 `artifacts/dshell-v0.1.14-profile-menu-cn.jpg`。

2026-10-25，Windows/MSVC：主机分组管理完成并准备发布 `v0.1.13`。侧栏可创建空分组，分组标题提供重命名和删除图标；重命名会同步主机资料、分组显示名、顺序和折叠状态，删除前明确确认并将主机移至 `UNGROUPED`。分组显示名、顺序和折叠状态写入版本化 UI 设置，旧版配置缺少字段时继续兼容。新增分组重命名/删除纯逻辑单测；`cargo fmt --all`、`cargo test --locked --offline`（116 项通过、9 项 Docker 专项跳过）和 `scripts\verify-hard-constraints.ps1` 均已完成。真实 Windows DPI、中文 IME、图标点击和删除确认仍需用户手工烟测。

2026-10-25，Windows/MSVC：简体中文界面补齐主机载入、最近连接、收藏、分组移动、主机保存/删除、认证模板反馈、会话删除确认和主菜单版本栏；用户提供的主机名与模板名保持原文，版本栏改为读取 `CARGO_PKG_VERSION`。准备发布 `v0.1.12`。`cargo fmt --all`、`cargo test --locked --offline`（114 项通过、9 项 Docker 专项跳过）和 `scripts\verify-hard-constraints.ps1` 均已完成；本轮仅修改 UI 本地化与版本显示，上一版本的 `scripts\test-docker-ssh.ps1` 9 项集成测试结果保持通过。真实 Windows DPI、中文 IME 和字形像素对齐仍需用户手工烟测。

2026-10-25，Windows/MSVC：主机侧栏分组重排完成并准备发布 `v0.1.11`。分组标题支持拖拽到另一个分组标题前方，顺序按归一化分组键写入版本化 UI 设置；旧版设置文件继续兼容，未保存的新分组自动追加。新增分组排序与设置往返单测；`cargo fmt --all`、`cargo test --locked --offline`（114 项通过、9 项 Docker 专项跳过）和 `scripts\verify-hard-constraints.ps1` 均已完成，`scripts\test-docker-ssh.ps1` 9 项集成测试全部通过。新鲜构建的中文工作区、连接属性和认证页截图分别保存在 `artifacts/dshell-v0.1.11-chinese-workspace.png`、`artifacts/dshell-v0.1.11-local-ssh-form.png` 和 `artifacts/dshell-v0.1.11-auth-cn.png`；真实 Windows DPI、中文 IME 和字形像素对齐仍需用户手工烟测。

2026-10-25，Windows/MSVC：简体中文覆盖补漏完成并准备发布 `v0.1.10`。撰写、日志回放、分屏、断开连接入口、认证说明和剩余输入占位符接入翻译表；切换语言后主机名称、SFTP 属性和键盘交互响应占位符即时刷新。新增本地化键单测；`cargo fmt --all`、`scripts\verify-hard-constraints.ps1`（113 项通过、9 项 Docker 专项跳过）和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。手工 SSH 服务继续绑定 `127.0.0.1:2222`，真实 Windows DPI、中文 IME 和字形像素对齐仍需用户手工烟测。

2026-10-25，Windows/MSVC：主机侧栏分组折叠完成并准备发布 `v0.1.9`。分组标题使用统一的折叠图标，点击后隐藏或展开该组主机；折叠键按大小写不敏感的分组名称保存到版本化 UI 设置，旧版设置文件缺少字段时默认全部展开。新增设置归一化和分组键单测；`cargo fmt --all`、离线 `cargo check --locked --all-targets`、`cargo test --locked --offline`（113 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。真实 Windows DPI、中文 IME 和侧栏实际交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：SFTP 远端文件系统容量信息完成并准备发布 `v0.1.8`。SFTP worker 通过 OpenSSH `statvfs@openssh.com` 读取当前路径所在文件系统的总容量、可用空间、空闲空间、块大小和最长文件名；面板自动显示已用百分比并提供图标刷新入口，服务器不支持扩展时显示明确的中文提示。新增容量百分比边界单测；Docker SFTP 集成覆盖真实文件系统信息读取。`cargo fmt --all`、离线 `cargo check --locked`、`cargo test --locked --offline`（112 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。真实 Windows DPI、中文 IME 和 SFTP 面板实际交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：SFTP 远端权限与时间戳管理完成并准备发布 `v0.1.7`。目录条目保留权限、访问时间和修改时间；属性图标入口支持八进制权限、Unix 访问时间和修改时间编辑，提交通过 SFTP `SETSTAT` 并保留文件类型位。非法权限或时间戳输入不会发送请求；编辑器和校验提示接入简体中文。新增权限格式化/解析和时间戳边界单测；Docker SFTP 集成覆盖元数据读取、修改及不存在目标失败路径。`cargo fmt --all`、离线 `cargo check --locked`、`cargo test --locked --offline`（111 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。真实 Windows DPI、中文 IME 和属性编辑器实际交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：已有 OpenSSH 私钥修改口令完成并准备发布 `v0.1.6`。Keys 面板为每个发现的身份提供修改口令图标和简体中文弹窗；旧口令先验证，错误时原文件保持不变；新口令为空时移除加密；修改过程使用同目录临时文件和 Windows 原子替换，并保留 OpenSSH 公钥注释。旧口令、新口令只在 `Zeroizing` 中短暂存在，输入在取消、提交和失败后清空。新增 8 个 keygen 测试覆盖旧口令失败保护、加密/解密转换、未加密密钥加口令和注释保留。`cargo fmt --all`、离线 `cargo check --locked`、`cargo test --locked --offline`（109 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。真实 Windows DPI、中文 IME、Keys 弹窗实际交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：受限触发器完成并准备发布 `v0.1.5`。命令条和 More 菜单新增 Bell 图标入口；触发器引擎与单条规则默认关闭，规则最多 32 条，关键词按字面量不区分大小写匹配。动作限定为终端高亮、Toast 提示或通过既有终端队列发送预设文本，可选择追加 Enter；不执行任意脚本，规则只保存在内存。实时终端和日志回放共用触发器高亮，断开会话清理锁存，SFTP/隧道/快捷命令/设置/密钥/关键词面板保持互斥。新增触发器面板、输入提示和验证文案的简体中文翻译。`cargo fmt --all`、离线 `cargo check --locked`、`cargo test --locked --offline`（107 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。真实 Windows DPI、中文 IME 和触发器实际桌面交互仍需用户手工烟测。

2026-10-25，Windows/MSVC：已有 OpenSSH 身份发现与复用、Keys 弹窗简体中文本地化完成，准备发布 `v0.1.4`。Keys 面板只扫描用户 `~/.ssh` 目录中的候选私钥文件，不读取私钥内容；显示已有公钥算法和路径，可一键填入主机认证表单，并在 `.pub` 文件可用时复制公钥。生成新密钥后自动刷新列表；算法说明、路径、口令、生成和复制操作均已接入简体中文翻译。新增密钥发现过滤、排序、公钥注释行处理和异常路径单测；`cargo fmt --all`、离线 `cargo check --locked`、`cargo test --locked --offline`（104 项通过、9 项 Docker 专项跳过）、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1`（9 项通过）均已完成。Windows 人工截图确认新鲜构建的中文 Keys 面板，证据保存在 `artifacts/dshell-v0.1.4-keys-cn.jpg`。

2026-10-24，Windows/MSVC：SFTP 文件传输队列完成并准备发布 `v0.1.3`。文件选择器支持一次选择多个本地文件上传；每个会话维护 FIFO 队列，文件和目录传输共用串行调度。当前任务独占进度与覆盖确认，成功、失败或取消后自动派发下一项；SFTP 面板显示等待数量并可清空等待队列。`cargo fmt --all`、离线 `cargo check`、`scripts\verify-hard-constraints.ps1` 和 `scripts\test-docker-ssh.ps1` 通过；单元测试 101 项通过、9 项 Docker 专项测试跳过，Docker 集成测试 9 项全部通过。

2026-10-24，Windows/MSVC：新增 English / 简体中文界面语言设置，写入版本 1 的 `settings.json`，旧配置缺失语言字段时默认 English。命令栏、More 菜单、布局菜单、侧栏、会话属性、设置和常用工具面板的主要文案接入翻译表；终端输出与用户主机名、命令片段保持原样。新增 `scripts/start-docker-ssh-manual.ps1`，以 Docker Ubuntu 24.04 镜像启动仅绑定 `127.0.0.1:2222` 的临时 SSH 服务，输出临时账号、密码、host key 指纹和待人工核验的 known_hosts 行，不自动修改用户 known_hosts。`cargo fmt --all`、`scripts\verify-hard-constraints.ps1` 通过，单元测试 99 项通过、9 项 Docker 专用测试跳过；`scripts\test-docker-ssh.ps1` 9 项全部通过。Windows 人工截图确认 v0.1.2 新鲜构建的中文工作区和 SSH 连接表单，证据保存在 `artifacts/dshell-v0.1.2-chinese-shell.png` 与 `artifacts/dshell-v0.1.2-chinese-connection-form.png`；真实 DPI、IME 与字形像素对齐仍需隔离输入环境复核。

阶段一的 SSH 生命周期与终端交互核心已落实。阶段二工作区继续完善：左侧主机列表可按分组显示、标记收藏，并记录最近连接；主机资料字段包含分组和收藏状态，旧版资料文件可无损加载。身份验证已支持密码、OpenSSH 私钥（含口令）、键盘交互认证（含 OTP/多因素逐提示输入）和系统 SSH agent；Keys 面板可生成 Ed25519、RSA 3072/4096 和 ECDSA P-256/P-384 私钥/公钥对。OpenSSH `config` 导入已交付：支持 Host/HostName/User/Port/IdentityFile/LocalForward/RemoteForward/DynamicForward/ServerAliveInterval 和 Include，安全相关的不支持选项会拒绝对应主机而不是静默忽略。阶段四 SFTP 已交付：复用同一认证会话打开 SFTP 子系统，可在侧栏浏览远端目录、上传/下载单个文件和整个目录树、覆盖/保留/断点续传、查看进度和取消传输，并支持新建目录、重命名和删除文件/空目录。阶段五跳板机与端口转发已交付：目标连接通过已保存跳板主机的 `direct-tcpip` 通道建立，两跳分别执行严格 known_hosts 校验和独立认证；Tunnels 面板支持本地、远程和动态 SOCKS5 转发、自动分配端口、连接计数和停止规则，规则持久化到主机资料并在连接后自动启动。阶段六已交付会话日志、日志回放、命令片段和外观设置：日志通过原生保存对话框显式开启并记录原始终端输出；Replay log 通过原生打开对话框读取日志，用同一终端模拟器重放颜色和光标定位，超过 16 MiB 时只读取末尾并在面板中提示；终端搜索可检查活动会话的滚动历史和日志回放；片段可持久化、发送到活动会话，并先勾选目标再显式广播到多个会话；终端字体家族、字号、颜色主题和光标样式可在 Settings 面板调整并持久化，单元格尺寸和 PTY 行列随字号更新，主题和光标样式可即时推送到所有活动会话。多会话标签、独立终端状态、双会话可调整分屏和会话属性窗口已交付。当前支持两个 SSH 会话水平或垂直分屏、拖动调整比例、切换方向和关闭分屏；键盘、鼠标、滚动和 PTY resize 按 pane 所属 session ID 路由，并提供工作区快捷键。UI 改版阶段一至阶段六已全部完成：设计令牌与基础组件、应用外壳、会话与主机管理、终端工具条与检查器、统一弹层与 Toast、主题/DPI/对比度打磨均已落地；业务渲染代码不再包含裸颜色，设置、会话属性、密钥生成、认证提示和日志回放统一为可通过 `Esc` 关闭的模态。阶段 7 已交付可调侧栏/检查器宽度、主机密钥算法与 SHA256 指纹展示、检查器传输取消与转发启停、标签拖拽排序；阶段 8 已交付带时间戳的会话日志格式、回放时间轴跳转和检查器 Log 分组；组合输入栏已交付 96px 多行撰写、Ctrl+Enter 发送和最近 20 条历史。Windows 桌面像素级人工验收（真实 DPI、中文 IME、字形对齐）仍待隔离输入环境。

## 已落实

- GPUI 图标资源已修复：应用内置 SVG 图标源，支持工具栏、More 菜单和布局预设的图标在全新构建中显示；布局预设按钮移除会导致激活时断言失败的重复 hover 样式。Windows UI 截图已保存到 `artifacts/dshell-v0.1.1-layout-menu.png` 和 `artifacts/dshell-v0.1.1-more-menu.png`。
- 命令条改为扁平图标工具栏：新建、搜索、属性、分屏以及 Files/Tunnels/Log/Replay/Find/Quick commands/Keys/Sync/主题/设置均使用 28px 图标按钮和 tooltip，激活状态使用语义色表面。右侧新增 More 图标菜单，按 Session/View/Tools 分组列出新建会话、会话属性、导入配置、关闭标签、断开、水平/垂直分屏、侧栏、检查器、组合输入、同步输入、主题、文件、转发、日志、回放、查找、快速命令、密钥和设置；每行包含图标、名称和快捷键提示，`Esc` 或点击菜单外关闭。菜单只复用现有动作，不新增绕过确认的发送路径。
- 工作区布局菜单已落地：命令条 Layout 图标和 More 菜单共用独立弹层，提供 Focus、Standard、Inspector、Split 四个扁平预设；Split 在没有第二个会话时禁用，预设切换后保留可拖拽分隔条，`Esc` 或点击菜单外关闭并把焦点还给终端。
- 标签页操作菜单已落地：标签栏省略号按钮和标签右键打开扁平图标菜单，支持会话属性、横向/纵向分屏、断开、关闭当前标签、关闭其他标签和关闭右侧标签；菜单使用共享焦点遮罩，`Esc` 或点击外部关闭。
- 工作区布局菜单补充分屏方向图标操作：可在同一菜单中切换 Side by side、Stacked 或关闭分屏；没有两个会话时方向按钮禁用。
- 阶段 9.3 快速命令网格已落地：命令条和 More 菜单把原 Snippets 入口标为 Quick commands，面板使用扁平卡片网格展示可运行命令，运行、编辑和删除均有图标按钮与 tooltip；编辑区保留 `Send + Enter`/`Paste only` 选择，面板显示当前目标会话，广播仍要求显式勾选已连接会话。新增快速命令面板与菜单顶部图标快捷区使用统一设计令牌，补齐新建、搜索、布局和设置的高频入口。
- 阶段 9.4 关键字高亮已落地：命令条与 More 菜单提供 `Keyword highlights` 图标入口，面板最多保存 32 条内存规则，支持添加、删除和眼睛图标启停；匹配按字面量、不区分大小写，实时终端和日志回放共用同一渲染路径，默认关闭且不执行任何远端动作。触发器和脚本自动化仍保持独立评估边界。
- 阶段 9.2 认证配置模板已落地：会话属性 Authentication 分栏可把当前认证方式、身份文件路径、跳板引用和 keep-alive 保存为命名模板，写入 `%APPDATA%\dshell\auth_profiles.json`；点击模板会填充这些字段并清空密码、私钥口令和跳板口令输入，但不会发起连接或自动重连。模板文件版本为 1，限制 50 条、名称去重、公钥模板必须有身份文件、keep-alive 0–3600 秒；结构中没有密码、口令或密钥字段，单测断言序列化内容不含 `password`/`passphrase`/`private_key`。损坏文件会显示错误且不覆盖。
- 阶段 9.1 多会话同步输入已落地：命令条 `Sync` 按钮打开选择面板，只列出已连接会话并允许显式选择；启用后活动会话的键盘输入、终端粘贴和组合输入通过有界队列镜像到每个已选会话，状态栏显示 `Sync N`。鼠标、滚动、选择和 PTY resize 仍按会话独立，不参与广播；关闭会话会从目标集合移除，目标全部移除时自动关闭同步。片段广播保持原有独立确认流程，不随同步输入隐式发送。
- 组合输入栏已落地：`Ctrl+Shift+M` 或终端悬浮工具条的 Compose 按钮打开 96px 多行撰写窗格，`Ctrl+Enter` 通过终端粘贴通道发送到活动会话，`Esc` 关闭，`Send` 按钮等效；最近 20 条非空草稿去重后保存在内存中，`History` 按钮按新到旧循环回填且仍可编辑；发送后清空并保持输入焦点，打开或关闭撰写栏不会清理终端选中文本。新增 `ctrl+shift+m` 可配置工作区快捷键，仍不占用终端 `Ctrl+Shift+C/V` 或 `Alt+1..9`。
- 侧栏主机支持拖拽分组：拖动已保存主机行到现有分组标题或 UNGROUPED 即可修改 `group` 并立即写入 `hosts.json`；拖拽预览显示主机名，拖到当前分组不产生写入，保存失败时保留原分组并给出错误提示。分组标题在存在任意主机时始终显示 UNGROUPED 作为取消分组目标。
- 侧栏分组标题支持拖拽重排：拖动一个分组标题到另一个分组标题即可更新 `group_order`，顺序写入版本化 UI 设置；大小写、空白、超长键和重复项在保存前归一化，旧配置缺少字段时自动使用默认顺序。
- 阶段 8（日志时间轴与诊断）已落地：新建日志使用带 `DSHELL-LOG` 头和 `DSL2` 同步标记的记录格式，每条记录包含 Unix 毫秒时间戳和原始终端字节长度；每 16 ms 输出批次写为一条记录。回放读取会识别新旧格式：时间戳日志生成 timeline（时间、拼接流偏移、长度），旧原始日志仍可只读回放但没有时间轴；超过 16 MiB 的时间戳日志从尾部完整的记录边界开始。回放窗口在顶部提供 Prev event / Next event 和相对时间/序号，跳转时用同一终端模拟器从头重放到目标记录。检查器新增第四个可折叠 Log 组，显示记录状态、当前文件，并提供开始/停止记录和打开回放。日志仍可能包含敏感终端输出，应用不做脱敏、时间戳轮转或凭据过滤。
- 阶段 7（可调工作区与连接可观测性）已落地：侧栏可在 200–360px、检查器可在 300–520px 之间拖动，宽度写入 `settings.json`，旧版设置继续使用 248/380 默认值；拖动过程中只更新布局，释放后统一重算一次 PTY。SSH runtime 在严格 known_hosts 校验通过后上报服务器主机密钥算法和 SHA256 指纹，检查器 Connection 组展示；指纹只读，不提供自动信任或改写 known_hosts。检查器 Transfers 组显示实时速率并可取消传输；Forwards 组保留已停止规则并可重新启动，显示监听地址、绑定端口、目标和活动连接数。会话标签支持拖拽排序，`Alt+1..9` 跟随视觉顺序，排序只重排 `sessions` 向量，不重建 SSH 会话或终端状态。russh 0.64.1 未暴露协商后的 kex/cipher/MAC，因此这三项仍待库提供访问接口。
- UI 改版阶段六已落地：业务渲染代码不再包含裸颜色，SFTP、转发和片段面板全部改用语义令牌；终端选中前景/背景色移入三套 `TerminalTheme` 调色板，保证在深色和 Solarized 主题下选中文本对比度 ≥ 4.5:1。新增深/浅主题状态文字色（`accent_text`、`success_text`、`warning_text`、`danger_text`），Pill 文本在 14% 色调背景上的对比度 ≥ 4.5:1；焦点环与强调色在所有表面上 ≥ 3:1。新增单测覆盖主题对比度、终端选中对比度、9–32 px 字体缩放以及 100%/150%/200% DPI 对应的逻辑视口（1920×1080、1280×720、960×540），确认行列数始终为正且字号增大时列数递减。Windows IME 仍走原生 `InputState`，终端选中和快捷键回归由既有单测覆盖。
- UI 改版阶段五已落地：会话属性、设置、密钥生成、键盘交互认证和日志回放统一使用同一套模态遮罩、面板、标题栏和页脚（`modal_overlay` / `modal_panel` / `modal_header` / `modal_footer`），尺寸分别遵循 720×640、760×600、560、480 和“覆盖终端区”的设计规格。所有模态都通过共享焦点句柄接收 `Esc`：会话属性在删除确认阶段首次 `Esc` 只取消确认，再次 `Esc` 才关闭窗口；日志回放中若搜索条打开，首次 `Esc` 只关闭搜索。认证提示改用语义令牌，密码类提示默认掩码。设置和密钥生成从右侧停靠面板改为居中模态。删除资料、SFTP 删除和覆盖确认把安全操作（Keep / Cancel / Keep existing）排在危险操作之前，危险按钮使用 `danger` 变体。新增右下角 Toast 组件和 4 秒自动消失机制，覆盖复制/粘贴、资料保存/删除、配置导入、外观设置应用/重置和密钥生成，可手动关闭。
- UI 改版阶段四已落地：终端卡片右上角新增悬浮工具条，提供查找、复制、粘贴、日志回放、发送文件和检查器开关；默认 40% 不透明度，鼠标进入卡片后恢复为不透明。终端查找条宽度固定为卡片宽度减 32px，并与日志回放顶部工具栏共用语义令牌的描边、圆角、遮罩和阴影。右侧 380px 检查器分为 Transfers、Forwards、Connection 三组，可独立折叠；分别显示传输文件名/方向/进度、转发类型与监听地址/目标/活动连接数、主机/连接状态/终端尺寸/认证方式/资料名/跳板/keep-alive/日志状态。打开或关闭检查器会重算终端列数并向所有活动会话下发 PTY resize，折叠分组不改变 PTY 尺寸。终端清屏动作保持移除。
- UI 改版阶段三已落地：侧栏主机与最近连接改为统一的两行列表行（名称、`user@host:port`、选中强调条），收藏与编辑动作使用 28px 图标按钮常驻以便 `Tab`/`Enter` 访问；空列表和无匹配结果使用带标题、说明与操作按钮的空状态。会话属性窗口支持 `Esc` 取消和 `Ctrl+Enter` 保存，左导航与右表单均可仅用键盘操作。删除确认和收藏切换仍通过窗口内按钮完成。
- UI 改版阶段二已落地：应用外壳按设计重排为 52px 命令条、248px 可折叠侧栏（折叠后 48px 图标列，`Ctrl+Shift+B` 或标题栏按钮切换）、40px 标签栏、28px 状态栏和 8px 内缩的圆角终端卡片。命令条左侧为新建会话、搜索和分屏入口，中部显示当前会话名与状态胶囊，右侧承载 Files/Tunnels/Log/Replay/Find/Quick commands/Keys、界面主题与设置；标签页带状态点、分屏标记和关闭按钮，选中标签使用强调色顶部线和抬升表面。
- 侧栏折叠状态与 UI 深/浅主题写入 `settings.json`；旧版设置文件缺少这两个字段时使用深色主题和展开侧栏。主题切换通过 `ui::theme::install` 立即应用到 gpui-component 控件，命令条中的调色板按钮可在深/浅色间切换。
- 终端行列数改为从外壳尺寸推导（命令条、标签栏、状态栏、工作区内边距、卡片描边和面板头），折叠侧栏会重新计算 PTY 行列。新增单测验证 1280×800 下默认字号至少保留 80 列、1920×1080 更大，以及折叠侧栏只增加列数。
- UI 改版阶段一已落地：新增 `src/ui/{mod,theme,tokens,components}.rs`，集中定义深色/浅色两套语义色令牌（背景、表面、描边、文本、强调、状态、焦点环），以及 4px 网格的间距、尺寸、圆角、字号和动效令牌。设置面板与会话属性窗口不再包含裸颜色和裸像素值，改用 `Field`、`PanelHeader`、`HelpText`、`SectionLabel`、`Button` 等组件；其余面板的字段也已切换到同一 `Field` 组件。令牌通过 `ui::theme::install` 写入 gpui-component 全局主题，按钮、输入框、滚动条和焦点环因此共享同一语义色。默认 UI 主题为深色，浅色令牌已定义但尚无切换入口；终端配色仍由三套 `TerminalTheme` 独立控制。
- 会话属性窗口按设计规格调整为最大 720×640、圆角 12、遮罩和阴影，页脚主操作使用 36px 高按钮；设置面板宽度改为 380px，标题栏、正文和按钮统一使用令牌。
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
- 左侧主机栏可按名称、用户名、地址、端口和分组搜索资料，并新增、载入、更新或确认删除。资料编辑与已经建立的 SSH 会话互不影响。
- 主机资料支持分组和收藏：侧栏按收藏、分组（未分组归入 UNGROUPED）和最近连接分区显示，组内按名称排序；星标按钮可直接切换收藏。分组与收藏写入同一资料文件，旧版 version 1 文件自动按空分组、未收藏和空最近列表加载。
- 最近连接在连接启动后记录，按主机、端口和用户名去重，最多保留 20 条并持久化；点击最近条目会载入匹配的已保存资料，未保存时填充连接表单供再次连接，密码仍需重新输入。
- 身份验证支持四种方式：密码、OpenSSH 私钥、键盘交互和系统 SSH agent。键盘交互先用密码自动回答单个非回显的 Password/Passphrase 提示；OTP、验证码、多提示或回显提示会打开独立对话框，按服务器给出的提示数量逐项收集响应，响应只经一次性 channel 发回 SSH runtime，不写入资料或日志，提交后立即清空输入。所有密码和口令在连接或取消时清零。
- Keys 面板在后台执行器生成 OpenSSH Ed25519、RSA 3072/4096 或 ECDSA P-256/P-384 密钥对，支持可选口令和注释，写入用户选择的私钥路径及同名 `.pub` 文件；已存在的私钥或公钥不会被覆盖。私钥口令不持久化，生成后清空输入；Unix 私钥以 0600 创建，Windows 依赖用户目录 ACL。生成的公钥可复制到剪贴板，生成文件通过 `load_secret_key` 兼容现有私钥认证路径。算法选择不写入设置文件，切换算法会清除上一次生成的公钥展示。
- 私钥路径可持久化，口令只在连接流程中读取并清零；支持 `~`、`~\` 和 OpenSSH `%d` 展开到用户主目录，加密私钥口令错误会明确失败。Windows 上 agent 依次尝试 `SSH_AUTH_SOCK`/OpenSSH named pipe 和 Pageant，并对服务器支持的 RSA 签名算法选择 `rsa-sha2-*`。
- 会话属性窗口的 Authentication 分栏提供认证方式切换：密码模式显示密码框，私钥模式显示身份文件和口令框，agent 模式显示使用说明。切换、取消和连接都会清空密码与口令输入。
- SFTP 复用已认证 SSH 会话：认证完成后在独立 channel 请求 `sftp` 子系统，worker 与终端 I/O 同处专用 Tokio runtime，通过有界命令/事件 channel 与 GPUI 通信。命令或事件队列不可用会显示明确错误。
- SFTP 面板支持进入目录、返回上级、刷新、列出文件大小、权限和修改时间、上传单个文件和下载单个文件；本地文件/目录通过 GPUI 原生文件选择器选择。上传或下载目标已存在时显示 Overwrite、Resume 和 Keep existing 三个选择；Resume 在目标小于源文件时从目标当前大小继续，在目标不小于源文件时从头开始，已完成的文件直接计为传输完成。条目属性图标可编辑八进制权限、Unix 访问时间和修改时间，worker 使用 `SETSTAT` 更新并在完成后刷新目录。面板通过 `statvfs` 显示远端文件系统总容量、已用/可用/空闲空间、块大小和最长文件名，并在不支持扩展时保留明确状态。
- 上传/下载按 32 KiB 分块并限频上报进度；取消标志在每个分块边界检查，取消上传会删除远端临时文件，取消下载会删除本地临时文件。传输完成后上传会自动刷新当前远端目录。
- SFTP 面板跟随活动会话：切换标签后显示该会话独立的工作目录、条目、进度和错误；关闭或断开连接会清除对应 SFTP 通道状态，不影响其他会话。
- SFTP 支持在当前远端目录新建文件夹、重命名条目和删除文件或空目录。删除前必须在面板中再次确认；操作成功后面板自动刷新当前目录，失败原因（例如目录非空或权限不足）显示在面板消息中。名称输入拒绝空名称、`.`、`..` 和包含路径分隔符的值。
- SFTP 支持目录递归上传和下载：上传前递归统计文件名和总字节数，下载前递归读取远端目录并校验每个条目名；传输沿用 32 KiB 分块、限频进度和取消标志，最多 10,000 个条目、32 层嵌套。目录冲突提示保留目录类型，Overwrite 和 Resume 会重新发送对应目录传输。取消或失败时尽力删除本次新建的远端文件/目录或本地文件/目录；已存在且被覆盖的文件可能保留部分内容，断点续传的文件会保留部分内容以便下次继续。
- 目录上传/下载对目标根目录做一次覆盖确认；符号链接和其他特殊文件跳过；远端条目名必须是安全的单段名称，避免下载路径逃逸。
- 跳板机使用另一个已保存主机资料：目标 SSH 握手通过跳板会话的 `direct-tcpip` channel 建立，跳板和目标分别用各自的 host/user/port 校验 known_hosts。未知或变更的跳板密钥会立即失败，不会因为经过代理而跳过检查。
- 跳板机和目标各自选择认证方式；跳板使用密码或加密私钥时，会话属性窗口提供独立的“Jump password”或“Jump key passphrase”输入，连接后与目标凭据一起清零。跳板提供的 SFTP 子系统仍属于目标连接，下载和终端流量都经过跳板。
- 跳板引用写入主机资料（`proxy_jump`），保存主机可同时保存跳板密码和口令；导出资料会移除这些凭据，私钥材料不会复制。保存资料时要求跳板指向另一个已保存主机；连接时拒绝自引用、指向同一目标地址和二级嵌套跳板。
- 端口转发按会话独立管理：本地和动态规则在客户端绑定监听端口，远程规则通过 `tcpip-forward` 请求由服务器监听；端口 0 自动分配并在 Tunnels 面板显示实际端口。每条规则显示活动连接数，可单独停止；断开会话时关闭本地监听器、取消远端转发并清理映射。
- 转发规则写入主机资料的 `forwards`，连接该资料时在认证完成后自动启动；在 Tunnels 面板新增或停止规则会同步更新资料文件。快速连接仍使用会话级规则。规则数量上限为 32，保存时校验监听地址和目标完整性。
- 主机资料支持 `keepalive_seconds`（0 到 3600 秒，0 关闭）；连接时设置 russh 的 keepalive interval，跳板和目标连接都会应用该配置。
- 会话日志默认关闭：点击 Log 后通过 GPUI 原生保存对话框选择文件，SSH runtime 以追加模式打开，按 16 ms 渲染节拍批量写入收到的 `ChannelMsg::Data`/`ExtendedData` 原始字节；停止日志或断开连接时刷新并关闭文件。文件打开失败会显示状态消息且不会进入记录状态。
- 左侧 Import 按钮通过原生文件选择器读取 OpenSSH `config`。导入支持前置于 Host 的全局 User/Port/IdentityFile/ServerAliveInterval 默认值、`Include`（相对路径、`~`、`%d`，禁止循环和过深嵌套）、Host 多别名以及 LocalForward/RemoteForward/DynamicForward；导入的主机归入 `Imported` 分组，重名主机跳过并提示。
- 导入不会静默应用无法支持的安全/路由语义：`Match` 整体拒绝导入；包含 ProxyJump、ProxyCommand、StrictHostKeyChecking、UserKnownHostsFile、IdentitiesOnly、ForwardAgent、X11/RemoteCommand、ControlMaster 等选项的 Host 会跳过并给出警告；通配符 Host 块不应用并显示警告。未知选项汇总为一条警告。
- 日志记录的是远端原始终端字节流，包含 ANSI 转义序列和终端上出现的一切内容（可能含敏感信息）；应用不会对内容做脱敏、时间戳或轮转，只在运行时上保持有界缓冲，避免逐包磁盘写入阻塞终端。
- 命令片段写入用户配置目录的 `dshell/snippets.json`，包含名称、命令和是否自动回车；名称区分大小写去重，最多 200 条。片段面板支持新建、载入、保存、删除和发送到活动会话。
- 多会话发送必须先勾选已连接的目标会话，面板按钮显示目标数量；点击后只向所选会话发送当前命令。发送通过终端粘贴通道执行，遵循远端 bracketed-paste 模式，自动回车使用普通 Enter 键序列。
- 外观设置写入用户配置目录的 `dshell/settings.json`，包含字体家族、9-32 px 字号、终端主题（Default dark / Solarized dark / Solarized light）、光标样式（Block / Underline / Bar）和工作区快捷键绑定。应用后按 13 px 默认单元格 8x18 的比例线性换算单元格尺寸，重新计算终端行列并向所有活动会话发送 PTY resize；窗口缩放、滚动像素换算、分屏尺寸和鼠标坐标都使用同一组度量。
- Settings 面板支持 Apply 和 Reset；字体家族为空或字号越界时归一化为默认值，快捷键必须包含至少一个修饰键和恰好一个键，重复绑定会被拒绝。设置文件缺失时使用默认 `monospace` 13 px、Default dark 主题、Block 光标和默认快捷键，损坏或版本不支持时保留错误消息。
- 终端主题在 `terminal.rs` 中定义前景、弱前景、背景和 16 色 ANSI 调色板；帧渲染使用该调色板解析命名色和索引色，应用主题时通过 `TerminalRequest::SetTheme` 推送到每个活动会话的终端模拟器并立即重绘。光标样式通过 `TerminalRequest::SetCursorStyle` 推送；Block 仍以反显单元格绘制，Underline 和 Bar 在光标行列位置叠加细条。
- 本地和远程转发通过目标会话的 `direct-tcpip` 打开通道，动态转发的 SOCKS5 握手在客户端监听任务中完成并设置 10 秒超时；双向数据复制在独立任务中运行，不阻塞终端 I/O。转发打开失败、目标不可达和握手失败都会显示在 Tunnels 面板。
- 新建或编辑主机资料时显示独立属性窗口，按 Connection、Authentication、Security 分类；保存仅持久化资料，Connect 单独启动 SSH，取消或连接时清空密码，密码不进入主机资料文件。
- 主窗口按主机导航、连接工具区、会话标签、终端画布和底部状态栏分区；快速连接或保存主机后可继续使用其他活动会话。
- 每个会话标签单独持有 SSH 命令/取消通道、连接状态、终端帧、PTY 尺寸、滚动余量和鼠标位置。切换标签只切换当前视图；关闭标签会通知对应 SSH 运行时取消，不影响其他标签。
- 多会话隔离测试验证 SSH 输出事件只更新指定会话、终端输入只进入指定会话的命令队列、关闭会话只触发其取消信号并正确回退活动标签。
- 窗口尺寸变化会向每个活动会话分别发送 PTY resize；状态栏显示当前标签的目标地址、连接状态和终端行列数。
- 双会话可在左右或上下方向分屏，GPUI 可拖动分隔条；分屏面板分别显示会话标题和连接状态，点击 pane 后将键盘、鼠标和滚动路由到该会话。调整分隔条或窗口大小时分别按 pane 尺寸更新两路 PTY；非分屏会话保持完整窗口尺寸。切换方向保留对应方向的窗口内分隔比例，创建新活动会话时自动回到单 pane 工作区。
- 分屏尺寸换算和 session ID 到 pane 尺寸的映射有单测；Docker SSH/PTY 集成验证覆盖远端运行时 resize。
- 终端聚焦时支持可配置的工作区快捷键：默认 Ctrl/Cmd+Shift+T 打开新会话窗口、W 关闭当前标签、F 切换文件面板、S 切换片段面板、O 切换设置、D 断开当前会话、R 回放日志；Alt+1 至 Alt+9 固定用于切换会话标签。快捷键保存在设置中，格式为 `ctrl+shift+t`、`alt+F12` 或 `cmd+space`；不合法或重复的绑定会拒绝保存并显示错误。Ctrl/Cmd+Shift+C/V 保留为复制和粘贴。
- Replay log 从原生打开对话框选择日志，在后台执行器读取文件，最多读取末尾 16 MiB，然后用当前字体、主题和光标样式创建独立 `TerminalEmulator` 重放。覆盖层为只读，可滚动历史并使用 Top/Bottom 跳转；不会连接远端、发送输入或修改日志文件。
- 终端搜索通过 Ctrl/Cmd+F 打开浮动搜索栏，可搜索活动会话的可见屏幕与滚动历史，日志回放窗口打开时搜索该回放。匹配为单行、ASCII 不区分大小写；Enter/Next 向后、Shift+Enter/Prev 向前并循环，Esc 关闭。命中内容通过终端选择高亮，可直接复制；新终端输出或 resize 会清空缓存匹配，下一次搜索从第一个匹配开始。
- 主机资料写入用户配置目录的 `dshell/hosts.json`，只包含名称、主机、端口和用户名；密码不进入持久化结构。资料文件损坏或不可读时显示错误，并阻止覆盖保存。
- scripts/verify-hard-constraints.ps1 固化格式、类型检查、测试和依赖边界检查命令。
- scripts/test-docker-ssh.ps1 构建本地可缓存 Ubuntu 24.04 SSH 测试镜像，包含 OpenSSH、Vim、tmux、htop；固定软件包索引和安装留在 Docker build layer，重复运行可复用。容器仍临时创建，以随机密码、实际 Ed25519 公钥和隔离的临时 known_hosts 执行 SSH/PTY 集成测试，结束后清理容器和临时密钥文件。

## 验证记录

2026-10-24，Windows/MSVC：`v0.1.1` 修复 gpui-component 图标在全新构建中缺少 SVG 资源导致不可见的问题，并修复布局预设按钮激活时的重复 hover 样式断言。`cargo fmt --all`、离线 `cargo check` 和 `scripts\verify-hard-constraints.ps1` 全部通过；全目标类型检查成功，106 项测试中 97 项通过、9 项 Docker 专用测试跳过。变更不涉及 SSH/认证/SFTP/转发/日志/PTY 路径，未重复运行 Docker 集成脚本。Windows UI 截图确认 More 与布局菜单图标可见；真实 DPI、IME 和字形像素对齐仍需隔离交互环境人工检查。下一段继续受限触发器评估。

2026-10-24，Windows/MSVC：首个公开版本 `v0.1.0` 发布基线完成。新增 `CHANGELOG.md`，并在 `AGENTS.md` 固化版本、验证、标签和推送规则；`Cargo.toml` 版本保持 `0.1.0`。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、锁定依赖全目标类型检查和 105 项测试（96 项通过，9 项 Docker 专用测试跳过）均通过；`./scripts/test-docker-ssh.ps1` 9 项 SSH/认证/SFTP/转发/日志/PTY 集成测试全部通过。真实 Windows DPI、中文 IME、字形对齐和 GUI 交互仍需隔离桌面人工烟测，未在发布说明中宣称已验证。

2026-10-24，Windows/MSVC：标签页图标操作菜单与布局分屏方向完成。标签栏省略号和标签右键提供属性、横向/纵向分屏、断开、关闭当前/其他/右侧标签；布局菜单补充 Side by side、Stacked 和关闭分屏图标操作。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 105 项测试（96 项通过，9 项 Docker 专用测试跳过）均通过。本次未触碰 SSH/认证/SFTP/PTY 通道，因此未重复 Docker 集成测试。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 9.4 关键字高亮完成。命令条与 More 菜单新增 Keyword highlights 图标入口，面板支持最多 32 条内存规则、添加/删除和启停；终端实时画面与日志回放按字面量、不区分大小写共用高亮渲染，规则默认关闭且不执行命令。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 105 项测试（96 项通过，9 项 Docker 专用测试跳过）均通过；新增关键字匹配纯逻辑单测。本次未触碰 SSH/认证/SFTP/PTY 通道，因此未重复 Docker 集成测试。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：工作区布局预设菜单完成。命令条 Layout 图标与 More 菜单共用 Focus、Standard、Inspector、Split 四个预设，Split 在缺少第二个会话时禁用；菜单使用独立焦点遮罩，`Esc` 或点击外部关闭。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 104 项测试（95 项通过，9 项 Docker 专用测试跳过）均通过；未触碰 SSH/认证/SFTP/PTY 通道，因此未重复 Docker 集成测试。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 9.3 快速命令网格完成。命令条与 More 菜单新增 Quick commands 图标入口；片段面板改为扁平命令卡片网格，卡片支持运行，旁置图标按钮支持编辑和删除，编辑区保留 Send + Enter/Paste only 选择，广播目标继续要求显式选择已连接会话。More 菜单顶部增加新建、搜索、布局和设置图标快捷区。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 104 项测试（95 项通过，9 项 Docker 专用测试跳过）均通过；本次为 UI 与已存在片段动作的重排，未新增 SSH/PTY 路径，因此未重复 Docker 集成测试。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：命令条与菜单现代化完成。命令条改为扁平图标工具栏，右侧新增按 Session/View/Tools 分组的图标菜单，包含图标、名称、快捷键提示和激活态；`Esc` 或点击菜单外关闭，所有条目复用现有动作。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 104 项测试（95 项通过，9 项 Docker 专用测试跳过）均通过；本次为 UI 重构，未新增纯逻辑测试。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 9.2 认证配置模板完成。会话属性可保存/应用/删除命名模板，模板保存认证方式、身份文件路径、跳板引用和 keep-alive，写入 `auth_profiles.json`；应用模板会填充字段并清空密码、私钥口令和跳板口令，不自动连接。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 104 项测试（95 项通过，9 项 Docker 专用测试跳过）均通过；新增存储往返、缺失文件、重名/公钥缺身份文件、数量上限和“序列化不含秘密字段”测试。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 9.1 多会话同步输入完成。命令条 `Sync` 面板只列出已连接会话并允许显式选择；启用后键盘、终端粘贴和组合输入镜像到所选会话，鼠标/滚动/resize 保持单会话；状态栏显示 `Sync N`，关闭会话自动移除目标，目标清空后关闭同步。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 100 项测试（91 项通过，9 项 Docker 专用测试跳过）均通过；新增同步目标过滤/去重/活动会话包含规则单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：组合输入栏完成。`Ctrl+Shift+M` 可配置快捷键和终端工具条按钮打开 96px 多行撰写窗格，`Ctrl+Enter` 通过终端粘贴通道发送，`Esc` 关闭；最近 20 条草稿去重保存在内存并可通过 History 回填编辑。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 99 项测试（90 项通过，9 项 Docker 专用测试跳过）均通过；新增组合历史去重/上限/循环和快捷键路由单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：侧栏主机拖拽分组完成。主机行可拖到现有分组标题或 UNGROUPED，修改 `group` 后立即写入 `hosts.json` 并重新排序；拖到当前分组不写入，保存失败保留原分组并显示错误。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 98 项测试（89 项通过，9 项 Docker 专用测试跳过）均通过；新增分组移动、无变化和缺失主机单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：密钥生成扩展完成。Keys 面板支持 Ed25519、RSA 3072/4096 和 ECDSA P-256/P-384，算法选择只保留在当前界面状态，不写入设置；生成仍不覆盖已有文件、口令不持久化、Unix 私钥 0600，并通过 `load_secret_key` 兼容现有私钥认证路径。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 97 项测试（88 项通过，9 项 Docker 专用测试跳过）均通过；新增 RSA/ECDSA 加密与未加密生成、公钥前缀和私钥加载测试。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 8（日志时间轴与诊断）完成。新日志使用 `DSHELL-LOG` 头、`DSL2` 同步标记、Unix 毫秒时间戳和长度前缀；回放读取兼容旧原始日志，截断的时间戳日志从完整记录边界开始。回放工具栏增加 Prev/Next event 和相对时间/序号，检查器增加可折叠 Log 组。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 96 项测试（87 项通过，9 项 Docker 专用测试跳过）均通过；新增格式往返、旧日志兼容、尾部边界、帧布局和时间格式化单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过，真实会话日志测试断言新格式、非空时间轴和单调时间戳。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：阶段 7（可调工作区与连接可观测性）完成。侧栏 200–360px 与检查器 300–520px 支持拖拽并写入 `settings.json`，拖动结束统一重算 PTY；SSH 校验通过后上报主机密钥算法与 SHA256 指纹并显示在检查器；Transfers 组增加实时速率和取消，Forwards 组保留已停止规则并可重新启动；标签支持拖拽排序且 `Alt+1..9` 跟随视觉顺序。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 91 项测试（82 项通过，9 项 Docker 专用测试跳过）均通过；新增面板宽度夹取/列数、指纹格式、传输速率、停止规则和标签排序单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过，终端往返测试断言真实会话返回 SHA256 指纹。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：UI 改版阶段六（打磨与验证）完成。清除业务渲染代码中的全部裸颜色，SFTP、转发和片段面板改用语义令牌；终端选中色移入三套终端调色板，新增深/浅主题高对比度状态文字色并在 Pill 中使用。新增主题对比度、终端选中对比度、9–32 px 字体缩放和 100%/150%/200% DPI 逻辑视口单测。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 85 项测试（76 项通过，9 项 Docker 专用测试跳过）均通过；`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。真实 Windows DPI、中文 IME 和字形像素对齐仍需隔离桌面人工烟测。

2026-10-24，Windows/MSVC：UI 改版阶段五（弹层与反馈）完成。会话属性、设置、密钥生成、键盘交互认证和日志回放统一为语义令牌模态；设置改为 760×600、密钥生成改为 560px 居中模态；所有模态通过共享焦点句柄支持 `Esc`，删除确认阶段首次 `Esc` 只撤销确认。认证提示的密码类字段恢复默认掩码，删除资料、SFTP 删除和覆盖确认将安全操作排在危险操作之前。新增右下角 Toast（4 秒自动消失、可手动关闭），覆盖复制/粘贴、资料保存/删除、配置导入、外观设置和密钥生成。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 82 项测试（73 项通过，9 项 Docker 专用测试跳过）均通过；新增 `Esc` 优先级和模态宽度规格单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：UI 改版阶段四（终端工具条与检查器）完成。终端卡片新增悬浮工具条（查找、复制、粘贴、日志回放、发送文件、检查器开关，默认 40% 不透明度并在悬停卡片时恢复）；查找条宽度为卡片宽度减 32px，日志回放顶部栏改用同一套令牌；右侧 380px 检查器提供 Transfers、Forwards、Connection 三组独立折叠，开关检查器会重算终端列数并下发 PTY resize。同时恢复工作区中被误删的 3 项终端单测（选中文本、双宽/组合字符、resize），并修正分屏下检查器开关按钮的重复 `ElementId`。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 80 项测试（71 项通过，9 项 Docker 专用测试跳过）均通过；新增检查器列数单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：UI 改版阶段三（会话与主机管理）完成。侧栏主机与最近连接改用统一的两行 `SidebarRow`，收藏/编辑动作常驻且可键盘聚焦，空列表与无匹配结果使用带操作的 `EmptyState`；会话属性窗口增加 `Esc` 取消和 `Ctrl+Enter` 保存。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 79 项测试（70 项通过，9 项 Docker 专用测试跳过）均通过；新增会话属性窗口键盘动作单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：UI 改版阶段二（应用外壳）完成。命令条、可折叠侧栏、标签栏、状态栏和圆角终端卡片改用语义令牌；侧栏折叠状态与深/浅 UI 主题写入设置文件，`Ctrl+Shift+B` 切换侧栏。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 78 项测试（69 项通过，9 项 Docker 专用测试跳过）均通过；新增设置文件向后兼容、1280×800 至少 80 列、1920×1080 更大以及折叠侧栏增加列数的单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：UI 改版阶段一（设计令牌与基础组件）完成。新增 `src/ui/{mod,theme,tokens,components}.rs`；设置面板与会话属性窗口不再包含裸颜色和裸像素值，所有面板的输入字段统一改用 `Field` 组件。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 75 项测试（66 项通过，9 项 Docker 专用测试跳过）均通过；新增 7 项令牌/组件测试，覆盖深色与浅色主题正文对比度 ≥ 4.5:1、语义色区分、间距与控件/圆角/字号顺序、按钮高度档位和语义色映射。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：Ed25519 密钥生成、终端/日志搜索、可配置工作区快捷键、日志回放、终端光标样式、键盘交互 OTP/多因素提示与 SFTP 断点续传完成。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 59 项测试（50 项通过，9 项 Docker 专用测试跳过）均通过；新增密钥生成/加密/`load_secret_key` 兼容、搜索命中/循环/选择、快捷键解析/冲突检测/自定义绑定、日志尾部读取、光标帧状态、密码提示自动应答决策和断点续传偏移单测。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过，终端测试新增真实 SSH 会话内搜索标记并校验选择结果，同时覆盖运行时主题和光标样式切换；SFTP 往返测试新增 100 KiB 文件的上传续传（远端保留前 40 KiB）和下载续传（本地保留前 30 KiB），并验证最终内容与源文件一致。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Windows/MSVC：终端颜色主题完成。`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 47 项测试（38 项通过，9 项 Docker 专用测试跳过）均通过；新增主题调色板/切换单测，`docker_ssh_terminal_round_trip` 在真实 SSH 会话中验证 Solarized light 与 Default dark 的运行时切换。`./scripts/test-docker-ssh.ps1` 9 项集成测试全部通过。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-24，Docker SSH/SFTP/跳板机/转发/日志集成：`./scripts/test-docker-ssh.ps1` 通过，9 项集成测试全部通过；脚本以单线程串行运行，避免并行测试的时序干扰。键盘交互认证使用 PAM 密码提示完成连接；SFTP 测试覆盖目录列举、上传、覆盖确认、下载内容一致、新建目录、重命名、删除文件、删除目录、嵌套目录递归上传/下载及失败路径；会话日志测试读回远端输出；端口转发测试覆盖本地、SOCKS5 和远程回连；跳板机、终端和公钥认证测试同时通过。

2026-10-24，Windows/MSVC：`./scripts/verify-hard-constraints.ps1` 通过，格式检查、全目标类型检查和 55 项测试（46 项通过，9 项 Docker 专用测试跳过；按 `docker_ssh_` 过滤时 9 项全部通过）均通过。新增外观设置默认值/往返/越界归一到测试，以及键盘交互认证、OpenSSH config、片段、SOCKS5、转发持久化、跳板、SFTP、资料兼容和最近连接测试。Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-23，Windows/MSVC：会话属性窗口、工作区工具栏及原有断开操作通过 `./scripts/verify-hard-constraints.ps1`，格式检查、类型检查和 28 项测试均通过（26 项通过，2 项 Docker 专用测试跳过）。Cargo 报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-23，Windows/MSVC：`./scripts/verify-hard-constraints.ps1` 全部通过，包含格式检查、锁定依赖下的全目标类型检查和 28 项测试（26 项通过，2 项 Docker 专用测试由独立脚本运行）。新增的分屏尺寸换算与 session ID 尺寸路由测试通过；Cargo 仅报告传递依赖 `proc-macro-error2 2.0.1` 的 future-incompatibility 提示。

2026-10-23，Docker SSH/PTY 集成：`./scripts/test-docker-ssh.ps1` 通过，两项集成测试通过。Ubuntu 镜像的 OpenSSH、Vim、tmux、htop 安装层显示 `CACHED`，本次未重新更新软件包索引。

2026-10-22，Windows/MSVC：`./scripts/verify-hard-constraints.ps1` 全部通过，包含格式检查、锁定依赖下的全目标类型检查、禁用依赖扫描及 20 项通过的单元测试（另有 2 项 Docker 专用测试由独立脚本运行）。本轮 UI/多会话改动通过全目标编译；自动化暂未覆盖多个活动标签之间的并行输出与关闭隔离。

`./scripts/test-docker-ssh.ps1` 已连续通过三次。本轮 Docker 日志中 OpenSSH、Vim、tmux、htop 安装层显示 `CACHED`，没有再次更新索引或下载软件包；两项 SSH/PTY 测试通过，覆盖 known_hosts 校验、密码认证、PTY resize（`stty size` 返回 `30 100`）、终端选择/粘贴/鼠标报告、Vim 全屏 Unicode 输入保存与 shell 读回，以及正常退出和主动断开。测试使用随机密码和独立的临时 known_hosts，测试容器及临时密钥文件均清理。

Cargo 对传递依赖 `proc-macro-error2 2.0.1` 输出 future-incompatibility 警告；当前不影响检查或测试通过。

2026-10-22，Windows 桌面烟测尝试：虚拟桌面只隔离窗口，不隔离指针和键盘；发现输入仍与用户共享后已停止桌面操作。个人 known_hosts 未修改。应用界面的实际像素布局、tmux/htop 全屏行为、Unicode 字形对齐及 IME/焦点交互仍待人工烟测。

## 当前限制

- UI 改版六阶段和阶段 7（可调工作区与连接可观测性）已完成。侧栏（200–360px）与检查器（300–520px）可拖拽并持久化宽度，标签可拖拽排序且 `Alt+1..9` 跟随视觉顺序；列表行的悬停才显示动作（当前动作常驻以支持键盘）仍未实现。命令条已改为扁平图标工具栏，More 菜单按 Session/View/Tools 分组提供图标、名称和快捷键提示，标签上下文菜单也已支持右键和省略号入口；仍不支持子菜单、自定义工具栏或面板停靠。Transfers 组已显示实时速率并可取消，但不支持暂停；Forwards 组可停止并重新启动规则，已停止规则只保留在当前会话内。Connection 组展示主机密钥算法与 SHA256 指纹，但 russh 0.64.1 未暴露协商后的 kex/cipher/MAC。标签顺序不跨重启持久化。会话属性、设置、密钥生成、认证提示和日志回放已统一为模态并支持 `Esc`；SFTP、转发和片段面板仍是右侧停靠样式，模态尚无 140–180 ms 的出现/关闭过渡动画，Toast 也没有排队或历史记录。深/浅主题对比度与 9–32 px 字号、三档 DPI 对应的逻辑视口已用单测覆盖，但 Windows 100%/150%/200% 实际缩放、中文 IME 组合输入和终端字形像素对齐仍需在隔离桌面人工烟测。
- 键盘映射覆盖常见按键和修饰键，但尚未验证完整 IME 组合输入；文本选择仍按字符范围处理，终端图像协议未实现。
- 当前通过 GPUI flex 元素绘制样式化网格，尚未实现自定义 `TerminalElement`、字体 shaping、精确字素宽度/重排或渲染性能优化。
- 当前认证已覆盖密码、OpenSSH 私钥、键盘交互（含 OTP/多因素逐提示输入）和系统 agent；Keys 面板可发现 `~/.ssh` 中的候选身份、填入主机认证表单、复制 `.pub` 公钥、生成 Ed25519/RSA/ECDSA、修改口令，以及导入、导出和删除密钥对。GSSAPI、证书文件和硬件密钥仍待实现。Windows 上私钥文件权限依赖用户目录 ACL，未额外设置 ACL。
- 首次连接不会弹出信任确认；用户需先通过独立可信渠道验证服务器指纹并写入 OpenSSH known_hosts。
- Windows OpenSSH agent/Pageant 路径已实现但尚未在安装并加载身份的桌面环境做自动化验证；私钥口令错误与隔离 Docker 私钥认证已覆盖。
- 跳板机当前只支持一层已保存主机引用，不支持 `ProxyJump` 列表、ProxyCommand、按通配符/`Match` 自动选择，也不支持在连接提示中临时输入跳板缺口令以外的交互式认证。跳板连接失败原因会显示在会话状态中，但不会针对单跳弹出独立重试对话框。
- SFTP 当前已支持单文件和目录断点续传、多选上传及每会话 FIFO 传输队列，尚不支持拖放、符号链接传输或把目录元数据自动保留到递归传输；目录传输失败时只清理本次新建的条目，覆盖已有文件时可能留下部分内容。同一会话的 SFTP 请求串行处理，传输期间导航请求会排队。上传/下载目标冲突只检查存在性，最终覆盖仍由服务器原子性语义决定。
- 多会话和分屏输入/输出路由、关闭取消及 PTY 尺寸分配已有纯状态与真实 channel 单测；尚未做 GPUI 窗口级交互自动化和异常 UI 生命周期验证。分屏比例只保留在当前窗口运行期间，不跨应用重启持久化。Windows 桌面中的真实 IME/焦点、全屏程序和 Unicode 字形像素对齐尚未人工烟测；虚拟桌面仍共享鼠标和键盘，不能作为隔离烟测环境。
- 端口转发规则已持久化到主机资料并在连接后自动启动；尚未实现离线规则编辑、转发规则导入/导出、UNIX socket 转发、按连接限速和独立活动连接详情列表。规则停止时会关闭本地监听器或取消远端转发，已有连接随通道关闭而结束。
- OpenSSH config 导入只应用上文列出的字段：通配符 Host 和 Match 条件不应用；ProxyJump/ProxyCommand 在导入阶段被拒绝，需要手动创建跳板资料；`%h`、`%r`、`%p` 等令牌和 Include 通配符未展开；`IdentityFile none` 和多 IdentityFile 只保留第一个并提示。
- 外观设置当前覆盖字体家族/字号、颜色主题、光标样式和工作区快捷键，单元格宽度/高度按默认字体比例线性估算，不进行真实字形测量；字体未安装时会回退到系统等宽字体。尚未实现行高/字距微调、闪烁光标或按会话覆盖全局外观。快捷键只支持修饰键加单个键，`+` 本身不能作为绑定键，Alt+数字切标签固定不可改。
- 命令片段目前是全局单层列表，不支持分组、变量替换、快捷键、定时执行或脚本条件；广播只按当前选中会话发送，不持久化目标选择，也没有发送前逐条回显命令内容的确认对话框（按钮和已选目标列表是显式操作）。
- 会话日志新文件使用带时间戳的记录格式，旧原始日志以及追加到旧日志的会话仍无时间轴。日志不提供明文/HTML 转换、轮转、上限或脱敏；日志文件可能包含密码、令牌和远端输出中的其他敏感信息，用户需自行保护。日志状态不跨应用重启保存。Replay log 对时间戳日志从完整记录边界读取最后 16 MiB 并支持 Prev/Next event 跳转，跳转会从头重放到目标记录；旧原始日志超过 16 MiB 时起点仍可能落在转义序列中间。日志未保存原始 PTY 尺寸，重放按当前字体换算固定视口，行换行位置可能与录制时不同；不提供时间轴搜索、按时间范围过滤或导出。
- 终端搜索只做单行匹配，不支持正则、全词/大小写敏感选项、跨行匹配或高亮全部命中；搜索通过覆盖用户当前选择来显示命中，新输出后需重新搜索。
- 端口转发当前以端口作为远程转发映射键，同一端口绑定到多个地址的场景未区分；本地监听失败、目标拒绝和服务器拒绝均有面板错误。
- 认证模板只保存认证方式、身份文件路径、跳板引用和 keep-alive，不包含用户名、端口或任何秘密；模板不会自动绑定到主机资料，需在会话属性中手动应用，应用后必须重新输入密码/口令。模板文件损坏时保留错误信息并阻止覆盖。
- 同步输入目标只保存在当前进程，不跨重启；同步范围是键盘、粘贴和组合输入，不包含鼠标、滚动、resize 或片段广播。多会话广播尚无“全选/按分组选择”。关键字高亮与受限触发器规则只保存在当前进程，均最多 32 条且仅支持字面量、不区分大小写匹配；触发器动作限定为高亮、Toast 或预设文本，不执行本地 Shell 或任意脚本。快速命令网格已交付，Xshell 对照能力矩阵与阶段 9 顺序见 ROADMAP。
- 侧栏分组支持最多 8 层嵌套、从分组标题创建子组、拖拽主机归组、折叠层级和同级重排；重命名/删除覆盖整个子树，目标路径冲突会拒绝重命名。旧版扁平分组和 UI 设置继续兼容。最近连接记录在连接启动时写入，不区分认证失败和成功握手。

当前发布版 `v0.1.23`、维护版本 `v0.1.22`、`v0.1.21`、`v0.1.20`、`v0.1.19`、`v0.1.18`、`v0.1.17`、`v0.1.16`、`v0.1.15`、`v0.1.14`、`v0.1.13`、`v0.1.12`、`v0.1.11`、`v0.1.10`、`v0.1.9`、`v0.1.8`、`v0.1.7`、`v0.1.6`、`v0.1.5`、`v0.1.4`、`v0.1.3`、`v0.1.2`、`v0.1.1` 和首个公开版本 `v0.1.0` 的发布范围及验证记录见 [CHANGELOG.md](CHANGELOG.md)。

## 下一段接力

完整路线、阶段依赖和验收条件见 [ROADMAP.md](ROADMAP.md)。阶段 7、8、9.1（同步输入）、9.2（认证模板）、9.3（快速命令网格）、9.4（关键字高亮）和 9.5（受限触发器）已完成，侧栏嵌套分组、主机拖拽、分组折叠与同级重排、组合输入栏、服务控制和 Xshell 能力矩阵已补充。真实 Windows DPI、中文 IME 和终端字形像素对齐仍作为独立人工烟测任务，继续遵守严格主机密钥拒绝策略和导出备份脱敏约束。
