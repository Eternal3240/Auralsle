//! 手机通知 JSON 数据模型与容错解析（对应 `ShowToast Data received:{json}`）。

use serde::{Deserialize, Deserializer};

fn de_flex_bool<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    use serde::de::Error;
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Bool(b) => Ok(b),
        serde_json::Value::String(s) => Ok(matches!(s.trim().to_ascii_lowercase().as_str(), "true" | "1" | "yes")),
        serde_json::Value::Number(n) => Ok(n.as_i64().unwrap_or(0) != 0),
        serde_json::Value::Null => Ok(false),
        other => Err(D::Error::custom(format!("unexpected bool: {other}"))),
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Notification {
    pub contracted_child: ContractedChild,
    pub expanded_child: ExpandedChild,
    pub group_state: GroupState,
    pub header_view: HeaderView,
    pub importance_level: i32,
    #[serde(deserialize_with = "de_flex_bool")]
    pub is_banner_show: bool,
    #[serde(deserialize_with = "de_flex_bool")]
    pub is_custom_content: bool,
    #[serde(deserialize_with = "de_flex_bool")]
    pub is_verification: bool,
    pub key: String,
    #[serde(deserialize_with = "de_flex_bool")]
    pub on_going: bool,
    #[serde(deserialize_with = "de_flex_bool")]
    pub pinned: bool,
    pub pkg_name: String,
    pub user_identifier: i32,
    #[serde(skip)]
    pub raw: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ContractedChild {
    pub contracted_content: String,
    pub contracted_title: String,
    /// base64（手机侧 PNG 每 76 字符换行，解码前跳过空白）
    pub large_icon_byte_str: String,
    pub progress: Progress,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Progress {
    pub indeterminate: bool,
    pub max: u32,
    pub value: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ExpandedChild {
    pub big_picture_style_state: BigPictureStyle,
    pub big_text_style_state: BigTextStyle,
    pub icdf_actions: Vec<serde_json::Value>,
    pub inbox_style_state: InboxStyle,
    pub messaging_style_state: MessagingStyle,
    pub notification_style: String,
    pub reply_content: Vec<serde_json::Value>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BigPictureStyle {
    pub big_large_icon_byte_str: String,
    pub big_picture_byte_str: String,
    pub expanded_summary_text: String,
    pub expanded_title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BigTextStyle {
    /// 上游日志字段名为 `bitText`
    pub bit_text: String,
    pub expanded_summary_text: String,
    pub expanded_title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct InboxStyle {
    pub expanded_summary_text: String,
    pub expanded_title: String,
    pub inbox_lines: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct MessagingStyle {
    pub conversation_title: String,
    pub historic_messages: Vec<serde_json::Value>,
    pub messages: Vec<serde_json::Value>,
    pub person: Person,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Person {
    pub icon_byte_str: String,
    pub name: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupState {
    pub children_in_group: bool,
    pub group_line: GroupLine,
    pub group_name: String,
    pub group_summary: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GroupLine {
    pub text: String,
    pub title: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct HeaderView {
    pub header_text: String,
    pub header_text_secondary: String,
    pub label: String,
    pub label_ext: String,
    pub show_when: bool,
    pub small_icon_byte_str: String,
    pub when_time: i64,
}

impl Notification {
    pub fn title(&self) -> String {
        let t = self.contracted_child.contracted_title.trim();
        if !t.is_empty() {
            return t.to_string();
        }
        let h = self.header_view.header_text.trim();
        if !h.is_empty() {
            return h.to_string();
        }
        self.app_display_name()
    }

    pub fn body(&self) -> String {
        let style = self.expanded_child.notification_style.as_str();
        let fallback = self.contracted_child.contracted_content.trim();
        match style {
            "BIG_TEXT" => {
                let t = self.expanded_child.big_text_style_state.bit_text.trim();
                if t.is_empty() { fallback.to_string() } else { t.to_string() }
            }
            "INBOX" => {
                let lines = &self.expanded_child.inbox_style_state.inbox_lines;
                if lines.is_empty() {
                    fallback.to_string()
                } else {
                    lines
                        .iter()
                        .take(4)
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join("\n")
                }
            }
            "MESSAGING" => {
                for m in self.expanded_child.messaging_style_state.messages.iter().rev() {
                    for key in ["text", "message", "content"] {
                        if let Some(s) = m.get(key).and_then(|v| v.as_str()) {
                            if !s.trim().is_empty() {
                                return s.to_string();
                            }
                        }
                    }
                }
                fallback.to_string()
            }
            _ => fallback.to_string(),
        }
    }

    pub fn attribution(&self) -> String {
        let l = self.header_view.label_ext.trim();
        if l.is_empty() {
            self.header_view.label.trim().to_string()
        } else {
            l.to_string()
        }
    }

    /// 来源应用可读名称（优先已知常见包名，其次 header_view.label / 包名末段）。
    pub fn app_display_name(&self) -> String {
        let pkg = self.pkg_name.trim().to_ascii_lowercase();
        let known = match pkg.as_str() {
            "com.tencent.mm" => Some("微信"),
            "com.tencent.mobileqq" | "com.tencent.tim" | "com.tencent.qqlite" => Some("QQ"),
            "com.tencent.wework" => Some("企业微信"),
            "com.ss.android.ugc.aweme" | "com.ss.android.ugc.aweme.lite" => Some("抖音"),
            "com.ss.android.lark" | "com.larksuite.suite" => Some("飞书"),
            "com.alibaba.android.rimet" => Some("钉钉"),
            "com.eg.android.alipaygphone" => Some("支付宝"),
            "com.taobao.taobao" => Some("淘宝"),
            "com.jingdong.app.mall" => Some("京东"),
            "com.xunmeng.pinduoduo" => Some("拼多多"),
            "com.sankuai.meituan" => Some("美团"),
            "me.ele" => Some("饿了么"),
            "com.xingin.xhs" => Some("小红书"),
            "tv.danmaku.bili" => Some("哔哩哔哩"),
            "com.sina.weibo" => Some("微博"),
            "com.zhihu.android" => Some("知乎"),
            "com.twitter.android" => Some("X"),
            "org.telegram.messenger" => Some("Telegram"),
            "com.whatsapp" => Some("WhatsApp"),
            "com.android.mms" | "com.oplus.mms" => Some("短信"),
            "com.android.incallui" | "com.android.dialer" | "com.oplus.dialer" => Some("电话"),
            "com.netease.cloudmusic" => Some("网易云音乐"),
            "com.tencent.qqmusic" => Some("QQ音乐"),
            _ => None,
        };
        if let Some(name) = known {
            return name.to_owned();
        }
        let hv = self.header_view.label.trim();
        if !hv.is_empty() {
            return hv.to_owned();
        }
        if let Some(ext) = self.header_view.label_ext.split('|').next() {
            let trimmed = ext.trim();
            if !trimmed.is_empty() && !trimmed.starts_with("手机") {
                return trimmed.to_owned();
            }
        }
        self.pkg_name
            .rsplit('.')
            .find(|s| !s.is_empty() && *s != "android")
            .unwrap_or(self.pkg_name.as_str())
            .to_owned()
    }

    pub fn is_wechat(&self) -> bool {
        self.pkg_name.trim().eq_ignore_ascii_case("com.tencent.mm")
    }

    pub fn is_qq(&self) -> bool {
        matches!(
            self.pkg_name.trim().to_ascii_lowercase().as_str(),
            "com.tencent.mobileqq" | "com.tencent.tim" | "com.tencent.qqlite"
        )
    }

    /// 仅读取来源应用小图标（`small_icon_byte_str`），不缓存联系人头像等个人图片（`large_icon_byte_str`）。
    pub fn icon_b64(&self) -> Option<&str> {
        let s = self.header_view.small_icon_byte_str.trim();
        if s.is_empty() {
            None
        } else {
            Some(self.header_view.small_icon_byte_str.as_str())
        }
    }

    pub fn hero_b64(&self) -> Option<&str> {
        if self.expanded_child.notification_style != "BIG_PICTURE" {
            return None;
        }
        let s = self
            .expanded_child
            .big_picture_style_state
            .big_picture_byte_str
            .trim();
        if s.is_empty() { None } else { Some(s) }
    }

    pub fn progress(&self) -> Option<Progress> {
        let p = &self.contracted_child.progress;
        if p.indeterminate || p.max > 0 { Some(p.clone()) } else { None }
    }

    pub fn dedupe_key(&self) -> String {
        let k = self.key.trim();
        if !k.is_empty() {
            return k.to_string();
        }
        format!(
            "{}|{}|{}|u{}",
            self.pkg_name,
            self.title(),
            self.header_view.when_time,
            self.user_identifier
        )
    }

    pub fn from_log_line(line: &str) -> Option<Notification> {
        let idx = line.find("ShowToast Data received:")?;
        let json = line[idx + "ShowToast Data received:".len()..].trim();
        let start = json.find('{')?;
        let json = json[start..].trim();
        let mut n: Notification = serde_json::from_str(json).ok()?;
        n.raw = json.to_string();
        Some(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_log_line_and_flex_bools() {
        let line = r#"2026-11-21 14:31:17,210 [1] INFO toastNotification.NotificationHelp [(null)] - ShowToast Data received:{"contractedChild":{"contractedTitle":"验证码","contractedContent":"您的验证码为 482913","largeIconByteStr":"iVBORw0K\nGg=="},"expandedChild":{"notificationStyle":"BIG_TEXT","bigTextStyleState":{"bitText":"【示例】您的验证码为 482913，5分钟内有效"}},"headerView":{"labelExt":"手机 柒 | 短信","smallIconByteStr":"iVBORw0K\nGg=="},"isVerification":"true","onGoing":0,"key":"abc-123","pkgName":"com.android.mms"}"#;
        let n = Notification::from_log_line(line).unwrap();
        assert_eq!(n.app_display_name(), "短信");
        assert_eq!(n.title(), "验证码");
        assert!(n.body().contains("482913"));
        assert!(n.is_verification);
        assert!(!n.on_going);
        assert_eq!(n.dedupe_key(), "abc-123");
        assert!(n.icon_b64().is_some());
    }

    #[test]
    fn does_not_use_large_contact_avatar_as_app_icon() {
        let line = r#"2026-11-21 14:31:17,210 [1] INFO toastNotification.NotificationHelp [(null)] - ShowToast Data received:{"contractedChild":{"contractedTitle":"好友","contractedContent":"hi","largeIconByteStr":"AAAA"},"headerView":{"label":"微信"},"pkgName":"com.tencent.mm"}"#;
        let n = Notification::from_log_line(line).unwrap();
        assert!(n.icon_b64().is_none(), "不应把联系人头像当成应用图标缓存");
    }

    #[test]
    fn parses_clone_notification_user_999() {
        let line = r#"2026-11-21 15:50:58,001 [1] INFO toastNotification.NotificationHelp [(null)] - ShowToast Data received:{"contractedChild":{"contractedTitle":"分身测试","contractedContent":"来自应用分身的一条消息"},"expandedChild":{"notificationStyle":"BIG_TEXT","bigTextStyleState":{"bitText":"来自应用分身的一条消息"}},"headerView":{"labelExt":"微信 | 分身"},"key":"999|com.tencent.mm|838453658|null|99910682","pkgName":"com.tencent.mm","userIdentifier":999}"#;
        let n = Notification::from_log_line(line).unwrap();
        assert_eq!(n.user_identifier, 999);
        assert!(n.is_wechat());
        assert_eq!(n.dedupe_key(), "999|com.tencent.mm|838453658|null|99910682");
        assert_eq!(n.title(), "分身测试");
        assert_eq!(n.body(), "来自应用分身的一条消息");
    }
}
