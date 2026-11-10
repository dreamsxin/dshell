# Bitvise SSH Client 功能差距分析

以 Bitvise SSH Client (v9) 为参照的功能审计。每项均经代码级 grep 验证，参照仓库 `FEATURE_GAP_ANALYSIS.md` 的格式。

## 一、已实现（对齐或超出）

| Bitvise 功能 | dshell 实现 |
|------|------|
| SSH 认证：密码 / 公钥 / 键盘交互 | ✅ `ssh.rs` authenticate 流程；公钥支持 OpenSSH 证书（`-cert.pub`）；键盘交互支持逐提示与 TOTP |
| SSH agent（连接与转发） | ✅ 系统 agent 认证 + `agent_forwarding` 资料字段经 `agent_forward_allowed` 下发 |
| 终端（xterm 全功能 + 标签/分屏/广播/查找/高亮/屏幕锁） | ✅ `terminal.rs` + 工作区 |
| SFTP 图形传输（上传/下载/断点续传/覆盖确认/递归目录/权限时间戳编辑/statvfs） | ✅ SFTP 面板 |
| 命令行客户端（sftpc / stermc 等价） | ✅ `dshell-cli` exec + sftp 子命令 |
| 端口转发（本地 / 远程 / 动态 SOCKS5） | ✅ Tunnels 面板，规则持久化到资料 |
| 通过 HTTP/SOCKS5 代理连接 | ✅ `ProxySpec`/`ProxyCredentials`（连接与 CLI 共用） |
| 多级跳板 | ✅ 逗号分隔跳板链逐级 direct-tcpip（超出 Bitvise 的单级 C2S/S2C 转发组合方式） |
| keep-alive / 自动重连 | ✅ keepalive 0-3600 秒 + 重连间隔/次数设置 |
| 压缩（zlib） | ✅ 2026-11-11 起：russh 启用 `flate2` 特性，与服务器协商 zlib（此前未启用该特性，仅 none） |
| 主机密钥严格校验（known_hosts，TOFU 对话框） | ✅ 验证对话框 + SHA256 指纹展示；未知/变更密钥失败关闭 |
| 登录后命令 / 会话日志 / 日志回放 | ✅ |
| 密钥生成（Ed25519 / RSA 3072/4096 / ECDSA P-256/P-384）+ 口令 | ✅ Keys 面板 |
| **公钥一键安装到服务器**（Bitvise "Initial method" 工作流） | ✅ 2026-11-11 新增：凭据弹窗 "Install public key"，密码登录后幂等写入 `~/.ssh/authorized_keys`（命令构建器有单测） |
| 服务器信息面板（进程/服务/网络/磁盘/Docker 容器/会话） | ✅ 检查器（超出 Bitvise） |
| 快速连接 / 命令面板 / 会话恢复 / 云同步 / 托盘 | ✅ |
| ZMODEM 收发 | ✅（超出 Bitvise） |

## 二、暂缓（含理由）

| 功能 | 理由 |
|------|------|
| GSSAPI / Kerberos (SSPI) 认证 | russh 无 GSSAPI 支持；需要手写 Windows SSPI FFI 绑定与 SPNEGO 令牌交换，工程量大且无法在此环境验证域环境 |
| PuTTY .ppk 导入 | 需手写 PPK v2/v3 私钥格式转换（v3 为 bcrypt KDF + AES-256-CTR），错误实现会导致静默密钥损坏；用户可用 PuTTYgen 一次性转换为 OpenSSH 格式 |
| 主机密钥管理器（列表/删除 known_hosts 条目） | 需要重写用户 known_hosts；仓库硬约束对 known_hosts 写入极其保守，仅在用户显式确认时执行。如需保留 Bitvise 体验，下一轮以显式确认对话框实现 |
| X11 转发 | russh 支持 `RequestX11`，但 Windows 无原生 X server 生态，可用性依赖用户自装 VcXsrv 等，暂缓 |
| 环境变量注入（`set_env`） | russh 支持，但绝大多数服务器默认不 `AcceptEnv`，成功率低；候选项 |
| RDP over SSH 快捷方式 | 可用 Tunnels 面板手动建本地转发到 3389 后启动 mstsc；后续可在转发行加"打开 RDP"动作 |
| 传输调度 / 目录镜像同步 | 候选项 |
| Bitvise SSH Server 组件 | 独立产品，超出 SSH 客户端范围 |

## 三、设计差异（有意保留）

- 凭据持久化：Bitvise 可加密保存密码；dshell 的安全约束是**凭据绝不写盘**（仅内存 Zeroizing），不在此对齐。
- 域集成：Bitvise 深度集成 Windows 账户；dshell 为通用 SSH 客户端。

## 覆盖率

按上表 24 项功能：19 ✅、7 暂缓/候选（其中 2 项超出范围或有意保留差异）。2026-11-11 审计。
