use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// User-facing language for the application shell.
///
/// Terminal data and remote command output are never translated.
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    #[default]
    English,
    SimplifiedChinese,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::English, Self::SimplifiedChinese];

    pub const fn label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::SimplifiedChinese => "简体中文",
        }
    }

    pub const fn is_simplified_chinese(self) -> bool {
        matches!(self, Self::SimplifiedChinese)
    }
}

/// Translate a stable English UI key while preserving unknown text for future
/// features and user-provided names.
pub fn translate<'a>(language: Language, key: &'a str) -> Cow<'a, str> {
    if !language.is_simplified_chinese() {
        return Cow::Borrowed(key);
    }
    let translated = match key {
        "Settings" => "设置",
        "Close" => "关闭",
        "Apply" => "应用",
        "Reset" => "重置",
        "English" => "English",
        "Simplified Chinese" => "简体中文",
        "Language" => "语言",
        "Dark" => "深色",
        "Light" => "浅色",
        "Default dark" => "默认深色",
        "Solarized dark" => "Solarized 深色",
        "Solarized light" => "Solarized 浅色",
        "Block" => "块状",
        "Underline" => "下划线",
        "Bar" => "竖线",
        "Switch to Dark theme" => "切换到深色主题",
        "Switch to Light theme" => "切换到浅色主题",
        "Terminal theme" => "终端主题",
        "Cursor style" => "光标样式",
        "Workspace shortcuts" => "工作区快捷键",
        "Font family" => "字体",
        "Font size" => "字号",
        "Format: ctrl+shift+t, alt+F12 or cmd+space. Alt+1..9 always switches tabs." => {
            "格式：ctrl+shift+t、alt+F12 或 cmd+space。Alt+1..9 始终用于切换标签页。"
        }
        "Allowed range: 9-32 px. Cell size and PTY dimensions follow the font size." => {
            "允许范围：9–32 px。终端单元格和 PTY 尺寸会随字号调整。"
        }
        "Appearance and shortcuts applied." => "外观和快捷键已应用。",
        "Appearance and shortcuts reset to defaults." => "外观和快捷键已恢复默认值。",
        "Settings storage is unavailable." => "设置存储不可用。",
        "Dismiss" => "关闭提示",
        "Find" => "查找",
        "Prev" => "上一个",
        "Next" => "下一个",
        "Top" => "顶部",
        "Bottom" => "底部",
        "Prev event" => "上一事件",
        "Next event" => "下一事件",
        "Replay log" => "回放日志",
        "Replay log (Ctrl+Shift+R)" => "回放日志 (Ctrl+Shift+R)",
        "Search terminal" => "搜索终端",
        "Search saved hosts" => "搜索已保存主机",
        "New session" => "新建会话",
        "Close session" => "关闭会话",
        "Toggle files" => "切换文件面板",
        "Toggle snippets" => "切换片段面板",
        "Toggle settings" => "切换设置",
        "Toggle sidebar" => "切换侧栏",
        "Toggle compose" => "切换命令编辑器",
        "Compose a multi-line command, then press Ctrl+Enter" => "编辑多行命令，然后按 Ctrl+Enter",
        "New session (Ctrl+Shift+T)" => "新建会话 (Ctrl+Shift+T)",
        "New SSH Session" => "新建 SSH 会话",
        "Split with another session" => "与另一会话分屏",
        "Session Properties" => "会话属性",
        "Session properties" => "会话属性",
        "No active sessions" => "没有活动会话",
        "Connection" => "连接",
        "Server health" => "服务器健康",
        "Processes" => "进程",
        "processes" => "个进程",
        "Top CPU" => "CPU 占用最高",
        "Services" => "服务",
        "Collecting service status..." => "正在采集服务状态……",
        "No services reported yet." => "尚未获取服务状态。",
        "Connect to the server before refreshing services." => "请先连接服务器，再刷新服务状态。",
        "Timed out waiting for server services." => "等待服务器服务状态超时。",
        "The remote host did not return a supported service response." => {
            "远端主机未返回受支持的服务响应。"
        }
        "The remote host returned a malformed service row." => "远端主机返回了格式错误的服务行。",
        "The remote host returned an invalid service row." => "远端主机返回了无效的服务行。",
        "The remote host returned too many service rows." => "远端主机返回了过多服务行。",
        "The server service response was too large." => "服务器服务响应过大。",
        "Server alerts" => "服务器告警",
        "CPU alert threshold (%)" => "CPU 告警阈值（%）",
        "Memory alert threshold (%)" => "内存告警阈值（%）",
        "Disk alert threshold (%)" => "磁盘告警阈值（%）",
        "Values at or above a threshold show a warning in the server inspector." => {
            "达到或超过阈值时，服务器检查器会显示警告。"
        }
        "Alert thresholds must be whole percentages from 1 to 100." => {
            "告警阈值必须是 1 到 100 的整数百分比。"
        }
        "CPU threshold exceeded." => "CPU 已超过告警阈值。",
        "Memory threshold exceeded." => "内存已超过告警阈值。",
        "Disk threshold exceeded." => "磁盘已超过告警阈值。",
        "Refreshing..." => "刷新中……",
        "Collecting server metrics..." => "正在采集服务器指标……",
        "No server metrics collected yet." => "尚未采集服务器指标。",
        "Collecting process list..." => "正在采集进程列表……",
        "No processes collected yet." => "尚未采集进程列表。",
        "Operating system" => "操作系统",
        "Kernel" => "内核",
        "Load average" => "负载均值",
        "Memory" => "内存",
        "Root disk" => "根分区",
        "Uptime" => "运行时间",
        "Unavailable" => "不可用",
        "Connect to the server before refreshing metrics." => "请先连接服务器，再刷新指标。",
        "The SSH session is not accepting requests." => "SSH 会话当前不接受请求。",
        "The SSH request queue is full." => "SSH 请求队列已满。",
        "Timed out waiting for server metrics." => "等待服务器指标超时。",
        "Connect to the server before refreshing processes." => "请先连接服务器，再刷新进程。",
        "Timed out waiting for server processes." => "等待服务器进程超时。",
        "The remote host did not return a supported process response." => {
            "远端主机未返回受支持的进程响应。"
        }
        "The remote host did not expose any process rows." => "远端主机未提供进程数据。",
        "The remote host returned a malformed process row." => "远端主机返回了格式错误的进程行。",
        "The remote host returned an invalid process ID." => "远端主机返回了无效的进程 ID。",
        "The remote host returned an invalid process CPU value." => {
            "远端主机返回了无效的进程 CPU 值。"
        }
        "The remote host returned an invalid process memory value." => {
            "远端主机返回了无效的进程内存值。"
        }
        "The remote host returned an invalid process row." => "远端主机返回了无效的进程行。",
        "The remote host returned too many process rows." => "远端主机返回了过多进程行。",
        "The server process response was too large." => "服务器进程响应过大。",
        "The remote host did not return a supported metrics response." => {
            "远端主机未返回受支持的指标响应。"
        }
        "The remote host did not expose usable system metrics." => "远端主机未提供可用的系统指标。",
        "The server metrics response was too large." => "服务器指标响应过大。",
        "The server metrics command exited with a non-zero status." => {
            "服务器指标命令以非零状态退出。"
        }
        "Authentication" => "认证",
        "Security" => "安全",
        "Session name" => "会话名称",
        "Host" => "主机",
        "Port" => "端口",
        "Group" => "分组",
        "Group order updated." => "分组顺序已更新。",
        "New group" => "新建分组",
        "Rename group" => "重命名分组",
        "Delete group" => "删除分组",
        "Group name" => "分组名称",
        "Group name is required." => "请输入分组名称。",
        "Group name is too long." => "分组名称过长。",
        "UNGROUPED is reserved." => "未分组是保留名称。",
        "A group with this name already exists." => "已存在同名分组。",
        "The group to rename is unavailable." => "要重命名的分组不可用。",
        "The group to delete is unavailable." => "要删除的分组不可用。",
        "The group will be removed and its hosts moved to Ungrouped." => {
            "该分组将被删除，其中的主机将移至未分组。"
        }
        "Group created." => "分组已创建。",
        "Group renamed." => "分组已重命名。",
        "Group deleted; hosts moved to Ungrouped." => "分组已删除，主机已移至未分组。",
        "Loaded {name}. Enter the credentials to connect." => "已载入{name}。请输入凭据以连接。",
        "Loaded {name}. Passwords are never saved; enter the password to connect." => {
            "已载入{name}。密码不会保存，请输入密码后连接。"
        }
        "Enter credentials for {name}. They will be remembered in memory for this run." => {
            "请输入{name}的认证信息。本次运行期间会在内存中记住。"
        }
        "Authenticate {name}" => "认证{name}",
        "Authenticate {target}" => "认证{target}",
        "SSH connection" => "SSH 连接",
        "Enter the authentication information and confirm to connect." => {
            "请输入认证信息，然后确认连接。"
        }
        "The password is remembered only in memory for this run and is never written to the host profile." => {
            "密码只会在本次运行的内存中记住，不会写入主机资料。"
        }
        "Leave the passphrase empty when the private key is not encrypted. It is remembered only in memory for this run." => {
            "如果私钥未加密，可将口令留空。口令只会在本次运行的内存中记住。"
        }
        "Uses the SSH agent; no password is required." => "使用 SSH agent，无需密码。",
        "Credentials are cleared from the form after connecting or cancelling. A successful connection can reuse them during this run; they are never written to host profiles." => {
            "连接或取消后会清空表单中的凭据。连接成功后，本次运行期间可以复用凭据；凭据不会写入主机资料。"
        }
        "Loaded {name} from recent connections." => "已从最近连接载入{name}。",
        "Loaded a recent connection. Save it as a host profile if you want to keep it." => {
            "已载入最近连接。若要保留，请将其保存为主机资料。"
        }
        "Added {name} to favorites." => "已将{name}加入收藏。",
        "Removed {name} from favorites." => "已将{name}移出收藏。",
        "Moved {name} to Ungrouped." => "已将{name}移至未分组。",
        "Moved {name} to {group}." => "已将{name}移至{group}。",
        "Could not move the host." => "无法移动主机。",
        "Saved {name}." => "已保存{name}。",
        "Deleted {name}." => "已删除{name}。",
        "Host profiles could not be loaded; fix the profile file before saving." => {
            "主机资料无法加载，请先修复资料文件再保存。"
        }
        "Host profile storage is unavailable." => "主机资料存储不可用。",
        "Keep-alive must be between 0 and 3600 seconds (0 disables it)." => {
            "保活必须介于 0 到 3600 秒之间（0 表示关闭）。"
        }
        "Port must be between 1 and 65535." => "端口必须介于 1 到 65535 之间。",
        "Profile name, host, username, and a valid port are required." => {
            "主机资料名称、主机、用户名和有效端口不能为空。"
        }
        "Public key authentication requires an identity file path." => "公钥认证需要身份文件路径。",
        "A host cannot use itself as its jump host." => "主机不能将自己设置为跳板机。",
        "Jump host {name} is not another saved host." => "跳板机{name}不是其他已保存的主机。",
        "A host named {name} already exists." => "已存在名为{name}的主机。",
        "Host and user are required." => "主机和用户名不能为空。",
        "A password is required for password authentication." => "密码认证需要输入密码。",
        "SSH password authentication failed" => "SSH 密码认证失败。",
        "SSH password authentication was rejected" => "SSH 密码认证被拒绝。",
        "A password is required for keyboard-interactive authentication." => {
            "键盘交互认证需要输入密码。"
        }
        "SSH keyboard-interactive authentication failed" => "SSH 键盘交互认证失败。",
        "SSH keyboard-interactive authentication was rejected" => "SSH 键盘交互认证被拒绝。",
        "SSH public key authentication failed" => "SSH 公钥认证失败。",
        "SSH public key authentication was rejected" => "SSH 公钥认证被拒绝。",
        "SSH agent authentication was rejected" => "SSH agent 认证被拒绝。",
        "An identity file path is required for public key authentication." => {
            "公钥认证需要输入身份文件路径。"
        }
        "Jump host {name} is not a saved host." => "跳板机{name}不是已保存的主机。",
        "Jump host {name} already uses jump host {jump}. Nested jump hosts are not supported yet." => {
            "跳板机{name}已经使用跳板机{jump}。暂不支持多级跳板机。"
        }
        "The jump host resolves to the same target; choose a different jump host." => {
            "跳板机解析到相同目标，请选择其他跳板机。"
        }
        "Jump host {name}: {error}" => "跳板机{name}：{error}",
        "Checking host keys and connecting to {host}:{port}{route}..." => {
            "正在检查主机密钥并连接到 {host}:{port}{route}……"
        }
        "Connecting..." => "连接中……",
        "Connected, but recent connections could not be saved: {error}" => {
            "连接成功，但无法保存最近连接：{error}"
        }
        "Could not start SSH runtime: {error}" => "无法启动 SSH 运行时：{error}",
        "Applied authentication template {name}. Re-enter any password or passphrase." => {
            "已应用认证模板{name}。请重新输入密码或口令。"
        }
        "Applied authentication template {name}." => "已应用认证模板{name}。",
        "Authentication template {name} was not found." => "未找到认证模板{name}。",
        "Enter a template name." => "请输入模板名称。",
        "Keep-alive must be a whole number of seconds." => "保活必须是整数秒数。",
        "Authentication profile storage is unavailable." => "认证资料存储不可用。",
        "Could not save the authentication template." => "无法保存认证模板。",
        "Saved authentication template {name}." => "已保存认证模板{name}。",
        "Select or name a template to delete." => "请选择要删除的模板，或输入模板名称。",
        "Could not delete the authentication template." => "无法删除认证模板。",
        "Deleted authentication template {name}." => "已删除认证模板{name}。",
        "name" => "名称",
        "Unix seconds" => "Unix 秒",
        "Favorite" => "已收藏",
        "Not favorite" => "未收藏",
        "Jump host" => "跳板机",
        "Keep-alive (seconds, 0 = off)" => "保活（秒，0 = 关闭）",
        "Protocol" => "协议",
        "Username" => "用户名",
        "Authentication method" => "认证方式",
        "Password" => "密码",
        "Passwords are remembered only in memory for this run and are never saved with host profiles." => {
            "密码只会在本次运行的内存中记住，不会随主机资料保存。"
        }
        "Hidden keyboard-interactive prompts are answered with this password. One-time codes and visible prompts are not supported yet." => {
            "隐藏的键盘交互提示会使用此密码回答。一次性验证码和明文提示暂不支持。"
        }
        "Public key" => "公钥",
        "Keyboard-interactive" => "键盘交互",
        "Keyboard interactive" => "键盘交互",
        "Identity file" => "身份文件",
        "Key passphrase" => "密钥口令",
        "Agent" => "代理",
        "Uses the Windows OpenSSH agent named pipe or Pageant. Load the key into the agent before connecting." => {
            "使用 Windows OpenSSH agent 命名管道或 Pageant。连接前请先将密钥加载到 agent。"
        }
        "Jump password" => "跳板机密码",
        "Jump key passphrase" => "跳板机密钥口令",
        "Server identity" => "服务器身份",
        "Credential handling" => "凭据处理",
        "Hosts with the same group are listed under one folder in the sidebar." => {
            "侧栏会将相同分组的主机归入同一文件夹。"
        }
        "No saved templates. Save the current method as a reusable template." => {
            "没有已保存的模板。保存当前认证方式即可重复使用。"
        }
        "Templates store the method, identity file path, jump-host reference and keep-alive only. Passwords, passphrases and key material are never saved; applying a template clears secret inputs." => {
            "模板只保存认证方式、身份文件路径、跳板机引用和保活设置。密码、口令和密钥材料不会保存；应用模板会清空秘密输入。"
        }
        "The identity file path is saved with the host. Passphrases and key material are never saved." => {
            "身份文件路径会随主机保存。口令和密钥材料不会保存。"
        }
        "Jump credentials are used only for the jump host and are never saved." => {
            "跳板机凭据只用于跳板机连接，不会保存。"
        }
        "Host keys must already match an entry in the user's OpenSSH known_hosts file. Unknown and changed keys are rejected. When a jump host is used, both the jump host and the target are verified independently." => {
            "主机密钥必须已匹配用户 OpenSSH known_hosts 中的条目。未知或变更的密钥会被拒绝。使用跳板机时，跳板机和目标主机会分别验证。"
        }
        "Password and key passphrase fields are cleared when you connect or cancel. Saved host profiles contain connection details, an optional identity file path, and a jump host reference, never passwords, passphrases, or key material." => {
            "连接或取消时会清空密码和密钥口令。已保存的主机资料只包含连接信息、可选身份文件路径和跳板机引用，不包含密码、口令或密钥材料。"
        }
        "Save" => "保存",
        "Cancel" => "取消",
        "Connect" => "连接",
        "Keep" => "保留",
        "Delete" => "删除",
        "Edit host" => "编辑主机",
        "Connect to host" => "连接主机",
        "Remove favorite" => "取消收藏",
        "Add favorite" => "加入收藏",
        "Load into the connection form" => "载入连接表单",
        "Quick connect" => "快速连接",
        "Files" => "文件",
        "Tunnels" => "隧道",
        "Log" => "日志",
        "Keys" => "密钥",
        "Compose" => "编辑命令",
        "Quick commands" => "快捷命令",
        "Keyword highlights" => "关键词高亮",
        "Triggers" => "触发器",
        "Highlight" => "高亮",
        "Notify" => "通知",
        "Send text" => "发送文本",
        "Send text + Enter" => "发送文本并回车",
        "No triggers" => "没有触发器",
        "Add a disabled rule, then enable it when you are ready." => {
            "先添加一条禁用规则，确认后再启用。"
        }
        "Trigger keyword" => "触发关键词",
        "Action" => "动作",
        "Preset text" => "预设文本",
        "Send + Enter" => "发送并回车",
        "Paste only" => "仅粘贴",
        "Add trigger" => "添加触发器",
        "Enable trigger" => "启用触发器",
        "Disable trigger" => "停用触发器",
        "Remove trigger" => "删除触发器",
        "Enable trigger engine" => "启用触发器引擎",
        "Disable trigger engine" => "停用触发器引擎",
        "Close triggers" => "关闭触发器",
        "Rules are disabled by default. Actions are limited to highlighting, a toast, or preset text." => {
            "规则默认禁用，动作仅限于高亮、Toast 提示或发送预设文本。"
        }
        "Enable individual rules to react to new terminal output." => {
            "启用单条规则后，它才会响应新的终端输出。"
        }
        "The trigger engine is off until you enable it." => "启用触发器引擎后才会开始匹配。",
        "Trigger engine enabled." => "触发器引擎已启用。",
        "Trigger engine disabled." => "触发器引擎已停用。",
        "Triggers are limited to 32 rules." => "触发器最多支持 32 条规则。",
        "Trigger is already listed:" => "触发器已存在：",
        "Added disabled trigger:" => "已添加禁用触发器：",
        "Enabled trigger:" => "已启用触发器：",
        "Disabled trigger:" => "已停用触发器：",
        "Removed trigger:" => "已删除触发器：",
        "Trigger matched:" => "触发器匹配：",
        "Trigger could not send:" => "触发器无法发送：",
        "Trigger could not send Enter:" => "触发器无法发送回车：",
        "Trigger sent preset text for:" => "触发器已发送预设文本：",
        "Enter a keyword for the trigger." => "请输入触发器关键词。",
        "Trigger keywords are limited to 128 characters." => "触发器关键词最多 128 个字符。",
        "Enter preset text for the trigger." => "请输入触发器预设文本。",
        "Preset trigger text is limited to 1024 characters." => "触发器预设文本最多 1024 个字符。",
        "preset command, for example: systemctl status ssh" => {
            "预设命令，例如：systemctl status ssh"
        }
        "Inspector" => "检查器",
        "Connection info" => "连接信息",
        "Transfers" => "传输",
        "Forwards" => "转发",
        "No connected sessions" => "没有已连接的会话",
        "No entries" => "没有条目",
        "No forwarding rules" => "没有转发规则",
        "SFTP" => "SFTP",
        "Up" => "上级",
        "Refresh" => "刷新",
        "Upload" => "上传",
        "Folder" => "文件夹",
        "Download" => "下载",
        "Rename" => "重命名",
        "New folder" => "新建文件夹",
        "Edit metadata" => "编辑属性",
        "Metadata" => "属性",
        "Permissions (octal)" => "权限（八进制）",
        "Access time (Unix)" => "访问时间（Unix）",
        "Modified time (Unix)" => "修改时间（Unix）",
        "Save metadata" => "保存属性",
        "Invalid permission mode." => "权限格式无效。",
        "Invalid access time." => "访问时间无效。",
        "Invalid modified time." => "修改时间无效。",
        "Show disk usage" => "显示磁盘用量",
        "Disk usage" => "磁盘用量",
        "Available" => "可用",
        "Free space" => "空闲",
        "Block size" => "块大小",
        "Max filename length" => "最长文件名",
        "Filesystem statistics unavailable." => "远端未提供文件系统统计信息。",
        "Cancel transfer" => "取消传输",
        "Queued transfers" => "排队中的传输",
        "Clear queue" => "清空队列",
        "Keep existing" => "保留现有文件",
        "Resume" => "续传",
        "Overwrite" => "覆盖",
        "Add tunnel" => "添加隧道",
        "Stop" => "停止",
        "Start" => "启动",
        "Open session log" => "打开会话日志",
        "Import OpenSSH config" => "导入 OpenSSH 配置",
        "Import dshell host profiles" => "导入 dshell 主机资料",
        "Export host profiles" => "导出主机资料",
        "Host profiles are unavailable for export." => "主机资料不可用于导出。",
        "Could not export host profiles." => "无法导出主机资料。",
        "Could not import host profiles." => "无法导入主机资料。",
        "Exported {count} host profile(s) to {path}." => "已将{count}个主机资料导出到{path}。",
        "Imported {added} host profile(s) from {path}; skipped {skipped} duplicate(s)." => {
            "已从{path}导入{added}个主机资料，跳过{skipped}个重复项。"
        }
        "Upload file" => "上传文件",
        "Choose download folder" => "选择下载文件夹",
        "Upload folder" => "上传文件夹",
        "Waiting for the SFTP subsystem..." => "正在等待 SFTP 子系统……",
        "Remote name" => "远端名称",
        "directory" => "目录",
        "file" => "文件",
        "Authentication templates" => "认证模板",
        "Template name" => "模板名称",
        "Save template" => "保存模板",
        "Algorithm" => "算法",
        "Comment" => "备注",
        "Generate" => "生成",
        "key" => "密钥",
        "Private key path" => "私钥路径",
        "Passphrase (optional)" => "口令（可选）",
        "Suggested private key path" => "建议私钥路径",
        "Modern default, small keys and fast signatures." => "现代默认算法，密钥较小、签名速度快。",
        "Compatible RSA key with a 3072-bit modulus." => "兼容性 RSA 密钥，模长为 3072 位。",
        "Larger RSA key with a 4096-bit modulus." => "更大的 RSA 密钥，模长为 4096 位。",
        "NIST P-256 ECDSA key." => "NIST P-256 ECDSA 密钥。",
        "NIST P-384 ECDSA key." => "NIST P-384 ECDSA 密钥。",
        "Creates an OpenSSH private key and a .pub file. Existing files are never overwritten. The passphrase is not stored; Unix key files use mode 0600." => {
            "创建 OpenSSH 私钥和 .pub 公钥文件。不会覆盖已有文件。口令不会保存；Unix 私钥文件权限为 0600。"
        }
        "Copy public key" => "复制公钥",
        "Public key copied." => "公钥已复制。",
        "Import key" => "导入密钥",
        "Import OpenSSH private key" => "导入 OpenSSH 私钥",
        "Importing key..." => "正在导入密钥……",
        "Imported key" => "已导入密钥",
        "Key imported." => "密钥已导入。",
        "Key import failed." => "密钥导入失败。",
        "Export key" => "导出密钥",
        "Exporting key..." => "正在导出密钥……",
        "Exported key" => "已导出密钥",
        "Key exported." => "密钥已导出。",
        "Key export failed." => "密钥导出失败。",
        "Delete key" => "删除密钥",
        "Keep key" => "保留密钥",
        "The private key and its .pub companion will be permanently removed from disk." => {
            "私钥及其 .pub 公钥文件将从磁盘永久删除。"
        }
        "Deleting key..." => "正在删除密钥……",
        "Key deleted." => "密钥已删除。",
        "Key deletion failed." => "密钥删除失败。",
        "Change passphrase" => "修改口令",
        "Change key passphrase" => "修改密钥口令",
        "Current passphrase" => "当前口令",
        "New passphrase (optional)" => "新口令（可选）",
        "Leave the new passphrase empty to remove encryption." => "新口令留空可移除加密。",
        "Update passphrase" => "更新口令",
        "No key selected." => "未选择密钥。",
        "Changing passphrase..." => "正在修改口令……",
        "Passphrase updated." => "密钥口令已更新。",
        "Passphrase update failed." => "密钥口令更新失败。",
        "Created" => "已创建",
        "Generating" => "正在生成",
        "key pair generated" => "密钥对已生成",
        "Key generation failed." => "密钥生成失败。",
        "Enter a private key path." => "请输入私钥路径。",
        "Selected for the next SSH connection." => "已选中，将用于下一次 SSH 连接。",
        "Could not scan" => "无法扫描",
        "User keys" => "用户密钥",
        "Existing keys" => "已有密钥",
        "No existing OpenSSH keys found in ~/.ssh." => "在 ~/.ssh 中没有找到已有 OpenSSH 密钥。",
        "Use for SSH" => "用于 SSH",
        "Identity file selected." => "已选择身份文件。",
        "No saved hosts" => "没有已保存的主机",
        "No matching hosts" => "没有匹配的主机",
        "Import an OpenSSH config or create a host to get started." => {
            "导入 OpenSSH 配置或新建主机以开始使用。"
        }
        "Try a different name, group, user or port." => "试试其他名称、分组、用户名或端口。",
        "Clear search" => "清除搜索",
        "New host" => "新建主机",
        "New" => "新建",
        "Add host" => "添加主机",
        "Import" => "导入",
        "FAVORITES" => "收藏",
        "UNGROUPED" => "未分组",
        "RECENT" => "最近连接",
        "Favorites" => "收藏",
        "Recent" => "最近连接",
        "Focus" => "聚焦",
        "Standard" => "标准",
        "Side by side" => "左右并排",
        "Stacked" => "上下堆叠",
        "Split" => "分屏",
        "SPLIT ORIENTATION" => "分屏方向",
        "Workspace layout" => "工作区布局",
        "Choose a workspace preset. Divider sizes remain adjustable." => {
            "选择工作区布局预设。分隔线仍可拖动调整。"
        }
        "Focus workspace" => "聚焦工作区",
        "Standard workspace" => "标准工作区",
        "Workspace with inspector" => "工作区与检查器",
        "Split active session" => "分屏当前会话",
        "Use the divider handles to fine-tune widths after choosing a preset." => {
            "选择预设后，可拖动分隔线微调宽度。"
        }
        "SESSION" => "会话",
        "VIEW" => "视图",
        "TOOLS" => "工具",
        "TABS" => "标签页",
        "Quick actions" => "快捷操作",
        "Close tab" => "关闭标签页",
        "Split side by side" => "左右分屏",
        "Split stacked" => "上下分屏",
        "Toggle inspector" => "切换检查器",
        "Synchronized input" => "同步输入",
        "Start logging" => "开始记录",
        "Stop logging" => "停止记录",
        "Quick commands (Ctrl+Shift+S)" => "快捷命令 (Ctrl+Shift+S)",
        "Settings (Ctrl+Shift+O)" => "设置 (Ctrl+Shift+O)",
        "dshell {version} · native GPUI · Esc closes" => {
            "dshell {version} · 原生 GPUI · 按 Esc 关闭"
        }
        "Disconnected" => "未连接",
        "Connecting" => "连接中",
        "Connected" => "已连接",
        "Disconnecting" => "断开中",
        "Disconnect" => "断开连接",
        "Copy selection" => "复制选中内容",
        "Paste" => "粘贴",
        "Send file" => "发送文件",
        "Show sidebar" => "显示侧栏",
        "Hide sidebar" => "隐藏侧栏",
        "Open settings" => "打开设置",
        "Open files" => "打开文件",
        "Open tunnels" => "打开隧道",
        "Open snippets" => "打开片段",
        "Open keys" => "打开密钥",
        "Expand sidebar" => "展开侧栏",
        "Collapse sidebar (Ctrl+Shift+B)" => "折叠侧栏 (Ctrl+Shift+B)",
        "Files (Ctrl+Shift+F)" => "文件 (Ctrl+Shift+F)",
        "Find (Ctrl+F)" => "查找 (Ctrl+F)",
        "Menu" => "菜单",
        "Path for the new private key" => "新私钥路径",
        "Optional passphrase" => "可选口令",
        "Production" => "生产环境",
        "Ungrouped" => "未分组",
        "Saved jump host name" => "已保存的跳板机名称",
        "Leave empty for a direct connection." => "直接连接时留空。",
        "Snippet name" => "片段名称",
        "ERROR or timeout" => "ERROR 或 timeout",
        "Response" => "响应",
        "server.example.com" => "server.example.com",
        "user" => "用户",
        "Ready" => "就绪",
        "SSH workspace" => "SSH 工作区",
        "No active terminal" => "没有活动终端",
        "Create a session or open a saved host from the sidebar." => {
            "新建会话或从侧栏打开已保存的主机。"
        }
        "Host key" => "主机密钥",
        "Fingerprint" => "指纹",
        "Local" => "本地",
        "Remote" => "远程",
        "Dynamic" => "动态",
        "Listen host" => "监听主机",
        "Listen port (0 = auto)" => "监听端口（0 = 自动）",
        "Target host" => "目标主机",
        "Target port" => "目标端口",
        "SOCKS5 clients choose the target host and port." => {
            "SOCKS5 客户端自行选择目标主机和端口。"
        }
        "connection" => "个连接",
        "connections" => "个连接",
        "No highlighted keywords" => "没有高亮关键词",
        "Add literal words such as error, warning or timeout." => {
            "添加 error、warning 或 timeout 等字面量词。"
        }
        "Remove keyword" => "删除关键词",
        "Disable keyword highlighting" => "停用关键词高亮",
        "Enable keyword highlighting" => "启用关键词高亮",
        "Close keyword highlights" => "关闭关键词高亮",
        "Add keyword" => "添加关键词",
        "Literal matches are highlighted in the terminal." => "终端中的字面量匹配会高亮显示。",
        "Highlighting is off until you enable it." => "启用前不会进行高亮。",
        "Keyword highlighting enabled." => "关键词高亮已启用。",
        "Keyword highlighting disabled." => "关键词高亮已停用。",
        "Enter a keyword to highlight." => "请输入要高亮的关键词。",
        "Keyword highlights are limited to {count} entries." => "关键词高亮最多支持 {count} 条。",
        "Keyword {keyword} is already listed." => "关键词 {keyword} 已存在。",
        "Added {keyword}." => "已添加 {keyword}。",
        "Removed {keyword}." => "已删除 {keyword}。",
        "No quick commands" => "没有快捷命令",
        "Create a command tile for the active SSH session." => "为当前 SSH 会话创建一个命令磁贴。",
        "Edit quick command" => "编辑快捷命令",
        "Delete quick command" => "删除快捷命令",
        "New quick command" => "新建快捷命令",
        "Close quick commands" => "关闭快捷命令",
        "Name" => "名称",
        "Command" => "命令",
        "Save command" => "保存命令",
        "Send to {title}" => "发送到{title}",
        "Open a connected session to run a command." => "请打开已连接的会话以运行命令。",
        "Broadcast targets" => "广播目标",
        "Send to {count} selected" => "发送到已选择的 {count} 个会话",
        "There is no active session." => "没有活动会话。",
        "Sent {name} to the active session." => "已将{name}发送到当前会话。",
        "The active session cannot accept commands." => "当前会话无法接收命令。",
        "Enter a command before broadcasting." => "广播前请输入命令。",
        "Select at least one connected target session." => "至少选择一个已连接的目标会话。",
        "No selected session could accept the command." => "没有已选择的会话可以接收命令。",
        "Sent to: {targets}." => "已发送到：{targets}。",
        "Could not send the command: {error}" => "无法发送命令：{error}",
        "{kind} target already exists: {target}" => "{kind} 目标已存在：{target}",
        "Editing {name}." => "正在编辑{name}。",
        "Snippet name and command are required." => "片段名称和命令不能为空。",
        "Command snippet storage is unavailable." => "快捷命令存储不可用。",
        "A snippet named {name} already exists." => "已存在名为{name}的片段。",
        "Cannot have more than {count} snippets." => "片段数量不能超过 {count} 个。",
        "History" => "历史",
        "History {current}/{total}" => "历史 {current}/{total}",
        "Compose · Ctrl+Enter sends · Esc closes" => "编辑命令 · Ctrl+Enter 发送 · Esc 关闭",
        "Send" => "发送",
        "Close compose" => "关闭命令编辑器",
        "The server requested {count} keyboard-interactive prompts; at most {max} are supported." => {
            "服务器请求了 {count} 个键盘交互提示，最多支持 {max} 个。"
        }
        "Waiting for keyboard-interactive input..." => "等待键盘交互输入……",
        "Keyboard-interactive authentication: {name}" => "键盘交互认证：{name}",
        "Submitting authentication response..." => "正在提交认证响应……",
        "Keyboard-interactive authentication" => "键盘交互认证",
        "Responses are sent only to the SSH server and are not stored." => {
            "响应只发送到 SSH 服务器，不会保存。"
        }
        "Continue" => "继续",
        "Session tab actions" => "标签页操作",
        "Close split" => "关闭分屏",
        "split" => "分屏",
        "Sync" => "同步",
        "target" => "目标",
        "Connect at least one session before enabling synchronized input." => {
            "请先连接至少一个会话，再启用同步输入。"
        }
        "Syncing" => "同步中",
        "Add" => "添加",
        "{count} target(s) selected" => "已选择 {count} 个目标",
        "Keyboard, paste and compose are mirrored; mouse, scrolling and resize stay per-session." => {
            "键盘、粘贴和命令编辑会镜像到目标会话；鼠标、滚动和尺寸调整仍按会话独立处理。"
        }
        "Clear" => "清空",
        "Done" => "完成",
        "Close tab actions" => "关闭标签操作",
        "Close other tabs" => "关闭其他标签",
        "Close tabs to the right" => "关闭右侧标签",
        "Unsaved" => "未保存",
        "None" => "无",
        "Pending" => "等待中",
        "Not reported" => "未报告",
        "Off" => "关闭",
        "Recording" => "记录中",
        "No active transfer." => "没有活动传输。",
        "Close files" => "关闭文件",
        "No forwarding rules." => "没有转发规则。",
        "Stop rule" => "停止规则",
        "active" => "活动",
        "Start rule" => "启动规则",
        "Stopped" => "已停止",
        "Close tunnels" => "关闭隧道",
        "Manage tunnels" => "管理隧道",
        "Auth" => "认证",
        "State" => "状态",
        "Terminal" => "终端",
        "Profile" => "资料",
        "Keep-alive" => "保活",
        "Logging" => "日志记录",
        "File" => "文件",
        "New logs use timestamped records with a replay timeline. Appending to an existing legacy raw log keeps the raw format. Logs may contain sensitive terminal output." => {
            "新日志使用带时间戳的记录并支持回放时间轴。追加到旧版原始日志时会保留原始格式。日志可能包含敏感的终端输出。"
        }
        "Open replay..." => "打开回放……",
        "Close inspector" => "关闭检查器",
        "Cancel connection" => "取消连接",
        _ => return Cow::Borrowed(key),
    };
    Cow::Borrowed(translated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_keeps_stable_keys() {
        assert_eq!(translate(Language::English, "Settings"), "Settings");
    }

    #[test]
    fn simplified_chinese_translates_shell_labels_and_preserves_names() {
        assert_eq!(translate(Language::SimplifiedChinese, "Settings"), "设置");
        assert_eq!(translate(Language::SimplifiedChinese, "Generate"), "生成");
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Compose a multi-line command, then press Ctrl+Enter"
            ),
            "编辑多行命令，然后按 Ctrl+Enter"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Replay log (Ctrl+Shift+R)"),
            "回放日志 (Ctrl+Shift+R)"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Split with another session"),
            "与另一会话分屏"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Disconnect"),
            "断开连接"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Production"),
            "生产环境"
        );
        assert_eq!(translate(Language::SimplifiedChinese, "name"), "名称");
        assert_eq!(
            translate(Language::SimplifiedChinese, "Unix seconds"),
            "Unix 秒"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Passwords are remembered only in memory for this run and are never saved with host profiles."
            ),
            "密码只会在本次运行的内存中记住，不会随主机资料保存。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Hidden keyboard-interactive prompts are answered with this password. One-time codes and visible prompts are not supported yet."
            ),
            "隐藏的键盘交互提示会使用此密码回答。一次性验证码和明文提示暂不支持。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Uses the Windows OpenSSH agent named pipe or Pageant. Load the key into the agent before connecting."
            ),
            "使用 Windows OpenSSH agent 命名管道或 Pageant。连接前请先将密钥加载到 agent。"
        );
        assert_eq!(translate(Language::SimplifiedChinese, "Triggers"), "触发器");
        assert_eq!(
            translate(Language::SimplifiedChinese, "Change key passphrase"),
            "修改密钥口令"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Edit metadata"),
            "编辑属性"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Permissions (octal)"),
            "权限（八进制）"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Trigger engine enabled."),
            "触发器引擎已启用。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "preset command, for example: systemctl status ssh"
            ),
            "预设命令，例如：systemctl status ssh"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Private key path"),
            "私钥路径"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Creates an OpenSSH private key and a .pub file. Existing files are never overwritten. The passphrase is not stored; Unix key files use mode 0600."
            ),
            "创建 OpenSSH 私钥和 .pub 公钥文件。不会覆盖已有文件。口令不会保存；Unix 私钥文件权限为 0600。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Loaded {name}. Enter the credentials to connect."
            ),
            "已载入{name}。请输入凭据以连接。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Loaded {name}. Passwords are never saved; enter the password to connect."
            ),
            "已载入{name}。密码不会保存，请输入密码后连接。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Enter credentials for {name}. They will be remembered in memory for this run."
            ),
            "请输入{name}的认证信息。本次运行期间会在内存中记住。"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Authenticate {name}"),
            "认证{name}"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Enter the authentication information and confirm to connect."
            ),
            "请输入认证信息，然后确认连接。"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "Uses the SSH agent; no password is required."
            ),
            "使用 SSH agent，无需密码。"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Connect to host"),
            "连接主机"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "SSH password authentication was rejected"
            ),
            "SSH 密码认证被拒绝。"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Export host profiles"),
            "导出主机资料"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Import dshell host profiles"),
            "导入 dshell 主机资料"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "dshell {version} · native GPUI · Esc closes"
            ),
            "dshell {version} · 原生 GPUI · 按 Esc 关闭"
        );
        assert_eq!(translate(Language::SimplifiedChinese, "my-host"), "my-host");
    }

    #[test]
    fn simplified_chinese_covers_recent_panels_and_validation() {
        assert_eq!(
            translate(Language::SimplifiedChinese, "Add tunnel"),
            "添加隧道"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Quick commands"),
            "快捷命令"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Synchronized input"),
            "同步输入"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "A password is required for password authentication."
            ),
            "密码认证需要输入密码。"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Inspector"),
            "检查器"
        );
        assert_eq!(translate(Language::SimplifiedChinese, "Processes"), "进程");
        assert_eq!(translate(Language::SimplifiedChinese, "Services"), "服务");
        assert_eq!(
            translate(Language::SimplifiedChinese, "Server alerts"),
            "服务器告警"
        );
        assert_eq!(
            translate(Language::SimplifiedChinese, "Collecting process list..."),
            "正在采集进程列表……"
        );
        assert_eq!(
            translate(
                Language::SimplifiedChinese,
                "The remote host returned too many process rows."
            ),
            "远端主机返回了过多进程行。"
        );
        assert_eq!(translate(Language::SimplifiedChinese, "TABS"), "标签页");
    }
}
