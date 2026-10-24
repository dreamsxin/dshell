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
        "Search terminal" => "搜索终端",
        "Search saved hosts" => "搜索已保存主机",
        "New session" => "新建会话",
        "Close session" => "关闭会话",
        "Toggle files" => "切换文件面板",
        "Toggle snippets" => "切换片段面板",
        "Toggle settings" => "切换设置",
        "Toggle sidebar" => "切换侧栏",
        "Toggle compose" => "切换命令编辑器",
        "New session (Ctrl+Shift+T)" => "新建会话 (Ctrl+Shift+T)",
        "New SSH Session" => "新建 SSH 会话",
        "Session Properties" => "会话属性",
        "Session properties" => "会话属性",
        "No active sessions" => "没有活动会话",
        "Connection" => "连接",
        "Authentication" => "认证",
        "Security" => "安全",
        "Session name" => "会话名称",
        "Host" => "主机",
        "Port" => "端口",
        "Group" => "分组",
        "Favorite" => "已收藏",
        "Not favorite" => "未收藏",
        "Jump host" => "跳板机",
        "Keep-alive (seconds, 0 = off)" => "保活（秒，0 = 关闭）",
        "Protocol" => "协议",
        "Username" => "用户名",
        "Authentication method" => "认证方式",
        "Password" => "密码",
        "Public key" => "公钥",
        "Keyboard-interactive" => "键盘交互",
        "Keyboard interactive" => "键盘交互",
        "Identity file" => "身份文件",
        "Key passphrase" => "密钥口令",
        "Agent" => "代理",
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
        "Cancel transfer" => "取消传输",
        "Keep existing" => "保留现有文件",
        "Resume" => "续传",
        "Overwrite" => "覆盖",
        "Add tunnel" => "添加隧道",
        "Stop" => "停止",
        "Start" => "启动",
        "Open session log" => "打开会话日志",
        "Import OpenSSH config" => "导入 OpenSSH 配置",
        "Upload file" => "上传文件",
        "Choose download folder" => "选择下载文件夹",
        "Upload folder" => "上传文件夹",
        "Authentication templates" => "认证模板",
        "Template name" => "模板名称",
        "Save template" => "保存模板",
        "Algorithm" => "算法",
        "Comment" => "备注",
        "Generate" => "生成",
        "User keys" => "用户密钥",
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
        "dshell 0.1.0 · native GPUI · Esc closes" => "dshell 0.1.0 · 原生 GPUI · 按 Esc 关闭",
        "Disconnected" => "未连接",
        "Connecting" => "连接中",
        "Connected" => "已连接",
        "Disconnecting" => "断开中",
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
        assert_eq!(translate(Language::SimplifiedChinese, "my-host"), "my-host");
    }
}
