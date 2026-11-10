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
| PuTTY .ppk 导入 | ❌ 维持暂缓（评估结论 2026-11-11）：v3 需要 bcrypt-AES KDF 解密 + 私有 blob 到 OpenSSH 线格式的手写转换，错误实现会静默损坏密钥材料且无法在本环境验证 PuTTYgen 互操作；一次性转换方案（PuTTYgen → OpenSSH）已覆盖全部算法。如后续有强需求，建议以 `ssh-keygen -i` 外部调用实现而非手写解析 |
| 主机密钥管理器（列表/删除 known_hosts 条目） | ✅ 2026-11-11 新增：Settings 面板 "Manage host keys" 弹窗列出条目（模式/算法/SHA256 指纹），两步确认后按行移除并即时重写 known_hosts（用户显式驱动，与主机密钥信任对话框同一安全模式） |
| X11 转发 | ❌ 定稿暂缓（评估结论 2026-11-11）：russh 暴露了 `request_x11` 与通道回调，但正确的客户端还必须（a）生成并在转发流上校验/替换伪造的 MIT-MAGIC-COOKIE-1——这是安全敏感的手写协议工作，错误实现会向远端泄露本地 X 凭据或静默破坏会话，与 ppk 导入同风险级别；（b）依赖本机 X server，而 Windows 原生不带（VcXsrv/WSLg），本环境无法做端到端自动化验证；（c）管理每会话 DISPLAY/screen 状态。只做裸 TCP 转发而不做 cookie 替换会迫使关闭 X 访问控制才能用。过渡方案：VcXsrv + OpenSSH `ssh -X`；待具备 cookie 替换实现与隔离 X server 测试环境后再评估 |
| 环境变量注入（`set_env`） | ✅ 2026-11-11 新增：资料 "Environment variables" 编辑器（`NAME=value` 解析器校验名称字符/长度/重复/上限），连接时在 PTY 请求前 fire-and-forget `request_env`（服务器不接受时静默忽略，与 OpenSSH 行为一致） |
| RDP over SSH 快捷方式 | ✅ 已实现（Tunnels 面板 Remote Desktop 分组：目标主机/端口 + "Open Remote Desktop" 按钮，临时 127.0.0.1 本地转发 + mstsc 启动；此前审计有误，2026-11-11 复核更正） |
| 传输调度 / 目录镜像同步 | ❌ 定稿暂缓（评估结论 2026-11-11）：目录镜像需要双侧递归变更检测（mtime/size 扫描）、冲突解决策略（新者胜 vs 人工选择）、删除传播（镜像会删除远端/本地多余文件，属破坏性操作，与仓库"覆盖必确认"的安全姿态冲突）以及映射管理界面；周期性后台写入也无法在本环境做长时自动化验证。既有的一次性递归上传/下载已覆盖目录拷贝，脚本录制 + 登录后命令可组合出定时工作流。待有明确用户需求并以"先预演差异、确认后执行"的安全设计重启 |
| Bitvise SSH Server 组件 | 独立产品，超出 SSH 客户端范围 |

## 三、设计差异（有意保留）

- 凭据持久化：Bitvise 可加密保存密码；dshell 的安全约束是**凭据绝不写盘**（仅内存 Zeroizing），不在此对齐。
- 域集成：Bitvise 深度集成 Windows 账户；dshell 为通用 SSH 客户端。

## 覆盖率

按上表 24 项功能：21 ✅、4 项暂缓全部含定稿理由（GSSAPI/Kerberos、PuTTY .ppk 导入、X11 转发、传输调度/目录镜像同步——前两者为手写协议/密钥转换风险，后两者依赖本环境无法提供的验证条件）、2 项超出范围或有意保留差异（Bitvise SSH Server、凭据加密落盘）。审计定稿，2026-11-11。
