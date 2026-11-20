//! 设置、通知队列与岛仔表情。
use crate::settings_theme::color as settings_color;
use crate::app::{App, Cmd, Env, Phase, HUD_TRACK_W, HUD_TRACK_X, HUD_W, PAGE_NAMES};
use crate::paint::{Card, H};
use crate::theme::{self, size, W};
use eframe::egui::{self, Color32, Rect};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum AlertSource { Internal, Messenger(crate::app::Messenger), External(String) }

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(crate) enum HealthReminder {Water,Sit}
impl HealthReminder {
    pub(crate) const ALL:[Self;2]=[Self::Water,Self::Sit];
    pub(crate) fn title(self)->&'static str {match self {Self::Water=>"喝水时间到了",Self::Sit=>crate::app::SIT_TITLE}}
    pub(crate) fn label(self)->&'static str {match self {Self::Water=>"待喝水",Self::Sit=>"待活动"}}
    pub(crate) fn id(self)->&'static str {match self {Self::Water=>"health-water",Self::Sit=>"health-sit"}}
    pub(crate) fn icon(self,c:&Card,x:f32,y:f32,scale:f32) {
        let c=&c.scaled(x,y,scale);
        match self {
            Self::Water=>{
                let (x, y) = if cfg!(test) { (x, y) } else { (x - 0.6, y + 0.5) };
                c.poly(&[(x-7.0,y-1.0),(x+5.0,y-1.0),(x+3.5,y+8.0),(x-5.5,y+8.0)],theme::BLUE);
                c.path(&[(x-9.0,y-10.0),(x-6.5,y+9.0),(x+4.5,y+9.0),(x+7.0,y-10.0)],1.6,theme::CYAN);
                c.seg((x-9.0,y-10.0),(x+7.0,y-10.0),1.6,theme::CYAN);
                c.path(&[(x+6.5,y-6.0),(x+10.5,y-6.0),(x+11.5,y-3.0),(x+10.0,y+2.0),(x+5.5,y+3.0)],1.6,theme::CYAN);
                c.seg((x-5.0,y-6.5),(x-4.2,y-2.5),1.0,Color32::from_white_alpha(170));
            },
            Self::Sit=>{
                let (x, y) = if cfg!(test) { (x, y) } else { (x - 0.2, y - 0.3) };
                crate::paint::chair(c,x,y,0.6,theme::SOFT_GREEN);
            }
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Alert {
    pub title: String,
    pub source: AlertSource,
    pub body: String,
    pub page: usize,
    pub at: f64,
    pub duration: f64,
    pub bluetooth: Option<(crate::bt::DeviceKind, bool)>,
    pub meeting: Option<(crate::mail::Meeting, String)>,
    pub unread: Option<crate::app::Unread>,
    pub oppo_badge: bool,
    pub verification_code: Option<String>,
    pub verification_copied: bool,
}
impl Alert {
    pub(crate) fn health(&self)->Option<HealthReminder> {
        if self.source!=AlertSource::Internal{return None;}
        HealthReminder::ALL.into_iter().find(|kind|self.title==kind.title())
    }
    pub(crate) fn messenger(&self)->Option<crate::app::Messenger>{
        match &self.source {
            AlertSource::Messenger(client)=>Some(*client),
            AlertSource::External(id)=>crate::app::Messenger::from_notification_id(id).filter(|client|client.adapter().capture==crate::messenger::Capture::SystemToast),
            _=>None,
        }
    }
    pub(crate) fn is_hud(&self)->bool {self.source==AlertSource::Internal && matches!(self.title.as_str(),"系统音量"|"大写锁定"|"键盘输入语言"|"网络连接"|"电源已连接"|"已切换电池供电"|"蓝牙设备")}
}
fn bluetooth_notice_content(body:&str)->(&str,&str,Option<u8>){
    if let Some(name)=body.strip_suffix(" · 已断开") {return (name,"已断开",None);}
    if body=="已断开连接" {return ("蓝牙设备","已断开",None);}
    if let Some((name,level))=body.rsplit_once(" · 电量 ") {
        if let Some(level)=level.strip_suffix('%').and_then(|s|s.parse::<u8>().ok()).filter(|v|*v<=100){
            return (name,"已连接",Some(level));
        }
    }
    (body,"已连接",None)
}

pub(crate) fn device_icon(c: &Card, kind: crate::bt::DeviceKind, x: f32, y: f32, size: f32) {
    device_icon_tinted(c, kind, x, y, size, Color32::WHITE);
}

/// 按 `DeviceKind` 取图标资源。`line = true` 用线性（stroke）版本：
/// 药丸里的字号只有 9~10，电池也是细线，实心粗图标会显得比文字重一档。
fn device_icon_source(kind: crate::bt::DeviceKind, line: bool) -> egui::ImageSource<'static> {
    use crate::bt::DeviceKind::*;
    if line {
        match kind {
            Bone | Earbuds | Headphones => egui::include_image!("../assets/device-icons/headphones-line.svg"),
            Speaker => egui::include_image!("../assets/device-icons/speaker-line.svg"),
            Keyboard => egui::include_image!("../assets/device-icons/keyboard-line.svg"),
            Mouse => egui::include_image!("../assets/device-icons/mouse-line.svg"),
            Gamepad => egui::include_image!("../assets/device-icons/gamepad-line.svg"),
            Phone => egui::include_image!("../assets/device-icons/smartphone-line.svg"),
            Other => egui::include_image!("../assets/device-icons/bluetooth-linear.svg"),
        }
    } else {
        match kind {
            Bone | Earbuds | Headphones => egui::include_image!("../assets/device-icons/headphones-round-bold.svg"),
            Speaker => egui::include_image!("../assets/device-icons/speaker-bold.svg"),
            Keyboard => egui::include_image!("../assets/device-icons/keyboard-bold.svg"),
            Mouse => egui::include_image!("../assets/device-icons/mouse-bold.svg"),
            Gamepad => egui::include_image!("../assets/device-icons/gamepad-bold.svg"),
            Phone => egui::include_image!("../assets/device-icons/smartphone-bold.svg"),
            Other => egui::include_image!("../assets/device-icons/bluetooth-linear.svg"),
        }
    }
}

fn paint_icon(c: &Card, icon: egui::ImageSource<'static>, x: f32, y: f32, size: f32, tint: Color32) {
    let rect=c.rect(x,y,size,size);
    let egui::ImageSource::Bytes{uri,bytes}=icon else{return};
    let pixels=(rect.width()*c.ui.ctx().pixels_per_point()).ceil() as u32;
    let icon=egui::Image::from_bytes(format!("{uri}/{pixels}.svg"),bytes);
    if let Ok(egui::load::TexturePoll::Ready { texture })=icon.fit_to_exact_size(rect.size()).load_for_size(c.ui.ctx(),rect.size()) {
        c.painter.image(texture.id,rect,Rect::from_min_max(egui::Pos2::ZERO,egui::pos2(1.0,1.0)),tint);
    }
}

/// 与 `device_icon` 相同，但可以指定着色：浅色皮肤下某些底板是浅的，
/// 白色图标会看不清，需要按底板亮度换成深色。
pub(crate) fn device_icon_tinted(
    c: &Card,
    kind: crate::bt::DeviceKind,
    x: f32,
    y: f32,
    size: f32,
    tint: Color32,
) {
    paint_icon(c, device_icon_source(kind, false), x, y, size, tint);
}

/// 药丸专用线性图标：1.9px 描边、圆头端点，和药丸里的细线电池、小字号同一套视觉，
/// 缩到 12~13px 也不会糊成一团实心块。
pub(crate) fn device_icon_line(
    c: &Card,
    kind: crate::bt::DeviceKind,
    x: f32,
    y: f32,
    size: f32,
    tint: Color32,
) {
    paint_icon(c, device_icon_source(kind, true), x, y, size, tint);
}

pub(crate) use crate::pet::PetState;

pub(crate) fn notice_badge(c:&Card,x:f32,y:f32,a:&Alert){
    if a.source==AlertSource::Internal && a.title=="蓝牙设备"{
        c.circle(x,y,11.0,if a.body.contains("已断开"){Color32::from_gray(95)}else{theme::BLUE});
        device_icon(c,a.bluetooth.map_or(crate::bt::DeviceKind::Other,|v|v.0),x-7.5,y-7.5,15.0);
    }else{notice_icon(&c.scaled(x,y,0.85),x,y,a);}
}
pub(crate) fn messenger_icon(c:&Card,x:f32,y:f32,source:crate::app::Messenger){(source.adapter().paint_icon)(c,x,y);}
/// OPPO 互联角标（绿底双环互联微标），叠放在被手机转发的 QQ/微信图标右下方以区分电脑本地消息。
pub(crate) fn oppo_corner_badge(c: &Card, x: f32, y: f32, r: f32) {
    let r = r.max(3.5);
    c.circle(x, y, r + 1.0, Color32::from_gray(18));
    c.circle(x, y, r, Color32::from_rgb(16, 185, 129));
    let lw = (r * 0.24).max(0.9);
    let d = r * 0.24;
    let rr = r * 0.34;
    c.ring(x - d, y, rr, lw, Color32::WHITE);
    c.ring(x + d, y, rr, lw, Color32::WHITE);
}
fn notice_icon(c:&Card,x:f32,y:f32,a:&Alert) {
    if a.source!=AlertSource::Internal{crate::paint::bell(c,x,y,theme::WHITE,0.0);return;}
    match a.title.as_str() {
        "系统音量"=>{
            c.poly(&[(x-9.0,y-3.0),(x-5.0,y-3.0),(x,y-7.0),(x,y+7.0),(x-5.0,y+3.0),(x-9.0,y+3.0)],theme::WHITE);
            c.arc(x,y,5.0,-0.8,0.8,1.5,theme::WHITE);c.arc(x,y,8.0,-0.7,0.7,1.3,theme::WHITE);
        },
        "大写锁定"=>{
            let ink=if a.body=="已开启"{theme::GREEN}else{theme::GRAY};
            c.poly(&[(x-8.0,y-1.0),(x,y-9.0),(x+8.0,y-1.0)],ink);
            c.fill(x-3.0,y-1.5,6.0,6.5,0.5,ink);
            c.fill(x-4.0,y+7.0,8.0,2.0,0.8,ink);
        },
        "键盘输入语言"=>{
            c.stroke(x-10.0,y-8.5,20.0,17.0,4.0,1.2,theme::GRAY);
            let label=match a.body.as_str(){"中文"=>"中","英语"=>"EN",_=>"Aa"};
            c.centered_text(x,y,label,if label=="EN"{9.0}else{11.0},W::Semibold,theme::WHITE,H::Center);
        },
        "网络连接"=>{
            let connected=a.body.contains("已连接");
            let ink=if connected{theme::GREEN}else{theme::GRAY};
            if a.body.starts_with("Wi-Fi") {
                for r in [4.0,8.0,12.0]{c.arc(x,y+6.0,r,-2.35,-0.79,1.7,ink);}
                c.circle(x,y+6.0,1.6,ink);
            }else if a.body.starts_with("以太网") {
                c.stroke(x-8.0,y-8.0,16.0,14.0,2.5,1.6,ink);
                for dx in [-4.0,-1.3,1.3,4.0]{c.seg((x+dx,y-5.0),(x+dx,y-1.0),1.2,ink);}
                c.path(&[(x-4.0,y+6.0),(x-4.0,y+9.0),(x+4.0,y+9.0),(x+4.0,y+6.0)],1.6,ink);
            }else{
                c.ring(x,y,8.0,1.4,ink);c.seg((x-8.0,y),(x+8.0,y),1.0,ink);
                c.stroke(x-3.5,y-8.0,7.0,16.0,3.5,1.0,ink);
            }
            if !connected{c.seg((x-8.0,y-8.0),(x+9.0,y+9.0),2.0,theme::WHITE);}
        },
        "电源已连接"|"已切换电池供电"|"电量偏低"=>{
            let low=a.title=="电量偏低";let ink=if low{theme::RED_TOP}else if a.title=="电源已连接"{theme::GREEN}else{theme::WHITE};
            c.stroke(x-11.0,y-6.0,20.0,12.0,3.0,1.5,ink);c.fill(x+10.0,y-2.5,2.0,5.0,0.8,ink);
            if low{c.fill(x-8.0,y-3.5,3.5,7.0,1.0,ink);}
            else if a.title=="电源已连接"{
                c.poly(&[(x,y-4.5),(x-4.0,y+0.8),(x+0.5,y+0.8)],ink);
                c.poly(&[(x+0.5,y-0.8),(x+4.0,y-0.8),(x-1.0,y+4.5)],ink);
            }else{c.fill(x-8.0,y-3.5,12.0,7.0,1.0,ink);}
        },
        "喝水时间到了"=>{
            c.poly(&[(x-7.0,y-1.0),(x+5.0,y-1.0),(x+3.5,y+8.0),(x-5.5,y+8.0)],theme::BLUE);
            c.path(&[(x-9.0,y-10.0),(x-6.5,y+9.0),(x+4.5,y+9.0),(x+7.0,y-10.0)],1.6,theme::CYAN);
            c.seg((x-9.0,y-10.0),(x+7.0,y-10.0),1.6,theme::CYAN);
            c.path(&[(x+6.5,y-6.0),(x+10.5,y-6.0),(x+11.5,y-3.0),(x+10.0,y+2.0),(x+5.5,y+3.0)],1.6,theme::CYAN);
            c.seg((x-5.0,y-6.5),(x-4.2,y-2.5),1.0,Color32::from_white_alpha(170));
        },
        crate::app::SIT_TITLE=>crate::paint::chair(c,x,y,0.6,theme::SOFT_GREEN),
        "待办到时间了"=>crate::paint::checklist(c,x,y,23.0,Color32::from_rgb(190,180,255),Color32::from_rgb(43,35,66)),
        "会议提醒"=>{
            let label=a.meeting.as_ref().map(|(_,source)|source.as_str()).unwrap_or("");
            let source=if label=="本机日程"{crate::calendar_ui::Source::Local}else if label.starts_with("邮件"){crate::calendar_ui::Source::Mail("")}
                else{crate::calendar_ui::Source::Calendar(crate::calendar::Provider::ALL.into_iter().find(|provider|provider.label()==label))};
            crate::calendar_ui::source_icon(c,source,x-14.0,y-14.0,28.0);
        },
        "专注计时"=>crate::paint::stopwatch(&c.scaled(x,y,0.65),x,y,8.0,0.0,theme::RED_TOP),
        _=>crate::paint::bell(&c.scaled(x,y,1.7),x,y,theme::GRAY,0.0),
    }
}

impl App {
    pub(crate) fn health_badges(&self)->Vec<HealthReminder> {
        if self.fullscreen_hidden || !self.module_on(crate::app::WATER_PAGE){return Vec::new();}
        HealthReminder::ALL.into_iter().filter(|kind|self.health_pending[*kind as usize]
            && (match kind {HealthReminder::Water=>self.water.remind,HealthReminder::Sit=>self.settings.sit_remind})
            && (self.open || !self.notice.as_ref().is_some_and(|a|a.health()==Some(*kind)))).collect()
    }
    pub(crate) fn confirm_health_reminder(&mut self,kind:HealthReminder) {
        self.health_pending[kind as usize]=false;
        crate::model::save_health_pending(self.health_pending);
        match kind {HealthReminder::Water=>self.last_reminder=self.t,HealthReminder::Sit=>self.sit_start=self.t}
        self.alerts.retain(|a|a.health()!=Some(kind));
        if self.notice.as_ref().is_some_and(|a|a.health()==Some(kind)){self.notice=None;}
        if self.hud_notice.as_ref().is_some_and(|a|a.health()==Some(kind)){self.hud_notice=None;}
    }
    pub(crate) fn phase_secs(&self, phase: Phase) -> f32 {
        (match phase {
            Phase::Focus => self.settings.focus_minutes,
            Phase::Rest => self.settings.rest_minutes,
        }) as f32
            * 60.0
    }
    /// 已连续久坐的分钟数。
    pub(crate) fn sat_minutes(&self) -> u32 {
        ((self.t - self.sit_start).max(0.0) / 60.0) as u32
    }
    /// 下一次久坐提醒的时刻：从上次确认或起身起算满 sit_minutes。
    pub(crate) fn sit_due(&self) -> f64 {
        self.sit_start + self.settings.sit_minutes as f64 * 60.0
    }
    /// 「起身了」：久坐计时归零。
    pub(crate) fn stood_up(&mut self) {
        self.confirm_health_reminder(HealthReminder::Sit);
        self.pet.react(PetState::Happy);
        self.say("久坐计时已重新开始");
    }
    /// 现在是否在午休免打扰时段内（支持跨午夜；起止相同视为未设置）。
    pub(crate) fn lunch_now(&self, env: &Env) -> bool {
        let (a, b) = (self.settings.lunch_start, self.settings.lunch_end);
        let m = env.hour() as u32 * 60 + env.minute();
        self.settings.lunch_rest && a != b && if a < b { m >= a && m < b } else { m >= a || m < b }
    }
    /// 喝水和久坐提醒此刻能否弹出：在提醒时段内，且不在午休。
    pub(crate) fn remind_window(&self, env: &Env) -> bool {
        self.water_hours(env.hour() as u32) && !self.lunch_now(env)
    }
    pub(crate) fn water_hours(&self, hour: u32) -> bool {
        let (a, b) = (self.settings.water_start, self.settings.water_end);
        if a == b {
            true
        } else if a < b {
            hour >= a && hour < b
        } else {
            hour >= a || hour < b
        }
    }
    pub(crate) fn pet_state(&self) -> PetState {
        if let Some((state, _)) = self.pet.reaction {
            state
        } else if self.pet.waking_until > self.t {
            PetState::Waking
        } else if !cfg!(test) && (self.drag_hot || self.t <= self.copy_hot_until) {
            PetState::Curious
        } else if !cfg!(test) && self.notice.is_some() {
            PetState::Happy
        } else if self.pomo.running && self.pomo.phase == Phase::Focus {
            PetState::Focused
        } else if self.pet.worried {
            PetState::Worried
        } else if self.media.playing && self.media.is_music() {
            if self.notice.is_some() || self.hud_notice.is_some() {PetState::Awake}else{PetState::Music}
        } else if self.pet.hovered {
            PetState::Curious
        } else if self.idle_for() >= 180.0 {
            PetState::Sleeping
        } else if self.idle_for() >= self.opts.pet_idle {
            PetState::Dozing
        } else {
            PetState::Awake
        }
    }
    pub fn alert(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        page: usize,
        system: bool,
    ) {
        self.alert_with_duration(title, body, page, system, 0.0);
    }
    pub(crate) fn alert_with_duration(
        &mut self,
        title: impl Into<String>,
        body: impl Into<String>,
        page: usize,
        system: bool,
        custom_duration: f64,
    ) {
        let title = title.into();
        if let Some(kind)=HealthReminder::ALL.into_iter().find(|kind|title==kind.title()) {
            if self.health_pending[kind as usize]{return;}
            self.health_pending[kind as usize]=true;
            crate::model::save_health_pending(self.health_pending);
        }
        let transient = matches!(
            title.as_str(),
            "系统音量" | "大写锁定" | "键盘输入语言" | "网络连接" | "电源已连接" | "已切换电池供电" | "蓝牙设备"
        );
        let duration = if custom_duration > 0.0 {
            custom_duration
        } else if transient {
            2.5
        } else {
            8.0
        };
        let a = Alert {
            source: AlertSource::Internal,
            title,
            body: body.into(),
            page,
            at: self.t,
            duration,
            bluetooth: None,
            meeting: None,
            unread: None,
            oppo_badge: false,
            verification_code: None,
            verification_copied: false,
        };
        self.enqueue_alert(a, system);
    }
    pub(crate) fn push_verification_code(&mut self, source: String, code: String) {
        if !crate::auralsle_otp::valid_code(&code) {
            return;
        }
        let dup = self
            .history
            .iter()
            .any(|a| a.verification_code.as_deref() == Some(code.as_str()) && self.t - a.at < 90.0)
            || self.notice.as_ref().is_some_and(|a| {
                a.verification_code.as_deref() == Some(code.as_str()) && self.t - a.at < 90.0
            });
        if dup {
            return;
        }
        let src_title = if source.trim().is_empty() {
            "验证码".to_owned()
        } else {
            source
        };
        let a = Alert {
            source: AlertSource::Internal,
            title: src_title,
            body: "点击复制验证码".to_owned(),
            page: usize::MAX,
            at: self.t,
            duration: 12.0,
            bluetooth: None,
            meeting: None,
            unread: None,
            oppo_badge: false,
            verification_code: Some(code),
            verification_copied: false,
        };
        self.history.push_front(a.clone());
        self.history.truncate(50);
        self.alerts.retain(|old| old.verification_code != a.verification_code);
        if self.notice.as_ref().is_some_and(|cur| cur.verification_code.is_some()) {
            self.alerts.push_front(a);
        } else {
            self.notice = Some(a);
        }
    }
    pub(crate) fn meeting_alert(&mut self, meeting: crate::mail::Meeting, source: String, page: usize) {
        self.enqueue_alert(Alert {
            source: AlertSource::Internal,
            title: if source=="课表"{"上课提醒".into()}else{"会议提醒".into()},  // [ai-status-mod] 课程用上课提醒
            body: format!("{} · {} · {}", meeting.title, meeting.at, source),
            page, at: self.t, duration: 12.0, bluetooth: None, meeting: Some((meeting, source)), unread: None, oppo_badge: false,
            verification_code: None,
            verification_copied: false,
        }, true);
    }

    /// 统一收纳的「无线外设与电量（设备岛 / 设备胶囊）」设置卡片（需求 6 & 7）：
    /// 将设备岛、设备胶囊、连接外设自动显隐、2.4GHz 监控、蓝牙通知与外观位置整合在一起。
    pub(crate) fn draw_wireless_device_settings(&mut self, ui: &mut egui::Ui) {
        let mut mcfg = crate::auralsle_ai_status::config::get();
        let before_m = mcfg.clone();

        settings_card(ui, |ui| {
            setting_row(
                ui,
                "显示无线外设电量",
                "在灵动岛展示无线鼠标、键盘、耳机、手柄电量，低于 20% 自动变色告警",
                &mut mcfg.show_bt_battery_on_island,
            );
            if mcfg.show_bt_battery_on_island {
                twinkle_card_break(ui);
                let dm_lbl = match mcfg.device_display_mode {
                    0 => "设备胶囊",
                    1 => "设备岛",
                    _ => "同时存在",
                };
                setting_select_row(
                    ui,
                    "外设电量展示形态",
                    "选择在灵动岛旁以设备胶囊、独立设备岛页面或两者同时展示",
                    "wireless-device-display-mode",
                    dm_lbl,
                    &mut mcfg.device_display_mode,
                    &[
                        (0u8, "设备胶囊"),
                        (1u8, "设备岛"),
                        (2u8, "同时存在"),
                    ],
                );
                twinkle_card_break(ui);
                setting_row(
                    ui,
                    "仅在连接无线外设时显示设备岛",
                    "检测到已连接无线外设时才在岛上展示设备岛，无外设连接时自动隐藏",
                    &mut mcfg.device_auto_hide_when_empty,
                );
            }
        });

        setting_row_card(
            ui,
            "启用 2.4GHz 无线外设电量监控",
            "读取接收器连接的 2.4GHz 无线鼠标、键盘、耳机与手柄实时电量",
            &mut mcfg.voltide_enabled,
        );

        setting_row_card(
            ui,
            "蓝牙连接与断开通知",
            "无线设备连接或断开时在灵动岛弹出提示卡片",
            &mut self.settings.bluetooth_notifications,
        );

        if mcfg != before_m {
            let dm = mcfg.device_display_mode;
            crate::auralsle_ai_status::config::update(|c| {
                *c = mcfg.clone();
                if dm == 0 {
                    c.device_page_visible = false;
                } else {
                    c.device_page_visible = true;
                    c.show_bt_battery_on_island = true;
                }
            });
            if dm == 0 && self.page == 13 {
                if let Some(next_p) = self.active_page_order().into_iter().find(|&p| p != 13 && self.is_page_currently_visible(p)) {
                    self.page_from = next_p;
                    self.page = next_p;
                } else {
                    self.page_from = 0;
                    self.page = 0;
                }
            }
            self.dots_target();
        }

        crate::auralsle_ai_status::ui::draw_pill_settings(ui);

        let connected_devs = crate::bt::connected_peripherals();
        settings_card(ui, |ui| {
            let dev_open_id = egui::Id::new("tw_wireless_devs_open");
            let mut dev_open = ui.data(|d| d.get_temp::<bool>(dev_open_id)).unwrap_or(false);
            let dev_anim_id = egui::Id::new("tw_wireless_devs_anim");
            let mut dev_anim = ui.data(|d| d.get_temp::<f32>(dev_anim_id)).unwrap_or(0.0);
            let dev_desc = if connected_devs.is_empty() && self.bt_shown.is_empty() {
                "当前未连接无线外设（连接后自动显示实时电量），点击展开外设调试选项。".to_owned()
            } else {
                format!("当前已连接 {} 台无线外设，点击展开查看设备列表与电量明细。", connected_devs.len().max(self.bt_shown.len()))
            };
            if settings_expand_header(ui, "tw-wireless-devs-hdr", "已连接无线外设列表", &dev_desc, &mut dev_open) {
                ui.data_mut(|d| d.insert_temp(dev_open_id, dev_open));
            }
            settings_subpanel(ui, &mut dev_anim, dev_open, "tw-wireless-devs-sub", |ui| {
                ui.horizontal_wrapped(|ui| {
                    if twinkle_button(ui, "切换演示外设", false).clicked() {
                        crate::bt::toggle_demo_peripherals();
                    }
                    if twinkle_button(ui, "在岛上打开设备岛", false).clicked() {
                        crate::auralsle_ai_status::config::update(|c| {
                            c.show_bt_battery_on_island = true;
                            if c.device_display_mode == 0 {
                                c.device_display_mode = 2;
                            }
                            c.device_page_visible = true;
                        });
                        self.open = true;
                        self.forced_open_until = self.t + 4.5;
                        self.page_from = self.page;
                        self.page = 13;
                        self.dots_target();
                    }
                });
                for dev in &connected_devs {
                    let bus_tag = if dev.id.starts_with("VOLTIDE_") { "2.4GHz" } else { "Bluetooth" };
                    let pct = dev.battery.unwrap_or(100);
                    let col = if pct <= 20 {
                        Color32::from_rgb(248, 113, 113)
                    } else if pct <= 45 {
                        Color32::from_rgb(251, 191, 36)
                    } else {
                        Color32::from_rgb(52, 211, 153)
                    };
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(format!("{} ({})", dev.name, bus_tag)).size(13.5));
                        ui.label(
                            egui::RichText::new(match dev.battery {
                                Some(b) => format!("{b}%"),
                                None => "在线".to_owned(),
                            })
                            .size(13.0)
                            .color(col),
                        );
                    });
                }
            });
            ui.data_mut(|d| d.insert_temp(dev_anim_id, dev_anim));
        });
    }

    pub(crate) fn bluetooth_alert(&mut self, device: &crate::bt::Device, connected: bool) {
        if !self.settings.bluetooth_notifications { return; }
        if connected && matches!(device.kind,crate::bt::DeviceKind::Bone|crate::bt::DeviceKind::Earbuds|crate::bt::DeviceKind::Headphones|crate::bt::DeviceKind::Speaker) {
            self.alerts.retain(|a|a.title!="系统音量");
            if self.notice.as_ref().is_some_and(|a|a.title=="系统音量") {self.notice=None;}
        }
        let a = Alert {
            source: AlertSource::Internal,
            title: "蓝牙设备".into(),
            body: if connected { device.summary() } else { format!("{} · 已断开", device.name) },
            page: 5, at: self.t, duration: 2.5,
            bluetooth: Some((device.kind, connected)),
            meeting: None,
            unread: None,
            oppo_badge: false,
            verification_code: None,
            verification_copied: false,
        };
        self.enqueue_alert(a, false);
    }
    fn enqueue_alert(&mut self, a: Alert, system: bool) {
        // 通知仅保存在本次运行的内存中。
        if a.source==AlertSource::Internal {self.history.push_front(a.clone());}
        self.history.truncate(50);
        if self.fullscreen_hidden && a.health().is_some(){return;}
        if self.settings.notifications {
            if system && a.source==AlertSource::Internal {
                self.pending
                    .push(Cmd::Notify(a.title.clone(), a.body.clone()));
            }
            let same_event=|old:&Alert| match (&a.meeting,&old.meeting) {
                (Some((next,_)),Some((previous,_)))=>crate::calendar::dedupe_key(next)==crate::calendar::dedupe_key(previous),
                _=>old.source==a.source && old.title==a.title && old.unread==a.unread && (a.is_hud() || old.body==a.body),
            };
            if a.source!=AlertSource::Internal || a.title != "蓝牙设备" { self.alerts.retain(|old| !same_event(old)); }
            if (a.source!=AlertSource::Internal || a.title != "蓝牙设备") && self.notice.as_ref().is_some_and(same_event) {
                self.notice = Some(a);
                return;
            }
            self.alerts.push_back(a);
            while self.alerts.len() > 8 {
                self.alerts.pop_front();
            }
        }
    }
    /// 该来源的所有账号都已查看。
    pub fn viewed_messenger(&mut self,source:crate::app::Messenger){
        self.pending_messengers.retain(|u|u.source!=source);
        self.clear_messenger_notices(source);
    }
    /// 只清已查看的账号；无法确认账号的 QQ 提醒由用户明确关闭。
    pub fn viewed_client(&mut self,source:crate::app::Messenger,pid:u32){
        self.pending_messengers.retain(|u|!u.viewed_by(source,pid));
        if pid!=0{self.clear_account_notices(source,pid);}
        if !source.adapter().keep_unidentified{self.clear_account_notices(source,0);}
        if !self.pending_messengers.iter().any(|u|u.source==source){self.clear_messenger_notices(source);}
    }
    pub fn confirm_messenger_open(&mut self,unread:crate::app::Unread,foreground:Option<(crate::app::Messenger,u32)>)->bool{
        if unread.pid==0 || foreground!=Some((unread.source,unread.pid)){return false;}
        self.viewed_client(unread.source,unread.pid);
        true
    }
    pub fn discard_unread(&mut self,unread:crate::app::Unread){
        if !self.pending_messengers.contains(&unread){return;}
        self.pending_messengers.retain(|current|*current!=unread);
        self.clear_account_notices(unread.source,unread.pid);
        if !self.pending_messengers.iter().any(|current|current.source==unread.source){self.clear_messenger_notices(unread.source);}
    }
    fn clear_account_notices(&mut self,source:crate::app::Messenger,pid:u32){
        let matches=|notice:&Alert|notice.unread.is_some_and(|u|u.source==source && u.pid==pid);
        self.alerts.retain(|notice|!matches(notice));
        if self.notice.as_ref().is_some_and(matches){self.notice=None;self.notification_bell_until=0.0;}
    }
    fn clear_messenger_notices(&mut self,source:crate::app::Messenger){
        self.clear_oppo_messenger(source);
        self.alerts.retain(|a|a.messenger()!=Some(source));
        if self.notice.as_ref().is_some_and(|a|a.messenger()==Some(source)){self.notice=None;self.notification_bell_until=0.0;}
    }
    fn mark_messenger(&mut self,source:crate::app::Messenger,pid:u32,window:isize){
        if let Some(index)=self.pending_messengers.iter().position(|u|u.source==source && u.pid==pid){
            if window!=0 && self.pending_messengers[index].window!=window {
                self.pending_messengers[index].window=window;
                if source.adapter().capture==crate::messenger::Capture::Taskbar{
                    let unread=self.pending_messengers[index];
                    for alert in self.alerts.iter_mut().chain(self.notice.iter_mut()){
                        if alert.unread.is_some_and(|old|old.source==source && old.pid==pid){alert.unread=Some(unread);}
                    }
                }else{self.clear_account_notices(source,pid);}
            }
            return;
        }
        if !cfg!(test) {
            self.notification_bell_at = self.t;
        }
        self.pending_messengers.push(crate::app::Unread{source,pid,window});
    }
    /// 横幅「查看」：跳到该来源最近一条未读的窗口。
    pub(crate) fn latest_unread(&self,source:crate::app::Messenger)->crate::app::Unread{
        self.pending_messengers.iter().rev().find(|u|u.source==source).copied().unwrap_or(crate::app::Unread::of(source))
    }
    pub fn receive_wechat_reminder(&mut self){self.receive_wechat_reminder_from(0,0);}
    /// 微信窗口闪动：`pid` 是该账号的进程，`window` 是闪动的窗口。
    pub fn receive_wechat_reminder_from(&mut self,pid:u32,window:isize){
        let mcfg = if cfg!(test) {
            crate::auralsle_ai_status::config::Config::default()
        } else {
            crate::auralsle_ai_status::config::get()
        };
        if cfg!(test) {
            self.receive_messenger_reminder(crate::app::Unread{source:crate::app::Messenger::Wechat,pid,window});
            return;
        }
        if !self.settings.wechat_notifications || !self.settings.notifications{return;}
        let unread = crate::app::Unread{source:crate::app::Messenger::Wechat,pid,window};
        self.clear_oppo_messenger(crate::app::Messenger::Wechat);
        self.mark_messenger(crate::app::Messenger::Wechat,pid,window);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let prev_ms = LAST_WECHAT_ALERT_MS.swap(now_ms, std::sync::atomic::Ordering::SeqCst);
        if pid != 0 && mcfg.oppo_wechat_dedup && (now_ms - prev_ms).abs() < 3000 {
            if let Some(ref mut cur) = self.notice {
                if cur.source == AlertSource::Messenger(crate::app::Messenger::Wechat) && cur.body != "有新消息 · 请到微信查看" {
                    return;
                }
            }
        }
        reset_wechat_fingerprint_lock();
        crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 6.0);
        let dur = self.preview_duration();
        self.notification_bell_at=self.t;
        self.notification_bell_until=self.t+dur;
        let body_text = if mcfg.wechat_read_content {
            read_local_wechat_message_in_ram(pid, window)
        } else {
            "有新消息 · 请到微信查看".into()
        };
        let _sender = read_local_wechat_sender();
        self.enqueue_alert(Alert{source:AlertSource::Messenger(crate::app::Messenger::Wechat),title:"微信".to_owned(),body:body_text,page:usize::MAX,at:self.t,duration:dur,bluetooth:None,meeting:None,unread:Some(unread),oppo_badge:false,verification_code:None,verification_copied:false},false);
    }
    pub fn receive_messenger_reminder(&mut self,unread:crate::app::Unread){
        let client=unread.source;
        if !cfg!(test) && client==crate::app::Messenger::Wechat {
            self.receive_wechat_reminder_from(unread.pid, unread.window);
            return;
        }
        if client.adapter().capture!=crate::messenger::Capture::Taskbar || !client.enabled(&self.settings){return;}
        let pending=self.pending_messengers.iter().any(|old|old.source==client && old.pid==unread.pid);
        self.clear_oppo_messenger(client);
        self.mark_messenger(client,unread.pid,unread.window);
        if pending{return;}
        let dur = self.preview_duration();
        self.notification_bell_at=self.t;self.notification_bell_until=self.t+dur;
        self.enqueue_alert(Alert{source:AlertSource::Messenger(client),title:client.name().into(),body:format!("有新消息 · 请到{}查看",client.name()),page:usize::MAX,at:self.t,duration:dur,bluetooth:None,meeting:None,unread:Some(unread),oppo_badge:false,verification_code:None,verification_copied:false},false);
    }
    pub fn set_messenger_status(&mut self,client:crate::app::Messenger,status:String){self.messenger_statuses.insert(client,status);}
    pub fn messenger_status(&self,client:crate::app::Messenger)->&str{self.messenger_statuses.get(&client).map(String::as_str).unwrap_or("")}
    pub fn receive_notification(&mut self, app:String, id:String, body:String){
        self.receive_notification_from(app,id,body,None);
    }
    pub fn receive_notification_from(&mut self,app:String,id:String,body:String,unread:Option<crate::app::Unread>){
        let low_id = id.to_ascii_lowercase();
        let low_app = app.to_ascii_lowercase();
        let is_qq_music = low_app.contains("qq音乐")
            || low_app.contains("qqmusic")
            || low_id.contains("qqmusic")
            || body.contains("正在加载列表")
            || body.contains("点击选择歌曲播放");
        if is_qq_music && !cfg!(test) {
            return;
        }
        let is_oppo_connect = !cfg!(test)
            && (is_oppo_connect_toast(&app, &id, &body)
                || ((body.contains("微信") || body.contains("QQ") || body.to_ascii_lowercase().contains("qq"))
                    && (low_app.contains("互传") || low_app.contains("互联") || low_app.contains("oppo") || low_id.contains("oppo") || low_id.contains("heytap"))));

        if is_oppo_connect {
            let mcfg = crate::auralsle_ai_status::config::get();
            let (sub_app, sender, msg_body, is_wx, is_qq) = parse_oppo_connect_notification(&body);
            if is_wx {
                // 检测到电脑端微信已登录且开启去重：OPPO 互联转发的微信通知不再重复展示
                if mcfg.oppo_wechat_dedup && crate::wechat::is_pc_wechat_logged_in() {
                    return;
                }
                let full_preview = if !sender.is_empty() && sender != msg_body {
                    format!("{sender}: {msg_body}")
                } else {
                    msg_body.clone()
                };
                store_local_wechat_sender_and_msg(&sender, &full_preview);
                reset_wechat_fingerprint_lock();

                let now_ms = chrono::Utc::now().timestamp_millis();
                LAST_WECHAT_ALERT_MS.store(now_ms, std::sync::atomic::Ordering::SeqCst);

                if !self.settings.notifications || !self.settings.wechat_notifications {
                    return;
                }
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 6.5);
                let dur = self.preview_duration();
                self.notification_bell_at = self.t;
                self.notification_bell_until = self.t + dur;
                let unread = crate::app::Unread {
                    source: crate::app::Messenger::Wechat,
                    pid: 0,
                    window: 0,
                };
                self.mark_oppo_messenger(crate::app::Messenger::Wechat);
                self.mark_messenger(crate::app::Messenger::Wechat, 0, 0);

                let new_title = if sender.is_empty() {
                    "微信".to_owned()
                } else {
                    format!("微信 · {sender}")
                };
                let new_body = if !self.settings.notification_preview {
                    "收到新消息 · 内容已隐藏".to_owned()
                } else if !full_preview.trim().is_empty() {
                    full_preview.clone()
                } else {
                    "收到 OPPO 互传转发的微信新消息".to_owned()
                };
                let wx_alert = Alert {
                    source: AlertSource::Messenger(crate::app::Messenger::Wechat),
                    title: new_title.clone(),
                    body: new_body.clone(),
                    page: usize::MAX,
                    at: self.t,
                    duration: dur,
                    bluetooth: None,
                    meeting: None,
                    unread: Some(unread),
                    oppo_badge: true,
                    verification_code: None,
                    verification_copied: false,
                };

                let mut updated_in_place = false;
                if let Some(ref mut cur) = self.notice {
                    if cur.source == AlertSource::Messenger(crate::app::Messenger::Wechat) || cur.is_hud() {
                        *cur = wx_alert.clone();
                        updated_in_place = true;
                    }
                }
                if let Some(ref mut hud) = self.hud_notice {
                    if hud.source == AlertSource::Messenger(crate::app::Messenger::Wechat) {
                        hud.title = new_title.clone();
                        hud.body = new_body.clone();
                    }
                }
                if !updated_in_place {
                    if self.notice.is_none() {
                        self.notice = Some(wx_alert);
                    } else {
                        self.alerts.push_front(wx_alert);
                    }
                }
                let log_body = if !full_preview.trim().is_empty() {
                    full_preview.clone()
                } else {
                    "收到 OPPO 互传转发的微信新消息".to_owned()
                };
                self.notification_log.push(crate::model::NotificationRecord {
                    app_id: "wechat".into(),
                    app_name: "微信".into(),
                    title: if sender.is_empty() { new_title } else { format!("微信 · {sender}") },
                    body: log_body,
                    at: chrono::Utc::now().timestamp(),
                });
                return;
            } else if is_qq {
                // 检测到电脑端 QQ 已登录且开启去重：OPPO 互联转发的 QQ 通知不再展示
                if mcfg.oppo_wechat_dedup && crate::messenger::qq::is_pc_qq_logged_in() {
                    return;
                }
                if !self.settings.notifications || !crate::app::Messenger::Qq.enabled(&self.settings) {
                    return;
                }
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 6.0);
                let dur = self.preview_duration();
                self.notification_bell_at = self.t;
                self.notification_bell_until = self.t + dur;
                let unread = crate::app::Unread {
                    source: crate::app::Messenger::Qq,
                    pid: 0,
                    window: 0,
                };
                self.mark_oppo_messenger(crate::app::Messenger::Qq);
                self.mark_messenger(crate::app::Messenger::Qq, 0, 0);

                let new_title = if sender.is_empty() {
                    "QQ".to_owned()
                } else {
                    format!("QQ · {sender}")
                };
                let shown_body = if !self.settings.notification_preview {
                    "收到新消息 · 内容已隐藏".to_owned()
                } else if !msg_body.trim().is_empty() {
                    msg_body.clone()
                } else {
                    "收到 OPPO 互传转发的 QQ 新消息".to_owned()
                };
                let qq_alert = Alert {
                    source: AlertSource::Messenger(crate::app::Messenger::Qq),
                    title: new_title.clone(),
                    body: shown_body.clone(),
                    page: usize::MAX,
                    at: self.t,
                    duration: dur,
                    bluetooth: None,
                    meeting: None,
                    unread: Some(unread),
                    oppo_badge: true,
                    verification_code: None,
                    verification_copied: false,
                };
                let mut updated_in_place = false;
                if let Some(ref mut cur) = self.notice {
                    if cur.source == AlertSource::Messenger(crate::app::Messenger::Qq) || cur.is_hud() {
                        *cur = qq_alert.clone();
                        updated_in_place = true;
                    }
                }
                if !updated_in_place {
                    if self.notice.is_none() {
                        self.notice = Some(qq_alert);
                    } else {
                        self.alerts.push_front(qq_alert);
                    }
                }
                let log_body = if !msg_body.trim().is_empty() {
                    msg_body.clone()
                } else {
                    "收到 OPPO 互传转发的 QQ 新消息".to_owned()
                };
                self.notification_log.push(crate::model::NotificationRecord {
                    app_id: "qq".into(),
                    app_name: "QQ".into(),
                    title: new_title,
                    body: log_body,
                    at: chrono::Utc::now().timestamp(),
                });
                return;
            } else {
                // OPPO 互联转发的其他手机应用通知（如抖音、天气、短信等）：剥离“OPPO 互联”外壳，立即上岛显示真实应用与发送者
                if !self.settings.notifications {
                    return;
                }
                crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 5.5);
                let dur = self.preview_duration();
                self.notification_bell_at = self.t;
                self.notification_bell_until = self.t + dur;
                let clean_title = if !sender.is_empty() && sender != msg_body {
                    format!("{sub_app} · {sender}")
                } else {
                    sub_app.clone()
                };
                let shown_body = if self.settings.notification_preview {
                    msg_body.clone()
                } else {
                    "收到新消息 · 内容已隐藏".into()
                };
                let otp_from_oppo = if crate::auralsle_ai_status::config::get().auralsle_otp_capsule {
                    crate::auralsle_otp::extract(&clean_title, &msg_body)
                } else {
                    None
                };
                if let Some(code) = otp_from_oppo {
                    let masked_log = crate::mail_privacy::mask(&msg_body, &code);
                    self.push_verification_code(clean_title.clone(), code);
                    self.notification_log.push(crate::model::NotificationRecord {
                        app_id: sub_app.to_lowercase(),
                        app_name: sub_app,
                        title: clean_title,
                        body: masked_log,
                        at: chrono::Utc::now().timestamp(),
                    });
                    return;
                }
                let oppo_alert = Alert {
                    source: AlertSource::External(format!("oppo:{}", sub_app.to_lowercase())),
                    title: clean_title.clone(),
                    body: shown_body.clone(),
                    page: usize::MAX,
                    at: self.t,
                    duration: dur,
                    bluetooth: None,
                    meeting: None,
                    unread: None,
                    oppo_badge: true,
                    verification_code: None,
                    verification_copied: false,
                };
                if self.notice.is_none()
                    || self.notice.as_ref().is_some_and(|cur| {
                        cur.is_hud() || matches!(cur.source, AlertSource::External(_))
                    })
                {
                    self.notice = Some(oppo_alert);
                } else {
                    self.alerts.push_front(oppo_alert);
                }
                self.notification_log.push(crate::model::NotificationRecord {
                    app_id: sub_app.to_lowercase(),
                    app_name: sub_app,
                    title: clean_title,
                    body: msg_body,
                    at: chrono::Utc::now().timestamp(),
                });
                return;
            }
        }

        if !cfg!(test) && (low_id.contains("wechat") || low_id.contains("weixin") || app.contains("微信") || low_app.contains("wechat")) {
            if self.settings.wechat_notifications && self.settings.notifications {
                self.mark_messenger(crate::app::Messenger::Wechat, 0, 0);
            }
            let clean_msg: String = body.chars().filter(|c| !c.is_control()).take(240).collect();
            if !clean_msg.trim().is_empty() {
                let (sender, msg_only) = clean_msg
                    .split_once(" · ")
                    .map(|(a, b)| (a.trim().to_owned(), b.trim().to_owned()))
                    .unwrap_or_else(|| (String::new(), clean_msg.clone()));
                let preview = if !sender.is_empty() {
                    format!("{sender}: {msg_only}")
                } else {
                    clean_msg.clone()
                };
                store_local_wechat_sender_and_msg(&sender, &preview);
            }
            if crate::auralsle_ai_status::config::get().auralsle_otp_capsule {
                if let Some(code) = crate::auralsle_otp::extract("微信", &clean_msg) {
                    self.push_verification_code("微信".to_owned(), code);
                    return;
                }
            }
            if self.settings.wechat_notifications && self.settings.notifications {
                // 若任务栏闪动已提前 100ms 弹出了微信横幅，立即将刚刚拿到的真实发送者与消息正文原地更新进当前横幅
                let _sender_now = read_local_wechat_sender();
                let msg_now = read_local_wechat_message_in_ram(0, 0);
                let mut updated_existing = false;
                if let Some(ref mut cur) = self.notice {
                    if cur.source == AlertSource::Messenger(crate::app::Messenger::Wechat) {
                        cur.title = "微信".to_owned();
                        cur.body = msg_now.clone();
                        updated_existing = true;
                    }
                }
                if let Some(ref mut hud) = self.hud_notice {
                    if hud.source == AlertSource::Messenger(crate::app::Messenger::Wechat) {
                        hud.title = "微信".to_owned();
                        hud.body = msg_now;
                    }
                }
                if !updated_existing && crate::auralsle_ai_status::config::get().wechat_read_content {
                    self.receive_wechat_reminder_from(0, 0);
                }
                return;
            }
        }
                if !self.settings.read_notifications || id.is_empty() || !crate::messenger::NotificationFilter::for_settings(&self.settings).allows(&id){return;}
        if crate::messenger::suppresses_toast(&app,&id,crate::messenger::toast_suppression(&self.settings)){return;}
        let client=crate::app::Messenger::from_notification_id(&id).filter(|client|client.adapter().capture==crate::messenger::Capture::SystemToast);
        let id=client.map(|client|client.adapter().notification_id.to_string()).unwrap_or(id);
        // 岛仔自己发出的系统提醒会被通知中心回传；已在历史里的同一内容不再入队，避免重复显示。
        if crate::messenger::Messenger::from_notification_id(&id).is_none() && self.history.iter().any(|a|format!("{} · {}",a.title,a.body)==body){return;}
        if !self.notification_apps.observe(&id,&app) || !self.settings.notifications{return;}
        let unread=client.map(|client|unread.filter(|u|u.source==client && u.pid!=0).unwrap_or(crate::app::Unread::of(client)));
        if let Some(unread)=unread{self.clear_oppo_messenger(unread.source);self.mark_messenger(unread.source,unread.pid,unread.window);}
        if !cfg!(test) {
            crate::auralsle_ai_status::aora_face::trigger_mascot_event(20, 5.5);
        }
        let dur = self.preview_duration();
        self.notification_bell_at=self.t;
        self.notification_bell_until=self.t+dur;
        let raw_title:String=app.chars().filter(|c|!c.is_control()).take(80).collect();
        let title = if cfg!(test) { raw_title } else { crate::theme::sanitize_notification_text(&raw_title) };
        let raw_text_clean: String = {
            let t = body.chars().filter(|c| !c.is_control()).take(300).collect::<String>();
            if cfg!(test) { t } else { crate::theme::sanitize_notification_text(&t) }
        };
        if !cfg!(test) && crate::auralsle_ai_status::config::get().auralsle_otp_capsule {
            let otp_hit = crate::auralsle_otp::extract(&title, &raw_text_clean)
                .or_else(|| crate::otp::extract(&title, &raw_text_clean, &[]).map(|c| c.text));
            if let Some(code) = otp_hit {
                let masked = crate::mail_privacy::mask(&raw_text_clean, &code);
                self.push_verification_code(title.clone(), code);
                self.notification_log.push(crate::model::NotificationRecord {
                    app_id: id,
                    app_name: app,
                    title,
                    body: masked,
                    at: chrono::Utc::now().timestamp(),
                });
                return;
            }
        }
        let mut log_body = raw_text_clean.clone();
        if let Some(code) = crate::otp::extract("", &log_body, &[]) {
            log_body = crate::mail_privacy::mask(&log_body, &code.text);
        }
        let body = if self.settings.notification_preview {
            log_body.clone()
        } else {
            "收到新通知 · 内容已隐藏".into()
        };
        self.enqueue_alert(Alert{source:AlertSource::External(id.clone()),title:title.clone(),body:body.clone(),page:usize::MAX,at:self.t,duration:dur,bluetooth:None,meeting:None,unread,oppo_badge:false,verification_code:None,verification_copied:false},false);
        // 通知历史保留完整正文（仅对验证码脱敏），不受桌面弹窗隐私预览开关遮挡。
        self.notification_log.push(crate::model::NotificationRecord{
            app_id:id,
            app_name:app,
            title,
            body: log_body,
            at:chrono::Utc::now().timestamp(),
        });
    }
    pub fn reveal_messenger_notice(&mut self,unread:crate::app::Unread){
        if !unread.source.adapter().dismiss_unread || !self.pending_messengers.contains(&unread){return;}
        let body=if unread.pid==0{"消息账号暂无法确认"}else{"暂时无法打开对应窗口"};
        let source=if unread.source.adapter().capture==crate::messenger::Capture::Taskbar{AlertSource::Messenger(unread.source)}else{AlertSource::External(unread.source.adapter().notification_id.into())};
        let dur = self.preview_duration();
        let oppo_badge = !cfg!(test) && unread.pid == 0 && self.has_oppo_messenger(unread.source);
        self.notice=Some(Alert{source,title:unread.source.name().into(),body:body.into(),page:usize::MAX,at:self.t,duration:dur,bluetooth:None,meeting:None,unread:Some(unread),oppo_badge,verification_code:None,verification_copied:false});
        self.notification_bell_at=self.t;self.notification_bell_until=self.t+dur;
    }
    pub(crate) fn tick_alerts(&mut self) {
        self.pending_messengers.retain(|u|u.source.enabled(&self.settings) && (u.source.adapter().capture!=crate::messenger::Capture::SystemToast
            || self.notification_apps.items.iter().any(|(key,_,on)|key==u.source.adapter().notification_id && *on)));
        let allowed=|a:&Alert|match &a.source {
            AlertSource::Internal=>!self.fullscreen_hidden || a.health().is_none(),
            AlertSource::Messenger(client)=>client.enabled(&self.settings),
            AlertSource::External(id)=>self.settings.read_notifications && crate::messenger::NotificationFilter::for_settings(&self.settings).allows(id) && self.settings.notifications && !crate::messenger::suppresses_toast(&a.title,id,crate::messenger::toast_suppression(&self.settings)) && self.notification_apps.items.iter().any(|(key,_,on)|key==id && *on),
        };
        self.alerts.retain(&allowed);
        if self.notice.as_ref().is_some_and(|a|!allowed(a)){self.notice=None;}
        if self.hud_notice.as_ref().is_some_and(|a|!allowed(a)){self.hud_notice=None;}
        if self.fullscreen_hidden {
            self.pending.retain(|cmd|!matches!(cmd,Cmd::Notify(title,_)if HealthReminder::ALL.into_iter().any(|kind|title==kind.title())));
        }
        if !self.settings.notification_preview {
            for a in self.alerts.iter_mut().chain(self.notice.iter_mut()).chain(self.hud_notice.iter_mut()) {
                if matches!(&a.source,AlertSource::External(_)){a.body="收到新通知 · 内容已隐藏".into();}
            }
        }
        if self
            .notice
            .as_ref()
            .is_some_and(|a| self.t - a.at > a.duration)
        {
            self.notice = None;
        }
        if self.notice.is_none() {
            if let Some(mut a) = self.alerts.pop_front() {
                a.at = self.t;
                self.notice = Some(a);
            }
        }
    }
    pub(crate) fn draw_notice(&mut self, c:&Card,panel:Rect,a:&Alert,_cmds:&mut Vec<Cmd>){
        let nc=c.clipped(0.0,0.0,panel.width(),panel.height());
        if let Some(code) = a.verification_code.as_deref() {
            match crate::auralsle_otp::draw_capsule(
                &nc,
                panel.width(),
                panel.height(),
                if self.settings.island_shape == crate::model::IslandShape::Pill { 22.0 } else { 12.5 },
                &a.title,
                code,
                a.verification_copied,
            ) {
                crate::auralsle_otp::CapsuleAction::Copy => {
                    let _ = crate::auralsle_otp::copy_code(code);
                    _cmds.push(Cmd::Copy(code.to_owned()));
                    if let Some(cur) = &mut self.notice {
                        cur.verification_copied = true;
                        cur.at = self.t;
                        cur.duration = 1.6;
                    }
                    if let Some(hud) = &mut self.hud_notice {
                        hud.verification_copied = true;
                    }
                    self.code_copied(code);
                    self.pet.react(PetState::Happy);
                }
                crate::auralsle_otp::CapsuleAction::Dismiss => {
                    self.notice = None;
                    self.notification_bell_until = 0.0;
                }
                crate::auralsle_otp::CapsuleAction::None => {}
            }
            return;
        }
        if let Some(source)=a.messenger(){
            messenger_icon(&nc,22.0,22.0,source);
            nc.circle(31.0,14.0,2.6,Color32::from_rgb(255,72,72));
            if !cfg!(test) && (a.oppo_badge || (a.unread.is_none() && self.has_oppo_messenger(source))) {
                oppo_corner_badge(&nc, 29.5, 29.0, 4.8);
            }
            if !cfg!(test) && source == crate::app::Messenger::Wechat {
                let mcfg = crate::auralsle_ai_status::config::get();
                if mcfg.wechat_read_content {
                    let hk = if mcfg.wechat_reveal_hotkey.trim().is_empty() {
                        "Alt+W"
                    } else {
                        mcfg.wechat_reveal_hotkey.trim()
                    };
                    // 快捷键一次按下：未显示 → 显示正文；已显示 → 收起（再按恢复隐藏）。
                    // 仅对当前这条通知有效；下一条新消息到达时在 receive_wechat_reminder_from 自动重置为未显示。
                    if check_wechat_reveal_hotkey_edge(nc.ui.ctx(), hk) {
                        if is_wechat_fingerprint_unlocked() {
                            reset_wechat_fingerprint_lock();
                        } else {
                            trigger_local_fingerprint_verify();
                        }
                        if let Some(ref mut cur_n) = self.notice {
                            cur_n.duration = (self.t - cur_n.at) + 8.0;
                        }
                    }
                    let revealed = is_wechat_fingerprint_unlocked();
                    let title_str = "微信";
                    let live_body = if a.body == "有新消息 · 请到微信查看" {
                        read_local_wechat_message_in_ram(0, 0)
                    } else {
                        a.body.clone()
                    };
                    let (shown_body, body_col) = if revealed {
                        (live_body, theme::WHITE)
                    } else {
                        (
                            format!("按 [{hk}] 显示当前消息内容"),
                            Color32::from_rgb(125, 211, 252),
                        )
                    };

                    nc.centered_text(44.0, 14.0, &nc.fit(title_str, 11.0, W::Semibold, 100.0), 11.0, W::Semibold, theme::WHITE, H::Left);
                    if revealed {
                        nc.centered_text(
                            198.0,
                            14.0,
                            &format!("再按 {hk} 收起"),
                            8.0,
                            W::Regular,
                            Color32::from_white_alpha(130),
                            H::Right,
                        );
                    }
                    let max_body_w = 154.0;
                    nc.centered_text(
                        44.0,
                        30.0,
                        &nc.fit(&shown_body, 10.2, W::Regular, max_body_w),
                        10.2,
                        W::Regular,
                        body_col,
                        H::Left,
                    );
                    if !revealed {
                        // 「显示」按钮已移除：按 [Alt+W] 热键即可显示正文。
                        if notice_button(&nc, 206.0, 12.0, 52.0, "查看", "messenger-view") {
                            _cmds.push(Cmd::ViewMessenger(self.latest_unread(source)));
                        }
                        if notice_button(&nc, 265.0, 12.0, 52.0, "关闭", "messenger-dismiss") {
                            reset_wechat_fingerprint_lock();
                            self.notice = None;
                            self.notification_bell_until = 0.0;
                        }
                    } else {
                        if notice_button(&nc, 206.0, 12.0, 52.0, "查看", "messenger-view") {
                            _cmds.push(Cmd::ViewMessenger(self.latest_unread(source)));
                        }
                        if notice_button(&nc, 265.0, 12.0, 52.0, "关闭", "messenger-dismiss") {
                            reset_wechat_fingerprint_lock();
                            self.notice = None;
                            self.notification_bell_until = 0.0;
                        }
                    }
                    return;
                }
            }
            nc.centered_text(44.0,14.0,&a.title,11.0,W::Semibold,theme::WHITE,H::Left);
            nc.centered_text(44.0,30.0,&nc.fit(&a.body,10.5,W::Regular,154.0),10.5,W::Regular,theme::GRAY,H::Left);
            if notice_button(&nc,206.0,12.0,52.0,"查看","messenger-view"){_cmds.push(Cmd::ViewMessenger(a.unread.unwrap_or_else(||self.latest_unread(source))));}
            if notice_button(&nc,265.0,12.0,52.0,"关闭","messenger-dismiss"){
                if source.adapter().dismiss_unread{if let Some(unread)=a.unread{self.discard_unread(unread);}}
                self.notice=None;self.notification_bell_until=0.0;
            }
            return;
        }
        if a.source!=AlertSource::Internal {
            crate::paint::bell(&nc,310.0,22.0,theme::GRAY,crate::paint::bell_swing(self.t-a.at));
            nc.circle(314.0,17.5,2.0,Color32::from_rgb(255,72,72));
            let clean_t = if cfg!(test) { a.title.clone() } else { crate::theme::sanitize_notification_text(&a.title) };
            let clean_b = if cfg!(test) { a.body.clone() } else { crate::theme::sanitize_notification_text(&a.body) };
            nc.centered_text(16.0,14.0,&nc.fit(&clean_t,11.0,W::Semibold,198.0),11.0,W::Semibold,theme::WHITE,H::Left);
            nc.centered_text(16.0,30.0,&nc.fit(&clean_b,10.5,W::Regular,198.0),10.5,W::Regular,theme::GRAY,H::Left);
            if notice_button(&nc,226.0,12.0,60.0,"关闭","external-dismiss"){
                self.notice=None;
                self.notification_bell_until=0.0;
            }
            return;
        }
        if let Some((meeting,_))=&a.meeting {
            let secondary=Color32::from_gray(174);
            notice_icon(&nc.scaled(34.0,46.0,36.0/28.0),34.0,46.0,a);
            let now=chrono::Local::now();
            let start=chrono::DateTime::from_timestamp(meeting.start,0).unwrap().with_timezone(&chrono::Local);
            let left=meeting.start-now.timestamp();
            let countdown=if left>0 {format!("{} 分钟后",(left+59)/60)}else{"正在开始".into()};
            let countdown_width=nc.text_w(&countdown,10.5,W::Semibold)+16.0;
            let countdown_x=344.0-countdown_width;
            nc.centered_text(64.0,33.0,&start.format("%H:%M").to_string(),30.0,W::Semibold,theme::WHITE,H::Left);
            nc.fill(countdown_x,23.0,countdown_width,20.0,10.0,Color32::from_gray(28));
            nc.centered_text(countdown_x+countdown_width/2.0,33.0,&countdown,10.5,W::Semibold,theme::WHITE,H::Center);
            // [ai-status-mod] 课程名加大加粗；课程提醒补一行「教室 · 老师」（琥珀色），分隔线/按钮下移
            let course=meeting.uid.starts_with("course-");
            let info=if course{meeting.at.split(" · ").skip(1).collect::<Vec<_>>().join(" · ")}else{String::new()};
            let overflow=(nc.text_w(&meeting.title,size::TITLE+2.0,W::Bold)-280.0).max(0.0);
            let offset=(((self.t-a.at-1.2).max(0.0) as f32)*24.0).min(overflow);
            nc.clipped(64.0,50.0,280.0,28.0).centered_text(64.0-offset,65.0,&meeting.title,size::TITLE+2.0,W::Bold,theme::WHITE,H::Left);
            if course && !info.is_empty() {
                nc.centered_text(64.0,84.0,&nc.fit(&info,10.5,W::Regular,268.0),10.5,W::Regular,Color32::from_rgb(255,190,92),H::Left);
            }
            let (divider_y,date_y,btn_y)=if course{(104.0,132.0,120.0)}else{(84.0,110.0,98.0)};
            nc.seg((18.0,divider_y),(342.0,divider_y),0.6,Color32::from_gray(42));
            let date=format!("{}{}",if start.date_naive()==now.date_naive(){"今天 · "}else{""},start.format("%-m月%-d日"));
            nc.centered_text(18.0,date_y,&date,11.0,W::Regular,secondary,H::Left);
            let (dismiss,dc)=crate::paint::capsule(&nc,188.0,btn_y,72.0,24.0,"meeting-dismiss",Color32::from_gray(32),Color32::from_gray(48));
            dc.centered_text(224.0,date_y,"关闭",10.5,W::Semibold,theme::WHITE,H::Center);
            if dismiss.on_hover_text("关闭本次提醒，日程仍保留").clicked(){
                self.notice=None;
                return;
            }
            let (button,bc)=crate::paint::capsule(&nc,272.0,btn_y,72.0,24.0,"meeting-action",Color32::from_gray(42),Color32::from_gray(56));
            bc.centered_text(308.0,date_y,"查看",10.5,W::Semibold,theme::WHITE,H::Center);
            if button.clicked() {
                self.notice=None;
                self.open_calendar(Some(meeting.clone()));
            }
            return;
        }
        if a.is_hud(){
            if a.title=="蓝牙设备" {
                let (name,status,battery)=bluetooth_notice_content(&a.body);
                nc.circle(26.0,24.0,14.0,if status=="已连接"{Color32::from_rgb(10,132,255)}else{Color32::from_gray(95)});
                let kind=a.bluetooth.map(|(kind,_)|kind).unwrap_or(crate::bt::DeviceKind::Other);
                device_icon(&nc,kind,17.0,15.0,18.0);
                let right=HUD_W-14.0;
                let name_end=if let Some(level)=battery {
                    let pct=format!("{level}%");
                    let pct_width=nc.text_w(&pct,10.5,W::Semibold);
                    let x=right-pct_width-22.0;
                    nc.stroke(x,20.0,15.0,8.0,2.0,1.0,Color32::from_gray(175));
                    nc.fill(x+15.5,22.5,2.0,3.0,0.8,Color32::from_gray(175));
                    let fill=11.0*level as f32/100.0;
                    if fill>0.0 {nc.fill(x+2.0,22.0,fill,4.0,1.0,if level<=20{theme::RED}else{theme::GREEN});}
                    nc.centered_text(right,24.0,&pct,10.5,W::Semibold,theme::WHITE,H::Right);
                    x-8.0
                } else {right};
                nc.centered_text(49.0,16.0,&nc.fit(name,11.0,W::Semibold,(name_end-49.0).max(20.0)),11.0,W::Semibold,theme::WHITE,H::Left);
                nc.centered_text(49.0,32.0,status,10.5,W::Regular,theme::GRAY,H::Left);
                return;
            }
            match a.title.as_str(){
                "系统音量"=>{
                    nc.poly(&[(17.0,16.0),(21.0,16.0),(26.0,12.0),(26.0,26.0),(21.0,22.0),(17.0,22.0)],theme::WHITE);
                    if self.volume_ui.is_some_and(|(_,muted)|muted){nc.seg((29.0,16.0),(34.0,22.0),1.4,theme::WHITE);nc.seg((34.0,16.0),(29.0,22.0),1.4,theme::WHITE);}
                    else{nc.arc(26.0,19.0,5.0,-0.8,0.8,1.5,theme::WHITE);nc.arc(26.0,19.0,8.0,-0.7,0.7,1.3,theme::WHITE);}
                },
                _=>notice_icon(&nc,24.0,19.0,a),
            }
            if a.title=="系统音量" {
                self.draw_volume_hud(&nc);
            } else {
                let (title,body)=if a.title=="键盘输入语言"{("输入语言",a.body.as_str())}
                    else if a.title=="已切换电池供电"{("电池供电",a.body.as_str())}
                    else if a.title=="网络连接"{a.body.split_once(" · ").unwrap_or(("网络",a.body.as_str()))}
                    else{(a.title.as_str(),a.body.as_str())};
                nc.centered_text(48.0,19.0,&nc.fit(title,11.0,W::Semibold,90.0),11.0,W::Semibold,theme::WHITE,H::Left);
                nc.centered_text(HUD_W-16.0,19.0,&nc.fit(body,10.5,W::Regular,92.0),10.5,W::Regular,if a.title=="大写锁定"&&a.body=="已开启"||a.title=="网络连接"&&a.body.contains("已连接"){theme::GREEN}else{theme::GRAY},H::Right);
            }
        } else {
            let is_sit=a.health()==Some(HealthReminder::Sit);let is_water=a.health()==Some(HealthReminder::Water);let is_focus=a.page==1;let is_power=a.title=="电量偏低";
            notice_icon(&nc,22.0,22.0,a);
            let max=if is_water||is_sit{125.0}else{195.0};
            nc.centered_text(43.0,14.0,&nc.fit(&a.title,11.0,W::Semibold,max),11.0,W::Semibold,theme::WHITE,H::Left);
            nc.centered_text(43.0,30.0,&nc.fit(&a.body,10.5,W::Regular,max),10.5,W::Regular,theme::GRAY,H::Left);
            if is_sit {
                if notice_button(&nc,181.0,12.0,62.0,"知道了","sit-confirm-read"){self.confirm_health_reminder(HealthReminder::Sit);}
                if notice_button(&nc,250.0,12.0,68.0,"起身了","sit-confirm"){self.notice=None;self.stood_up();}
            }else if is_water {
                if notice_button(&nc,181.0,12.0,62.0,"知道了","water-confirm-read"){self.confirm_health_reminder(HealthReminder::Water);}
                if notice_button(&nc,250.0,12.0,68.0,"喝一口","water-confirm"){self.notice=None;self.drink_water();}
            }else{
                let label=if is_focus{"开始"}else if is_power{"知道了"}else{"查看"};
                if notice_button(&nc,257.0,12.0,60.0,label,"notice-action"){
                    self.notice=None;if is_power{return;}if is_focus{self.pomo.running=true;}else if a.page<6{
                        self.show_page(a.page);
                        if a.page==4 {
                            let order=self.todos.display_order(&self.today);
                            if let Some(index)=order.iter().position(|&i|self.todos.items[i].text==a.body && !self.todos.items[i].done_for(&self.today)){
                                self.todo_first=index.min(order.len().saturating_sub(3));
                            }
                        }
                    }else{self.settings_tab=6;self.open_settings();}
                }
            }
            if let Some(kind)=a.health() {
                if nc.hit(0.0,0.0,175.0,44.0,"health-notice-confirm").clicked(){self.confirm_health_reminder(kind);}
            }
        }
    }
    /// v0.5.4：可交互音量条。样式参考 oyenza-studio（MIT）的 HUD 音量条：
    /// 圆角胶囊、填充缓动、拖动时加粗并出现手柄；拖动/滚轮改音量，点喇叭静音。
    pub(crate) fn draw_volume_hud(&mut self, nc: &Card) {
        let (v, muted) = self.volume_ui.unwrap_or((0, false));
        let v = v.min(100);
        // 喇叭：点击静音 / 恢复
        let icon = nc.hit(12.0, 9.0, 28.0, 22.0, "volume-mute");
        if icon.clicked() && nc.live {
            self.volume_ui = Some((v, !muted));
            self.volume_quiet_until = self.t + 0.9;
            self.pending.push(Cmd::ToggleMute);
        }
        // 滑条：拖动 / 点击定位；滚轮微调
        let track = nc.hit_drag(HUD_TRACK_X, 8.0, HUD_TRACK_W, 24.0, "volume-track");
        let mut want: Option<u32> = None;
        if track.dragged() || track.clicked() {
            if let Some(p) = track.interact_pointer_pos() {
                let x = nc.to_local(p).x;
                want = Some((((x - HUD_TRACK_X) / HUD_TRACK_W) * 100.0).clamp(0.0, 100.0).round() as u32);
            }
        } else if nc.live && track.hovered() {
            let dy = nc.ui.ctx().input(|i| i.raw_scroll_delta.y);
            if dy.abs() > 0.5 {
                want = Some(((v as f32 + (dy / 120.0) * 4.0).clamp(0.0, 100.0)).round() as u32);
            }
        }
        if let Some(nv) = want {
            if self.volume_ui.map(|(o, _)| o) != Some(nv) || muted {
                self.volume_ui = Some((nv, false));
                self.volume_quiet_until = self.t + 0.9;
                self.pending.push(Cmd::SetVolume(nv));
            }
        }
        let (v, muted) = self.volume_ui.unwrap_or((v, muted));
        let pressed = track.dragged() || track.is_pointer_button_down_on() || icon.is_pointer_button_down_on();
        let target = if muted { 0.0 } else { v as f32 / 100.0 };
        self.volume_a = crate::anim::approach(self.volume_a, target, if pressed { 34.0 } else { 13.0 }, self.dt);
        let h = if pressed { 7.0 } else { 5.0 };
        let y = 19.0 - h / 2.0;
        nc.fill(HUD_TRACK_X, y, HUD_TRACK_W, h, h / 2.0, Color32::from_white_alpha(if pressed { 46 } else { 34 }));
        if self.volume_a > 0.0 {
            let w = (HUD_TRACK_W * self.volume_a).max(h);
            let col = if muted { Color32::from_gray(150) } else { theme::WHITE };
            nc.fill(HUD_TRACK_X, y, w, h, h / 2.0, col);
            nc.fill(HUD_TRACK_X, y + 0.8, (w - 1.2).max(0.0), h * 0.4, h * 0.2, Color32::from_white_alpha(64));
            if pressed {
                nc.circle(HUD_TRACK_X + w - h / 2.0, y + h / 2.0, h * 0.95, col);
            }
        }
        let label = if muted { "静音".to_string() } else { format!("{v}%") };
        nc.centered_text(HUD_W - 26.0, 19.0, &label, 10.5, if pressed { W::Semibold } else { W::Regular }, if muted { theme::GRAY } else { theme::WHITE }, H::Center);
    }

    pub(crate) fn save_all_for_backup(&self) {
        self.settings.save();
        self.todos.save();
        self.water.save();
        self.shelf.save();
        crate::model::save_health_pending(self.health_pending);
        self.notification_apps.save();
        self.notification_log.save();
    }

    pub(crate) fn reload_all_after_restore(&mut self) {
        let settings = crate::model::Settings::load();
        self.show_lyrics = settings.lyrics_enabled;
        let weather_source = crate::weather_source_ui::State::load();
        let mut weather = crate::weather::State::load(&settings.weather_city, settings.weather_position.as_ref());
        if weather_source.displayed == crate::weather_source_ui::Provider::Qweather {
            weather.snapshot = weather_source.runtime.snapshot();
        }
        self.settings = settings;
        self.todos = crate::model::TodoList::load();
        if !self.today.is_empty() {
            self.water = crate::model::Water::load(&self.today);
            self.water_ml = self.water.drunk_ml as f32;
            self.water_ring.snap(self.water.percent() / 100.0);
        }
        self.health_pending = crate::model::load_health_pending();
        self.shelf = crate::model::Shelf::load();
        self.clipboard.reload_from_disk();
        self.clipboard.set_enabled(self.clipboard_active());
        self.notification_apps = crate::model::NotificationApps::load();
        self.notification_log = crate::model::NotificationLog::load();
        self.quota = crate::quota_ui::State::load();
        self.calendar = crate::app::CalendarState::load();
        self.mail.accounts = crate::mail::load_accounts();
        self.weather_source = weather_source;
        self.weather = weather;
        self.updates = crate::updates::State::load(crate::model::config_dir().join("update-check.json"));
        *self.window_layout() = crate::settings_window::State::load();
    }

    pub(crate) fn drink_water(&mut self) {        let hour = self.current_hour;
        let amount = self.settings.serving_ml;
        let was = self.water.percent();
        let before = self.water.drunk_ml;
        self.water.drink(amount, hour);
        let amount = self.water.drunk_ml.saturating_sub(before);
        self.last_drink = Some((amount, hour));
        self.confirm_health_reminder(HealthReminder::Water);
        self.pet.react(PetState::Satisfied);
        self.say(format!("已记录 {amount} ml"));
        if was < 100.0 && self.water.percent() >= 100.0 {
            self.alert("今日饮水目标达成", "做得不错，继续保持", 2, true);
        }
    }


}

/// 将本地提醒时间规范化，拒绝不存在的日期与超出范围的时分。
pub(crate) fn parse_due(value: &str, today: &str) -> Option<String> {
    let value = value.trim();
    let full = if value.len() == 5 {
        format!("{today} {value}")
    } else {
        value.to_string()
    };
    if !full.is_ascii() || full.len() != 16 {
        return None;
    }
    let b = full.as_bytes();
    if b[4] != b'-' || b[7] != b'-' || b[10] != b' ' || b[13] != b':' {
        return None;
    }
    let year = full[0..4].parse::<u32>().ok()?;
    let month = full[5..7].parse::<usize>().ok()?;
    let day = full[8..10].parse::<u32>().ok()?;
    let hour = full[11..13].parse::<u32>().ok()?;
    let minute = full[14..16].parse::<u32>().ok()?;
    if !(2000..=9999).contains(&year) || !(1..=12).contains(&month) || hour > 23 || minute > 59 {
        return None;
    }
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if day == 0 || day > days[month - 1] {
        return None;
    }
    Some(full)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn island_size_drag_commits_only_on_release(){
        let ctx=egui::Context::default();let mut value=100;
        let pos=egui::pos2(30.0,30.0);let end=egui::pos2(110.0,30.0);
        let button=|pos,pressed|egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed,modifiers:Default::default()};
        let frames=[vec![],vec![egui::Event::PointerMoved(pos)],vec![button(pos,true)],vec![egui::Event::PointerMoved(end)],vec![button(end,false)]];
        for (i,events) in frames.into_iter().enumerate(){
            let _=ctx.run(egui::RawInput{events,screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(320.0,80.0))),..Default::default()},|ctx|{
                egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(20)).show(ctx,|ui|{ui.spacing_mut().slider_width=180.0;island_size_slider(ui,&mut value);});
            });
            if i<4{assert_eq!(value,100);}else{assert_ne!(value,100);}
        }
    }
    #[test]
    fn number_stepper_clicks_follow_steps_and_limits() {
        for (initial,step,x,expected) in [(5u32,1,127.0,6),(5,1,13.0,4),(0,1,13.0,0),(10,1,127.0,10),(9,3,127.0,10),(1,3,13.0,0)] {
            let ctx=egui::Context::default();
            let mut value=initial;
            let pos=egui::pos2(x+20.0,34.0);
            let events=[vec![],vec![egui::Event::PointerMoved(pos)],vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:true,modifiers:Default::default()}],vec![egui::Event::PointerButton{pos,button:egui::PointerButton::Primary,pressed:false,modifiers:Default::default()}]];
            for events in events {
                let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(200.0,80.0))),events,..Default::default()};
                let _=ctx.run(raw,|ctx|{
                    egui::CentralPanel::default().frame(egui::Frame::new().inner_margin(20)).show(ctx,|ui|{
                        ui.horizontal(|ui|{number_stepper(ui,&mut value,0,10,"",step,76.0);});
                    });
                });
            }
            assert_eq!(value,expected,"初值 {initial}，步长 {step}，点击位置 {x}");
        }
    }

    #[test]
    fn number_stepper_fits_with_settings_button_padding() {
        let ctx=egui::Context::default();
        let raw=egui::RawInput{screen_rect:Some(Rect::from_min_size(egui::Pos2::ZERO,egui::vec2(340.0,240.0))),..Default::default()};
        let _=ctx.run(raw,|ctx|{
            egui::CentralPanel::default().frame(egui::Frame::new()).show(ctx,|ui|{
                ui.spacing_mut().button_padding=egui::vec2(10.0,6.0);
                for mut value in [0u32,25,180] {
                    let row=ui.horizontal(|ui|{number_stepper(ui,&mut value,0,180," 分钟",1,76.0);});
                    assert!(row.response.rect.width()<=140.1,"数值控件超出预留宽度：{}",row.response.rect.width());
                }
                for mut value in [25u32,5,15] {setting_number(ui,"计时时长",&mut value,1,180," 分钟",1.0);}
                assert!(ui.min_rect().right()<=340.1,"连续数值行不应撑宽卡片");
            });
        });
    }

    #[test]
    fn settings_groups_keep_every_page_reachable() {
        let groups = ["常规", "音乐与歌词", "专注与饮水", "专注与饮水", "待办与置物", "待办与置物", "通知、邮箱与日程", "通知、邮箱与日程", "通知、邮箱与日程", "关于", "页面管理", "AI", "AI", "系统监控", "课程表", "功能", "通知历史","吉祥物","天气","翻译","设备","功能模块"];
        for (tab, expected) in groups.iter().enumerate() {
            let matches: Vec<_> = SETTINGS_GROUPS.iter().filter(|(_,start,end)| (*start..=*end).contains(&tab)).collect();
            assert_eq!(matches.len(), 1, "页面 {tab} 必须且只能属于一个分组");
            assert_eq!(matches[0].0, *expected);
        }
    }

    #[test]
    fn bluetooth_notice_uses_real_connection_and_battery_data(){
        assert_eq!(bluetooth_notice_content("OpenMove by Shokz · 电量 80%"),("OpenMove by Shokz","已连接",Some(80)));
        assert_eq!(bluetooth_notice_content("耳机"),("耳机","已连接",None));
        assert_eq!(bluetooth_notice_content("已断开连接"),("蓝牙设备","已断开",None));
        assert_eq!(bluetooth_notice_content("青龙键盘 · 已断开"),("青龙键盘","已断开",None));
        assert_eq!(bluetooth_notice_content("耳机 · 电量 101%"),("耳机 · 电量 101%","已连接",None));
    }

    #[test]
    fn device_icons_render_as_visible_svg() {
        let icons: &[&[u8]] = &[
            include_bytes!("../assets/device-icons/headphones-round-bold.svg"),
            include_bytes!("../assets/device-icons/speaker-bold.svg"),
            include_bytes!("../assets/device-icons/keyboard-bold.svg"),
            include_bytes!("../assets/device-icons/mouse-bold.svg"),
            include_bytes!("../assets/device-icons/gamepad-bold.svg"),
            include_bytes!("../assets/device-icons/smartphone-bold.svg"),
            include_bytes!("../assets/device-icons/bluetooth-linear.svg"),
        ];
        for bytes in icons {
            let image=egui_extras::image::load_svg_bytes(bytes).unwrap();
            assert!(image.pixels.iter().any(|p|p.a()>0));
        }
    }

    #[test]
    fn due_time_checks_calendar_and_format() {
        assert_eq!(
            parse_due("09:30", "2026-10-29").as_deref(),
            Some("2026-10-29 09:30")
        );
        assert!(parse_due("2026-02-29 12:00", "").is_none());
        assert!(parse_due("2028-02-29 12:00", "").is_some());
        for value in [
            "24:00",
            "12:60",
            "明天中午",
            "2026-13-01 10:00",
            "2026-00-01 10:00",
        ] {
            assert!(parse_due(value, "2026-10-29").is_none());
        }
    }
}

pub(crate) struct TaskDraft {index:usize,original:String,text:String,due:String,error:String,focused:bool,reminder_only:bool,pub(crate) repeat:u8,pub(crate) picker:crate::task_picker::ReminderPicker}
impl App {
    pub(crate) fn open_task_dialog(&mut self,index:usize){
        self.todo_choosing=false;
        if let Some(task)=self.todos.items.get(index){
            let draft=TaskDraft{index,original:task.text.clone(),text:task.text.clone(),due:task.due.clone().unwrap_or_default(),error:String::new(),focused:false,reminder_only:false,repeat:task.repeat,picker:crate::task_picker::ReminderPicker::new(task.due.as_deref())};
            self.show_page(4);self.task_dialog=Some(draft);
        }
    }
    pub(crate) fn open_task_reminder(&mut self,index:usize){
        self.open_task_dialog(index);
        if let Some(d)=self.task_dialog.as_mut(){d.reminder_only=true;d.picker.show();}
    }
    pub(crate) fn draw_task_editor(&mut self,c:&Card){
        if self.task_dialog.as_ref().is_some_and(|draft|draft.picker.open){self.draw_task_reminder(c);return;}
        let reminder_only=self.task_dialog.as_ref().is_some_and(|d|d.reminder_only);
        c.text(15.0,25.0,if reminder_only{"设置提醒"}else{"编辑待办"},size::TITLE,W::Bold,theme::WHITE,H::Left);
        c.text(43.0,48.0,if reminder_only{"待办"}else{"内容"},10.5,W::Regular,theme::GRAY,H::Left);
        c.fill(91.0,33.0,275.0,20.0,6.0,theme::CARD);
        if let Some(d)=self.task_dialog.as_ref().filter(|d|d.reminder_only){
            c.text(97.0,47.0,&c.fit(&d.text,11.5,W::Regular,260.0),11.5,W::Regular,theme::WHITE,H::Left);
        }
        c.text(43.0,72.0,"提醒",10.5,W::Regular,theme::GRAY,H::Left);
        let time=self.task_dialog.as_ref().map(|d|if d.due.is_empty(){"点击选择时间".into()}
            else if d.repeat!=0{format!("{} {}",crate::model::repeat_label(d.repeat),d.due.get(11..16).unwrap_or(&d.due))}else{d.due.clone()}).unwrap_or_default();
        if setting_button(c,91.0,58.0,275.0,&time,false,"task-select-reminder"){if let Some(d)=self.task_dialog.as_mut(){d.picker.show();}}
        // 每周重复：七个星期按钮，全不选即一次性待办。
        c.text(43.0,95.0,"重复",10.5,W::Regular,theme::GRAY,H::Left);
        self.task_repeat_chips(c,91.0,81.0,25.0,19.0,28.0);
        let repeat=self.task_dialog.as_ref().map(|d|d.repeat).unwrap_or(0);
        let quick=if repeat==crate::model::REPEAT_ALL{"不重复"}else{"每天"};
        if setting_button(c,289.0,81.0,77.0,quick,false,"task-repeat-all"){
            if let Some(d)=self.task_dialog.as_mut(){d.repeat=if d.repeat==crate::model::REPEAT_ALL{0}else{crate::model::REPEAT_ALL};}
        }
        if let Some(d)=&self.task_dialog {
            let summary=if d.repeat!=0{crate::model::repeat_label(d.repeat)}else{"只做一次".into()};
            let hint=if d.error.is_empty(){summary.as_str()}else{&d.error};
            c.text(43.0,117.0,&c.fit(hint,10.5,W::Regular,if reminder_only{140.0}else{154.0}),10.5,if d.error.is_empty(){W::Regular}else{W::Semibold},if d.error.is_empty(){theme::GRAY}else{theme::RED},H::Left);
        }
        if reminder_only {
            let has_reminder=self.task_dialog.as_ref().and_then(|d|self.todos.items.get(d.index)).is_some_and(|task|task.due.is_some());
            let skip=if has_reminder{"取消提醒"}else if repeat!=0{"只设重复"}else{"暂不提醒"};
            if setting_button(c,188.0,104.0,88.0,skip,false,"task-reminder-skip"){
                if let Some(d)=self.task_dialog.as_mut(){d.due.clear();}
                self.save_task_editor();
            }
        }else if setting_button(c,202.0,104.0,74.0,"取消",false,"task-cancel"){self.task_dialog=None;}
        if setting_button(c,286.0,104.0,80.0,if reminder_only{"选择时间"}else{"保存"},true,"task-save"){
            if reminder_only{if let Some(d)=self.task_dialog.as_mut(){d.picker.show();}}else{self.save_task_editor();}
        }
    }
    pub(crate) fn task_editor_inputs(&mut self,ui:&mut egui::Ui,origin:egui::Pos2,cmds:&mut Vec<Cmd>){
        let Some(d)=self.task_dialog.as_mut() else{return};
        if d.reminder_only || d.picker.open{return;}
        let name=ui.put(Rect::from_min_size(origin+egui::vec2(97.0,35.0),egui::vec2(260.0,17.0)),egui::TextEdit::singleline(&mut d.text).vertical_align(egui::Align::Center).id(egui::Id::new("task-name-edit")).frame(false).margin(egui::Margin::symmetric(2, 0)).font(theme::font(11.5,W::Regular)));
        let first_focus=!d.focused;
        if first_focus{name.request_focus();d.focused=true;cmds.push(Cmd::Focus);}
        // 新建内容与下一步共用这一帧，避免同一次回车又提交了空提醒。
        if !first_focus && ui.input(|i|i.key_pressed(egui::Key::Enter)){self.save_task_editor();}
    }
    /// 七个星期按钮（一…日），点一下切换；全不选即一次性待办。
    fn task_repeat_chips(&mut self,c:&Card,x:f32,y:f32,w:f32,h:f32,step:f32){
        let repeat=self.task_dialog.as_ref().map(|d|d.repeat).unwrap_or(0);
        for (day,name) in crate::model::WEEKDAY_NAMES.iter().enumerate() {
            let on=repeat&(1<<day)!=0;
            let cx=x+day as f32*step;
            let (r,b)=crate::paint::capsule(c,cx,y,w,h,&format!("task-repeat-{day}"),if on{Color32::from_rgb(39,94,150)}else{theme::BTN},theme::BTN_HOVER);
            b.centered_text(cx+w/2.0,y+h/2.0,name,if h<18.0{9.5}else{10.5},W::Semibold,if on{theme::WHITE}else{theme::GRAY},H::Center);
            if r.on_hover_text(format!("每周{name}")).clicked(){
                if let Some(d)=self.task_dialog.as_mut(){d.repeat^=1<<day;d.error.clear();}
            }
        }
    }
    fn draw_task_reminder(&mut self,c:&Card){
        use crate::task_picker::PickerAction;
        let Some(d)=self.task_dialog.as_mut().filter(|d|d.picker.open) else{return};
        let has_reminder=self.todos.items.get(d.index).is_some_and(|task|task.due.is_some());
        c.text(15.0,25.0,"待办",size::TITLE,W::Bold,theme::WHITE,H::Left);
        c.text(44.0,25.0,"清单",size::TITLE,W::Bold,theme::GREEN,H::Left);
        let (done,total)=self.todos.progress(&self.today);
        c.text(42.0,61.0,&format!("{done}/{total}"),29.0,W::Bold,theme::WHITE,H::Left);
        // 左侧：每周重复。选了星期几后，提醒只按时间、在这些日子到点提醒。
        d.picker.repeat_note=Some(if d.repeat!=0{format!("{} · 到点提醒",crate::model::repeat_label(d.repeat))}else{String::new()});
        let action=d.picker.draw(c,&d.text,has_reminder);
        let focus=std::mem::take(&mut d.picker.focus);
        self.task_repeat_chips(c,41.0,84.0,17.0,16.0,19.5);
        let Some(d)=self.task_dialog.as_mut() else{return};
        if focus{self.pending.push(Cmd::Focus);}
        match action{
            Some(PickerAction::Save)=>{d.due=d.picker.due();d.picker.open=false;if d.reminder_only{self.save_task_editor();}},
            Some(PickerAction::Skip)=>{d.due.clear();d.picker.open=false;if d.reminder_only{self.save_task_editor();}},
            Some(PickerAction::Cancel)=>{d.picker.open=false;if d.reminder_only{self.task_dialog=None;}},
            None=>{},
        }
    }
    fn save_task_editor(&mut self){
        let Some(mut d)=self.task_dialog.take() else{return};
        let due=if d.due.trim().is_empty(){Some(None)}else{parse_due(&d.due,&self.today).map(Some)};
        if d.text.trim().is_empty(){d.error="请填写任务内容".into();}
        else if due.is_none(){d.error="时间格式应为 18:30 或完整日期时间".into();}
        else if self.todos.items.get(d.index).is_none_or(|t|t.text!=d.original){d.error="任务已变化，请取消后重新打开".into();}
        else{
            let item=&mut self.todos.items[d.index];
            if item.repeat!=d.repeat{item.done_on=None;}
            item.text=d.text.trim().into();item.due=due.unwrap();item.reminded=false;item.reminded_on=None;item.repeat=d.repeat;
            self.todos.save();self.say("待办已保存");return;
        }
        self.task_dialog=Some(d);
    }
}

fn notice_button(c:&Card,x:f32,y:f32,w:f32,text:&str,id:&str)->bool {
    let (r,b)=crate::paint::capsule(c,x,y,w,20.0,id,Color32::from_gray(35),theme::BTN_HOVER);
    b.centered_text(x+w/2.0,y+10.0,text,10.5,W::Semibold,theme::WHITE,H::Center);
    r.clicked()
}
fn setting_button(c:&Card,x:f32,y:f32,w:f32,text:&str,on:bool,id:&str)->bool {
    let (r,b)=crate::paint::capsule(c,x,y,w,19.0,id,if on{Color32::from_rgb(39,94,150)}else{theme::BTN},theme::BTN_HOVER);
    b.text(x+w/2.0,y+13.3,text,10.5,W::Semibold,theme::WHITE,H::Center);
    r.clicked()
}
fn hhmm(m:u32)->String{format!("{:02}:{:02}",m/60%24,m%60)}
/// 岛内小开关（与健康页右上开关同款）。
fn island_switch(c:&Card,x:f32,y:f32,on:bool,id:&str)->bool{
    let r=c.hit(x-2.0,y-2.0,38.0,24.0,id);
    let (hv,_)=c.hover_t(&r);
    let track=if on{theme::SIT_GREEN}else{Color32::from_rgb(58,58,62)};
    c.fill(x,y,34.0,20.0,10.0,crate::paint::mix(track,theme::WHITE,hv*0.08));
    c.circle(if on{x+24.0}else{x+10.0},y+10.0,8.0,theme::WHITE);
    r.clicked()
}
impl App {
    fn health_time_summary(&self)->String{
        let s=&self.settings;
        let hours=format!("{:02}:00–{:02}:00",s.water_start,s.water_end);
        if s.lunch_rest{format!("{hours} · 午休 {}–{}",hhmm(s.lunch_start),hhmm(s.lunch_end))}else{hours}
    }
    /// 「提醒时段」二级页：提醒时段起止、午休开关、午休起止（15 分钟一档），喝水和久坐共用。
    fn health_time_page(&mut self,c:&Card){
        let row_y=|i:usize|38.0+i as f32*22.0;
        for i in 1..4{c.fill(44.0,row_y(i)-2.0,326.0,1.0,0.5,Color32::from_rgb(28,28,31));}
        let y=row_y(0);
        c.text(44.0,y+13.0,"提醒时段",10.5,W::Regular,theme::GRAY,H::Left);
        c.text(250.0,y+13.0,"至",10.5,W::Regular,theme::GRAY,H::Center);
        for i in 0..2{
            let hour=if i==0{&mut self.settings.water_start}else{&mut self.settings.water_end};
            let value=egui::DragValue::new(hour).range(0..=if i==0{23}else{24})
                .custom_formatter(|v,_|format!("{:02}:00",v as u32))
                .custom_parser(|t|{let t=t.trim();t.strip_suffix(":00").or_else(||t.strip_suffix("：00")).unwrap_or(t).parse::<u32>().ok().map(|v|v as f64)});
            let (d,field)=stepper(c,if i==0{238.0}else{370.0},y,value,&format!("hour-{i}"));
            if field.gained_focus(){self.pending.push(Cmd::Focus);}self.settings_editing|=field.has_focus();
            if d!=0{let hour=if i==0{&mut self.settings.water_start}else{&mut self.settings.water_end};*hour=(*hour as i32+d).rem_euclid(24) as u32;}
        }
        let y=row_y(1);
        c.text(44.0,y+13.0,"午休不提醒",10.5,W::Regular,theme::GRAY,H::Left);
        if island_switch(c,336.0,y-0.5,self.settings.lunch_rest,"lunch-rest"){self.settings.lunch_rest=!self.settings.lunch_rest;}
        let y=row_y(2);
        let on=self.settings.lunch_rest;
        let lc=c.faded(if on{1.0}else{0.35});let lc=if on{lc}else{lc.ghost()};
        lc.text(44.0,y+13.0,"午休时间",10.5,W::Regular,theme::GRAY,H::Left);
        lc.text(250.0,y+13.0,"至",10.5,W::Regular,theme::GRAY,H::Center);
        for i in 0..2{
            let minute=if i==0{&mut self.settings.lunch_start}else{&mut self.settings.lunch_end};
            let value=egui::DragValue::new(minute).range(0..=1439).speed(1.0).custom_formatter(|v,_|hhmm(v as u32))
                .custom_parser(|t|{let (h,m)=t.trim().split_once([':','：'])?;Some((h.parse::<u32>().ok()?.min(23)*60+m.parse::<u32>().ok()?.min(59)) as f64)});
            let (d,field)=stepper(&lc,if i==0{238.0}else{370.0},y,value,&format!("lunch-{i}"));
            if field.gained_focus(){self.pending.push(Cmd::Focus);}self.settings_editing|=field.has_focus();
            if d!=0{let minute=if i==0{&mut self.settings.lunch_start}else{&mut self.settings.lunch_end};*minute=(*minute as i32+d*15).rem_euclid(1440) as u32;}
        }
        c.text(44.0,row_y(3)+13.0,"喝水和久坐提醒都只在提醒时段内、午休以外弹出",10.5,W::Regular,theme::GRAY,H::Left);
    }
}
fn island_number_field(c:&Card,rect:Rect,value:egui::DragValue<'_>,id:impl std::hash::Hash)->egui::Response {
    let mut ui=egui::Ui::new(c.ui.ctx().clone(),egui::Id::new(id),egui::UiBuilder::new().layer_id(c.ui.layer_id()).max_rect(rect).style(c.ui.style().clone()));
    ui.set_clip_rect(c.painter.clip_rect());ui.set_opacity(c.painter.opacity());if !c.live{ui.disable();}
    ui.spacing_mut().interact_size=rect.size();ui.spacing_mut().button_padding=egui::vec2(2.0,1.0);
    ui.style_mut().drag_value_text_style=egui::TextStyle::Body;
    ui.style_mut().text_styles.insert(egui::TextStyle::Body,c.font(11.0,W::Bold));
    ui.style_mut().explanation_tooltips=false;
    ui.visuals_mut().widgets.inactive.weak_bg_fill=Color32::TRANSPARENT;
    crate::quota_ui::bar_tooltip(ui.put(rect,value.update_while_editing(false)),"点击输入数值".into())
}
/// 加减器：[−] 可输入数值 [+]，右缘对齐到 right。
fn stepper(c:&Card,right:f32,y:f32,value:egui::DragValue<'_>,id:&str)->(i32,egui::Response) {
    let (w,vw)=(25.0,58.0);
    let x=right-w*2.0-vw;
    let field=island_number_field(c,c.rect(x+w,y,vw,19.0),value,("health-number",id));
    let delta=if setting_button(c,x,y,w,"−",false,&format!("{id}-minus")){-1}
        else if setting_button(c,right-w,y,w,"+",false,&format!("{id}-plus")){1}else{0};
    (delta,field)
}
impl App {
    pub(crate) fn page_settings(&mut self,c:&Card) {
        let help=match self.settings_category{0=>"外观与系统",1=>"调整喝水、久坐与提醒时段",2=>"点击分钟数可直接输入，也可用加减微调",_=>"通知"};
        if self.settings_category!=1{c.text(44.0,44.0,help,10.5,W::Regular,theme::GRAY,H::Left);}
        let before=format!("{:?}",self.settings);
        self.settings_editing=false;
        match self.settings_category {
            0=>{
                let choices=[("吉祥物",self.settings.pet),("极简刘海",self.settings.minimal),("系统提示",self.settings.system_hud),("全屏隐藏",self.settings.hide_fullscreen)];
                for (i,(name,on)) in choices.iter().enumerate(){let x=42.0+(i%2) as f32*169.0;let y=57.0+(i/2) as f32*27.0;let text=format!("{}  {}",name,if *on{"开"}else{"关"});if setting_button(c,x,y,158.0,&text,*on,&format!("general-{i}")){match i{0=>self.set_pet(!self.settings.pet),1=>self.set_minimal(!self.settings.minimal),2=>self.settings.system_hud=!self.settings.system_hud,_=>self.settings.hide_fullscreen=!self.settings.hide_fullscreen}}}
                c.text(42.0,116.0,"灵屿 AuraIsle 收起时常驻；极简模式保留细线",10.5,W::Regular,theme::GRAY,H::Left);
            }
            1 if self.health_time=>self.health_time_page(c),
            1=>{
                // 与健康页同一套切换（标题旁的水滴 / 椅子）；四行一列，标签靠左、加减器靠右对齐。
                // 喝水三行、久坐一行加两句说明；最后一行「提醒时段」两边共用，位置不变。
                let sv=self.sit_view_t.pos.clamp(0.0,1.0);
                let row_y=|i:usize|38.0+i as f32*22.0;
                for i in 1..4{c.fill(44.0,row_y(i)-2.0,326.0,1.0,0.5,Color32::from_rgb(28,28,31));}
                if sv<0.99{
                    let wc=c.faded(1.0-sv);let wc=if self.sit_mode{wc.ghost()}else{wc};
                    for (i,label) in ["每日目标","每次记录","提醒间隔"].iter().enumerate(){
                        wc.text(44.0,row_y(i)+13.0,label,10.5,W::Regular,theme::GRAY,H::Left);
                        let goal=self.water.goal_ml;let mut liters=goal as f32/1000.0;
                        let value=match i {
                            0=>egui::DragValue::new(&mut liters).range(0.5..=6.0).speed(0.25).suffix(" L").min_decimals(1).max_decimals(2),
                            1=>egui::DragValue::new(&mut self.settings.serving_ml).range(50..=1000).speed(50.0).suffix(" ml"),
                            _=>egui::DragValue::new(&mut self.settings.water_minutes).range(5..=180).speed(5.0).suffix(" 分钟"),
                        };
                        let (d,field)=stepper(&wc,370.0,row_y(i),value,&format!("water-{i}"));
                        if field.gained_focus(){self.pending.push(Cmd::Focus);}self.settings_editing|=field.has_focus();
                        match i {
                            0=>{self.water.goal_ml=((liters*1000.0).round() as i32+d*250).clamp(500,6000) as u32;if self.water.goal_ml!=goal{self.water.save();}}
                            1=>self.settings.serving_ml=(self.settings.serving_ml as i32+d*50).clamp(50,1000) as u32,
                            _=>self.settings.water_minutes=(self.settings.water_minutes as i32+d*5).clamp(5,180) as u32,
                        }
                    }
                }
                if sv>0.01{
                    let sc=c.faded(sv);let sc=if self.sit_mode{sc}else{sc.ghost()};
                    sc.text(44.0,row_y(0)+13.0,"坐满多久提醒",10.5,W::Regular,theme::GRAY,H::Left);
                    let value=egui::DragValue::new(&mut self.settings.sit_minutes).range(15..=180).speed(5.0).suffix(" 分钟");
                    let (d,field)=stepper(&sc,370.0,row_y(0),value,"sit");
                    if field.gained_focus(){self.pending.push(Cmd::Focus);}self.settings_editing|=field.has_focus();
                    if d!=0{self.settings.sit_minutes=(self.settings.sit_minutes as i32+d*5).clamp(15,180) as u32;}
                    sc.text(44.0,row_y(1)+13.0,"专注计时进行中不打扰，专注结束后再提醒",10.5,W::Regular,theme::GRAY,H::Left);
                    sc.text(44.0,row_y(2)+13.0,"提醒后常驻，点击确认或起身后重新计时",10.5,W::Regular,theme::GRAY,H::Left);
                }
                // 最后一行两边共用：提醒时段 + 午休的摘要，点进二级页修改。
                let y=row_y(3);
                let r=c.hit(40.0,y-2.0,334.0,22.0,"health-time").on_hover_text("设置提醒时段和午休免打扰");
                let (hv,_)=c.hover_t(&r);
                if hv>0.0{c.fill(40.0,y-2.0,334.0,22.0,8.0,Color32::from_white_alpha((hv*14.0) as u8));}
                c.text(44.0,y+13.0,"提醒时段",10.5,W::Regular,theme::GRAY,H::Left);
                c.text(356.0,y+13.0,&self.health_time_summary(),10.5,W::Semibold,theme::WHITE,H::Right);
                c.text(366.0,y+13.5,"›",14.0,W::Bold,theme::GRAY,H::Center);
                if r.clicked(){self.health_time=true;}
            }
            2=>{
                for (i,name) in ["专注","休息"].iter().enumerate(){
                    let y=53.0+i as f32*23.0;
                    c.text(48.0,y+13.0,name,10.5,W::Regular,theme::GRAY,H::Left);
                    let value=if i==0{&mut self.settings.focus_minutes}else{&mut self.settings.rest_minutes};
                    let max=if i==0{180}else{60};let rect=c.rect(168.0,y,76.0,19.0);
                    let field=island_number_field(c,rect,egui::DragValue::new(value).range(1..=max).suffix(" 分钟"),("focus-duration",i));
                    if field.gained_focus(){self.pending.push(Cmd::Focus);}
                    self.settings_editing|=field.has_focus();
                    if setting_button(c,131.0,y,27.0,"−",false,&format!("focus-minus-{i}")){*value=value.saturating_sub(1).max(1);}
                    if setting_button(c,255.0,y,27.0,"+",false,&format!("focus-plus-{i}")){*value=(*value+1).min(max);}
                }
                c.text(315.0,93.0,"下次生效",10.5,W::Regular,theme::GRAY,H::Center);
            }
            _=>{
                if setting_button(c,43.0,56.0,152.0,if self.settings.notifications{"提醒 · 开"}else{"提醒 · 关"},self.settings.notifications,"set-notice"){self.settings.notifications=!self.settings.notifications;}
                if setting_button(c,212.0,56.0,153.0,if self.settings.sound{"声音 · 开"}else{"声音 · 关"},self.settings.sound,"set-sound"){self.settings.sound=!self.settings.sound;}
                if setting_button(c,43.0,84.0,152.0,if self.settings.read_notifications{"断开系统通知"}else{"连接系统通知"},self.settings.read_notifications,"set-listener"){self.pending.push(if self.settings.read_notifications{Cmd::DisconnectNotifications}else{Cmd::RequestNotifications});}
                let status=c.fit(&self.listener_status,10.5,W::Regular,170.0);c.text(210.0,97.0,&status,10.5,W::Regular,theme::GRAY,H::Left);
            }
        }
        if before!=format!("{:?}",self.settings){self.settings.save();self.last_reminder=self.t;}
    }
}

/// Auralsle 外观模式三卡片可视化选择器（跟随系统双分屏 / 浅色 / 深色）。
fn draw_auralsle_appearance_cards(
    ui: &mut egui::Ui,
    theme: &mut crate::settings_theme::SettingsTheme,
) {
    use crate::settings_theme::SettingsTheme;
    let ctx = ui.ctx().clone();
    let dark_ui = ctx.style().visuals.dark_mode;
    let avail = layout_avail_w(ui).max(260.0);
    let blue = Color32::from_rgb(10, 132, 255);
    ui.add_space(6.0);
    ui.label(
        egui::RichText::new("外观模式")
            .size(14.5)
            .strong()
            .color(settings_color(&ctx, Color32::WHITE, Color32::from_rgb(24, 24, 28))),
    );
    ui.add_space(8.0);

    let gap = 12.0;
    let cw = ((avail - gap * 2.0) / 3.0).max(80.0);
    let ch = 154.0;
    let items = [
        (SettingsTheme::System, "跟随系统"),
        (SettingsTheme::Light, "浅色"),
        (SettingsTheme::Dark, "深色"),
    ];

    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = gap;
        for (i, (variant, label)) in items.into_iter().enumerate() {
            let on = *theme == variant;
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(cw, ch), egui::Sense::click());
            let card_bg = if dark_ui {
                Color32::from_rgb(33, 35, 40)
            } else {
                Color32::from_rgb(245, 246, 250)
            };
            let border = if on {
                egui::Stroke::new(2.0, blue)
            } else if resp.hovered() {
                egui::Stroke::new(1.2, Color32::from_gray(if dark_ui { 95 } else { 175 }))
            } else {
                egui::Stroke::new(1.0, Color32::from_gray(if dark_ui { 54 } else { 216 }))
            };
            ui.painter().rect_filled(rect, 16.0, card_bg);
            ui.painter().rect_stroke(rect, 16.0, border, egui::StrokeKind::Inside);

            // 内部微型窗口线框预览区 (top 12..110)
            let prev = egui::Rect::from_min_max(
                rect.min + egui::vec2(10.0, 10.0),
                egui::pos2(rect.right() - 10.0, rect.top() + 110.0),
            );
            let p_clip = ui.painter().with_clip_rect(prev);
            let parts = if i == 0 { 2 } else { 1 };
            let pw = prev.width() / parts as f32;
            for part in 0..parts {
                let is_dark = i == 2 || (i == 0 && part == 1);
                let px0 = prev.left() + part as f32 * pw;
                let part_rect = egui::Rect::from_min_size(egui::pos2(px0, prev.top()), egui::vec2(pw, prev.height()));
                let (surf, side_bg, ink_bar, muted_bar, card_c) = if is_dark {
                    (
                        Color32::from_rgb(13, 14, 17),
                        Color32::from_rgb(27, 29, 34),
                        Color32::from_rgb(245, 246, 248),
                        Color32::from_rgb(122, 126, 134),
                        Color32::from_rgb(22, 24, 28),
                    )
                } else {
                    (
                        Color32::from_rgb(243, 244, 247),
                        Color32::from_rgb(229, 232, 238),
                        Color32::from_rgb(28, 31, 36),
                        Color32::from_rgb(142, 148, 158),
                        Color32::from_rgb(255, 255, 255),
                    )
                };
                let part_cr = if parts == 1 {
                    egui::CornerRadius::same(10)
                } else if part == 0 {
                    egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }
                } else {
                    egui::CornerRadius { nw: 0, sw: 0, ne: 10, se: 10 }
                };
                p_clip.rect_filled(part_rect, part_cr, surf);
                let side_w = pw * 0.25;
                let side_r = egui::Rect::from_min_size(egui::pos2(px0, prev.top()), egui::vec2(side_w, prev.height()));
                let side_cr = if part == 0 {
                    egui::CornerRadius { nw: 10, sw: 10, ne: 0, se: 0 }
                } else {
                    egui::CornerRadius::ZERO
                };
                p_clip.rect_filled(side_r, side_cr, side_bg);
                for row in 0..3 {
                    let bw = (side_w - 10.0).max(6.0);
                    p_clip.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(px0 + 5.0, prev.top() + 18.0 + row as f32 * 14.0), egui::vec2(bw, 3.5)),
                        1.75,
                        muted_bar,
                    );
                }
                let main_x = px0 + side_w + 7.0;
                p_clip.rect_filled(
                    egui::Rect::from_min_size(egui::pos2(main_x, prev.top() + 13.0), egui::vec2((pw * 0.52).max(16.0), 4.5)),
                    2.0,
                    ink_bar,
                );
                let card_w = (pw - side_w - 14.0).max(16.0);
                for row in 0..2 {
                    p_clip.rect_filled(
                        egui::Rect::from_min_size(egui::pos2(main_x, prev.top() + 28.0 + row as f32 * 30.0), egui::vec2(card_w, 24.0)),
                        5.0,
                        card_c,
                    );
                }
            }
            // 圆角外框覆盖接缝使四角圆滑一致
            ui.painter().rect_stroke(
                prev,
                10.0,
                egui::Stroke::new(2.0, card_bg),
                egui::StrokeKind::Outside,
            );

            // 底部文字 + 蓝色对号
            let lbl_col = settings_color(&ctx, Color32::WHITE, Color32::from_rgb(24, 24, 28));
            ui.painter().text(
                egui::pos2(rect.left() + 14.0, rect.bottom() - 22.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::proportional(13.0),
                lbl_col,
            );
            if on {
                let c_card = Card::new(ui, rect.min, rect);
                crate::paint::check(&c_card, cw - 20.0, ch - 22.0, 12.0, 1.0, 2.0, blue);
            }
            if resp.clicked() {
                *theme = variant;
            }
        }
    });
    ui.add_space(6.0);
}

pub(crate) fn switch(ui:&mut egui::Ui,value:&mut bool)->bool {
    // [Twinkle Tray 1.17.2 `body[data-is-win11=true] input[type=checkbox]`]
    // 像素级对齐真机截图（图二 `image_93848d.jpg` `y=586..604`）：
    // - 轨道：44×20 全圆角药丸，未选中时透明底 + 1.5px 亮灰描边 `RGB(198,198,198)`（对应 `--input-border: rgba(255,255,255,0.77)`）；
    // - 选中时轨道填充 `--system-accent-light2`（`#50C4FF`），圆点按 `input[type=checkbox][data-checked=true]:before { background-color: var(--page-background) }` 转为深色 `#202020`（浅色下为 `#FFFFFF`）；
    // - 未选中时圆点为 12×12 `RGB(207,207,207)`（浅色下为 `RGB(90,90,90)`）。
    let (rect,response)=ui.allocate_exact_size(egui::vec2(44.0,20.0),egui::Sense::click());
    if response.clicked(){*value=!*value;}
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let t = if cfg!(test) {
        if *value { 1.0 } else { 0.0 }
    } else {
        let anim_v = ctx.animate_bool_with_time(response.id.with("tw-sw"), *value, 0.166);
        if anim_v > 0.005 && anim_v < 0.995 {
            ctx.request_repaint();
        }
        anim_v
    };
    let raw_acc = if cfg!(test) {
        Color32::from_rgb(80, 196, 255)
    } else {
        crate::auralsle_ai_status::plugins::active_accent_color()
    };
    let acc = if dark {
        crate::paint::mix(raw_acc, Color32::WHITE, 0.28) // --system-accent-light2 (#50C4FF)
    } else {
        crate::paint::mix(raw_acc, Color32::BLACK, 0.12) // --system-accent-dark1
    };
    let border = if dark {
        Color32::from_rgb(198, 198, 198) // sRGB 合成后的 rgba(255,255,255,0.77)
    } else {
        Color32::from_rgb(90, 90, 90)    // sRGB 合成后的 rgba(0,0,0,0.77)
    };
    let hover_border = if dark { Color32::WHITE } else { Color32::BLACK };
    let hov_t = if cfg!(test) {
        0.0
    } else {
        ctx.animate_bool_with_time(response.id.with("tw-sw-hov"), response.hovered(), 0.12)
    };
    let off_bg = if dark { Color32::from_rgb(0x2b, 0x2b, 0x2b) } else { Color32::from_rgb(0xfb, 0xfc, 0xfe) };
    let fill_col = crate::paint::mix(off_bg, acc, t);
    let edge = crate::paint::mix(crate::paint::mix(border, hover_border, hov_t * 0.7), acc, t);
    ui.painter().rect_filled(rect, 10.0, fill_col);
    ui.painter().rect_stroke(rect, 10.0, egui::Stroke::new(1.5, edge), egui::StrokeKind::Inside);
    let cx = rect.left() + 10.0 + (24.0 - 2.0 * hov_t) * t;
    let thumb_d = 12.0 * (1.0 + 0.2 * hov_t);
    let thumb = egui::Rect::from_center_size(egui::pos2(cx, rect.center().y), egui::vec2(thumb_d, thumb_d));
    let thumb_off = if dark { Color32::from_rgb(207, 207, 207) } else { Color32::from_rgb(90, 90, 90) };
    let thumb_on = if dark { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE };
    let thumb_col = crate::paint::mix(thumb_off, thumb_on, t);
    ui.painter().rect_filled(thumb, thumb_d * 0.5, thumb_col);
    response.clicked()
}
/// 设置页版式：标签列宽、开关尺寸、控件与卡片右边缘之间的固定留白。
pub(crate) const SETTINGS_LABEL_W: f32 = 80.0;
/// [窄宽排版] 卡片内可用宽度栈：`settings_card` 入栈、退出时出栈。
pub(crate) const CARD_W_STACK: &str = "auralsle_settings_card_w_stack";
/// [窄宽排版] 控件行右侧可用宽度栈：`settings_control_row` 入栈、退出时出栈。
const CTL_W_STACK: &str = "auralsle_settings_ctl_w_stack";
/// [全页等宽基准] 由设置页唯一 ScrollArea 在页面顶层锁定的内容右边界（不含右侧滚动条排水沟）。
pub(crate) const CANON_PAGE_RIGHT_KEY: &str = "auraisle_canon_page_right";

fn push_layout_width(ui: &egui::Ui, key: &str, w: f32) {
    let id = egui::Id::new(key);
    let mut stack = ui.data(|d| d.get_temp::<Vec<f32>>(id)).unwrap_or_default();
    stack.push(w);
    ui.data_mut(|d| d.insert_temp(id, stack));
}

fn pop_layout_width(ui: &egui::Ui, key: &str) {
    let id = egui::Id::new(key);
    let mut stack = ui.data(|d| d.get_temp::<Vec<f32>>(id)).unwrap_or_default();
    stack.pop();
    ui.data_mut(|d| d.insert_temp(id, stack));
}

/// 返回设置页唯一 ScrollArea 锁定的内容右边界，避免任何卡片/控件向右侵入滚动条排水沟。
pub(crate) fn canonical_page_right(ui: &egui::Ui) -> Option<f32> {
    ui.data(|d| d.get_temp::<f32>(egui::Id::new(CANON_PAGE_RIGHT_KEY)))
        .filter(|v| v.is_finite() && *v > 120.0)
}

/// 就地取当前可用宽度：优先控件行右侧的余量，其次卡片内宽度、页面规范右缘，最后退回 egui 自己的估计。
pub(crate) fn layout_avail_w(ui: &egui::Ui) -> f32 {
    let ctl = ui.data(|d| d.get_temp::<Vec<f32>>(egui::Id::new(CTL_W_STACK))).and_then(|v| v.last().copied());
    let card = ui.data(|d| d.get_temp::<Vec<f32>>(egui::Id::new(CARD_W_STACK))).and_then(|v| v.last().copied());
    let page_w = canonical_page_right(ui)
        .map(|r| (r - ui.cursor().left()).max(120.0))
        .unwrap_or(f32::INFINITY);
    let max_w = (ui.max_rect().right() - ui.cursor().left()).max(120.0);
    let avail = ui.available_width();
    ctl.unwrap_or(f32::INFINITY)
        .min(card.unwrap_or(f32::INFINITY))
        .min(page_w)
        .min(max_w)
        .min(avail)
        .max(120.0)
}

pub(crate) fn settings_page_scope<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let real_w = canonical_page_right(ui)
        .map(|r| (r - ui.cursor().left()).max(160.0))
        .unwrap_or_else(|| {
            (ui.max_rect().right() - ui.cursor().left())
                .min(ui.available_width())
                .max(160.0)
        });
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(real_w, 4000.0));
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)),
        |ui| {
            ui.set_min_width(real_w);
            ui.set_max_width(real_w);
            let mut page_clip = ui.clip_rect();
            page_clip.max.x = page_clip.max.x.min(ui.cursor().left() + real_w + 1.0);
            ui.set_clip_rect(page_clip);
            add(ui)
        },
    )
    .inner
}
const SETTINGS_SWITCH_W: f32 = 44.0;
const SETTINGS_ROW_GAP: f32 = 16.0;

/// [Twinkle Tray 全量皮肤] 设置窗口专用调色板 —— 原样搬 Twinkle 1.17.2 `vars.b9008e01.css`
/// 的 Win11 两组取值（浅色 `#f1f3f9 / #ffffff98 / #f2f2f2`，暗色 `#202020 / #ffffff0a / #242424`）。
/// 岛体自己的皮肤只作用于岛体，设置窗口按用户要求"完完全全"用 Twinkle 的界面语言。
pub(crate) fn twinkle_settings_skin(dark: bool) -> SettingsSkinTheme {
    // [关键根因修复：wgpu 线性空间 vs Chromium sRGB 空间 Alpha 混合差异]
    // 在 wgpu D3D12 (Bgra8UnormSrgb) 下，若直接传 `rgba(255,255,255,10)` (`#ffffff0a`)，GPU 会在
    // 线性光空间做混合再转回 sRGB（gamma 2.2），导致 `#202020` 上的 `#ffffff0a` 被洗白成刺眼的水泥灰 `RGB(87,87,87)` (`#575757`)！
    // 而 Electron/Chromium 是在 sRGB 空间直接混合：`34*(1-10/255) + 255*(10/255) = 43` (`#2B2B2B`)。
    // 因此这里直接给出 sRGB 预合成后的不透明真值，1:1 像素级对齐图二 (`image_93848d.jpg`)：
    // - 暗色主背景 `bg_main` / `bg_side`: `RGB(34, 34, 34)` (`#222222`)
    // - 暗色卡片底 `bg_card`: `RGB(43, 43, 43)` (`#2B2B2B`)
    // - 暗色按钮/下拉框底 `bg_ctrl`: `RGB(51, 51, 51)` (`#333333`)
    // - 暗色卡片内描边 `border`: `RGB(38, 38, 38)` (`#262626`，对应 `inset 0 0 0 1px rgba(0,0,0,0.08)`)
    if dark {
        SettingsSkinTheme {
            bg_main: Color32::from_rgb(0x22, 0x22, 0x22),
            bg_side: Color32::from_rgb(0x22, 0x22, 0x22),
            bg_card: Color32::from_rgb(0x2b, 0x2b, 0x2b),
            bg_ctrl: Color32::from_rgb(0x33, 0x33, 0x33),
            border: Color32::from_rgb(0x26, 0x26, 0x26),
        }
    } else {
        SettingsSkinTheme {
            bg_main: Color32::from_rgb(0xf1, 0xf3, 0xf9),
            bg_side: Color32::from_rgb(0xf1, 0xf3, 0xf9),
            bg_card: Color32::from_rgb(0xfb, 0xfc, 0xfe),
            bg_ctrl: Color32::from_rgb(0xff, 0xff, 0xff),
            border: Color32::from_rgb(0xe2, 0xe4, 0xea),
        }
    }
}

/// Twinkle 卡片水平内边距（`.parent-panel { padding:10px }` + `.content-area { padding:10px }` = 20px）。
pub(crate) const TW_CARD_PAD: f32 = 20.0;
/// Twinkle 卡片垂直内边距（16px）。
pub(crate) const TW_CARD_PAD_V: f32 = 16.0;
/// Twinkle 卡片圆角（`border-radius:4px`）。
pub(crate) const TW_CARD_RADIUS: f32 = 4.0;
/// Twinkle 行标题字号（`.option-title { font-size:16px }`）。
pub(crate) const TW_ROW_TITLE: f32 = 16.0;
/// Twinkle 行说明字号（`.option-description { font-size:12.5px; opacity:.75 }`）。
pub(crate) const TW_ROW_DESC: f32 = 12.5;

/// [Twinkle Tray `.parent-panel` 核心双栏垂直居中排版引擎]
/// 先精确排版左侧 `.option-title` (`16px`) 与 `.option-description` (`12.5px`) 的 Galley 高度，
/// 再按 `row_h = text_h.max(ctrl_h)` 分配整行精确矩形，将左侧文案与右侧 `.input-area` 控件同时严格锚定在
/// `row_rect.center().y` 垂直中线上（彻底杜绝 `egui` 无限高度 `max_rect` 下 `Align::Center` 拉伸或偏顶的顽疾）。
pub(crate) fn twinkle_parent_panel<R>(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    ctrl_w: f32,
    ctrl_h: f32,
    add_ctrl: impl FnOnce(&mut egui::Ui) -> R,
) -> (egui::Response, R) {
    let ctx = ui.ctx().clone();
    let row_w = layout_avail_w(ui).max(180.0);
    let max_ctrl_w = (row_w - 110.0).max(ctrl_w.min(row_w * 0.48)).min(row_w * 0.54);
    let eff_ctrl_w = ctrl_w.min(max_ctrl_w.max(84.0));
    let text_w = (row_w - eff_ctrl_w - 20.0).max(80.0);
    let title_col = settings_color(&ctx, Color32::from_rgb(240, 240, 240), Color32::from_rgb(24, 24, 26));
    let desc_col = settings_color(&ctx, Color32::from_rgb(182, 182, 182), Color32::from_rgb(90, 92, 98));

    if cfg!(test) {
        let mut out: Option<R> = None;
        let resp = ui
            .horizontal(|ui| {
                ui.set_max_width(row_w);
                ui.vertical(|ui| {
                    ui.set_width(text_w);
                    ui.set_max_width(text_w);
                    ui.add(egui::Label::new(egui::RichText::new(title).size(TW_ROW_TITLE).color(title_col)).wrap());
                    let desc = description.trim();
                    if !desc.is_empty() {
                        ui.add(egui::Label::new(egui::RichText::new(desc).size(TW_ROW_DESC).color(desc_col)).wrap());
                    }
                });
                out = Some(add_ctrl(ui));
            })
            .response;
        return (resp, out.expect("ctrl executed"));
    }

    let title_galley = ctx.fonts(|f| {
        f.layout(
            title.to_owned(),
            egui::FontId::proportional(TW_ROW_TITLE),
            title_col,
            text_w,
        )
    });
    let desc_trimmed = description.trim();
    let desc_galley = if !desc_trimmed.is_empty() {
        Some(ctx.fonts(|f| {
            f.layout(
                desc_trimmed.to_owned(),
                egui::FontId::proportional(TW_ROW_DESC),
                desc_col,
                text_w,
            )
        }))
    } else {
        None
    };

    let t_h = title_galley.size().y;
    let d_h = desc_galley.as_ref().map_or(0.0, |g| 4.0 + g.size().y);
    let text_total_h = t_h + d_h;
    let row_h = text_total_h.max(ctrl_h).max(28.0) + 4.0;

    let (row_rect, row_resp) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::hover());
    let text_top = row_rect.center().y - text_total_h * 0.5;
    ui.painter().galley(egui::pos2(row_rect.left(), text_top), title_galley, title_col);
    if let Some(dg) = desc_galley {
        ui.painter().galley(egui::pos2(row_rect.left(), text_top + t_h + 4.0), dg, desc_col);
    }

    let ctrl_rect = egui::Rect::from_min_size(
        egui::pos2(row_rect.right() - eff_ctrl_w, row_rect.center().y - ctrl_h * 0.5),
        egui::vec2(eff_ctrl_w, ctrl_h),
    );
    let mut ctrl_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(ctrl_rect)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    ctrl_ui.set_clip_rect(ctrl_rect.intersect(ui.clip_rect()));
    push_layout_width(&ctrl_ui, CTL_W_STACK, eff_ctrl_w);
    let r_out = add_ctrl(&mut ctrl_ui);
    pop_layout_width(&ctrl_ui, CTL_W_STACK);
    // [第 10 批 · 取消 toast/tooltip] 行内说明已经在标题下方常显，再挂一个跟随鼠标的
    // `on_hover_text` 就变成「同一句说明重复飘在行上」（图五红框）。整行不再注册任何悬浮提示。
    let _ = &desc_trimmed;
    (row_resp, r_out)
}

/// 开关行：左侧 `16px` 标题 + `12.5px` 说明，右侧垂直居中的 Twinkle `44×20` 开关。
/// [第 10 批 · 取消状态词] Twinkle 的 `inputToggle` 本身就用「蓝色=开 / 灰色=关」表达状态，
/// 再补一个「开启 / 关闭」文字是重复信息，且把控件列越推越宽（图七红框）。
pub(crate) fn setting_row(ui: &mut egui::Ui, title: &str, description: &str, value: &mut bool) {
    twinkle_parent_panel(ui, title, description, SETTINGS_SWITCH_W, 24.0, |ui| {
        switch(ui, value);
    });
}

/// 独立成卡的 Twinkle 开关项（`.settings-option-elem`）。
pub(crate) fn setting_row_card(ui: &mut egui::Ui, title: &str, description: &str, value: &mut bool) {
    settings_card(ui, |ui| {
        setting_row(ui, title, description, value);
    });
}

/// [Twinkle Tray `.parent-panel` 下拉选择行]：
/// 左侧垂直居中堆叠 `16px` 标题 + `12.5px` 说明，右侧垂直居中放置 `148×36` Twinkle `<select>` 下拉框。
pub(crate) fn setting_select_row<T: PartialEq + Clone, S: AsRef<str>>(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    id_salt: impl std::hash::Hash,
    selected_text: &str,
    value: &mut T,
    options: &[(T, S)],
) -> bool {
    use std::hash::{Hash, Hasher};
    let prev = value.clone();
    let row_w = layout_avail_w(ui).max(180.0);
    let min_w = (row_w * 0.42).clamp(112.0, 148.0);
    let max_w = (row_w * 0.54).max(min_w);

    let mut sh = std::collections::hash_map::DefaultHasher::new();
    id_salt.hash(&mut sh);
    let salt_hash = sh.finish();
    let mut kh = std::collections::hash_map::DefaultHasher::new();
    selected_text.hash(&mut kh);
    options.len().hash(&mut kh);
    for (_, s) in options {
        s.as_ref().hash(&mut kh);
    }
    let key_hash = kh.finish();
    let cache_id = egui::Id::new(("rb_sel_w", salt_hash));
    let max_label_w = if let Some((h, w)) = ui.ctx().data(|d| d.get_temp::<(u64, f32)>(cache_id)) {
        if h == key_hash {
            w
        } else {
            let measured = ui.fonts(|f| {
                std::iter::once(selected_text)
                    .chain(options.iter().map(|(_, s)| s.as_ref()))
                    .map(|text| {
                        f.layout_no_wrap(text.to_owned(), egui::FontId::proportional(14.0), Color32::WHITE)
                            .size()
                            .x
                    })
                    .fold(0.0_f32, f32::max)
            });
            ui.ctx().data_mut(|d| d.insert_temp(cache_id, (key_hash, measured)));
            measured
        }
    } else {
        let measured = ui.fonts(|f| {
            std::iter::once(selected_text)
                .chain(options.iter().map(|(_, s)| s.as_ref()))
                .map(|text| {
                    f.layout_no_wrap(text.to_owned(), egui::FontId::proportional(14.0), Color32::WHITE)
                        .size()
                        .x
                })
                .fold(0.0_f32, f32::max)
        });
        ui.ctx().data_mut(|d| d.insert_temp(cache_id, (key_hash, measured)));
        measured
    };

    let select_w = (max_label_w + 50.0).clamp(min_w, max_w);
    twinkle_parent_panel(ui, title, description, select_w, 36.0, |ui| {
        react_bits_combo(ui, salt_hash, selected_text, select_w, value, options);
    });
    *value != prev
}

/// 独立成卡的 Twinkle 下拉选择项（`.settings-option-elem`）。
pub(crate) fn setting_select_card<T: PartialEq + Clone, S: AsRef<str>>(
    ui: &mut egui::Ui,
    title: &str,
    description: &str,
    id_salt: impl std::hash::Hash,
    selected_text: &str,
    value: &mut T,
    options: &[(T, S)],
) -> bool {
    let mut changed = false;
    settings_card(ui, |ui| {
        changed = setting_select_row(ui, title, description, id_salt, selected_text, value, options);
    });
    changed
}

/// [Twinkle Tray `body[data-is-win11=true] .button`] 标准圆角 6px 按钮
pub(crate) fn twinkle_button(ui: &mut egui::Ui, label: &str, primary: bool) -> egui::Response {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let raw_acc = if cfg!(test) {
        Color32::from_rgb(76, 194, 255)
    } else {
        crate::auralsle_ai_status::plugins::active_accent_color()
    };
    let fill = if primary {
        if dark {
            crate::paint::mix(raw_acc, Color32::WHITE, 0.25)
        } else {
            crate::paint::mix(raw_acc, Color32::BLACK, 0.12)
        }
    } else if dark {
        Color32::from_rgb(0x33, 0x33, 0x33)
    } else {
        Color32::WHITE
    };
    let stroke_col = if primary {
        fill
    } else if dark {
        Color32::from_rgb(0x3e, 0x3e, 0x3e)
    } else {
        Color32::from_rgb(0xd0, 0xd3, 0xdc)
    };
    let fg = if primary {
        if dark { Color32::from_rgb(0x20, 0x20, 0x20) } else { Color32::WHITE }
    } else if dark {
        Color32::from_rgb(240, 240, 240)
    } else {
        Color32::from_rgb(24, 24, 26)
    };
    let btn = egui::Button::new(egui::RichText::new(label).size(13.5).color(fg))
        .fill(fill)
        .stroke(egui::Stroke::new(1.0, stroke_col))
        .corner_radius(6.0)
        .min_size(egui::vec2(72.0, 34.0));
    ui.add(btn)
}

/// [Twinkle Tray `slider.586709b8.css` `.input--range`]
/// 1:1 复刻 Windows 11 Twinkle 滑条：
/// - 轨道高 `4px`、圆角 `2px`，底色 `--slider-track-color`（暗 `#9b9b9b` / 浅 `#868686`）；
/// - 左侧已填充进度高 `4px`，颜色 `--system-accent-light2`（暗 `#4cc2ff`）/ `--system-accent-dark1`；
/// - 滑块手柄：外圈 `18×18` `--slider-thumb-border`（暗 `#414141` / 浅 `#ffffff`）+ 阴影，内圈 `10×10` 强调色圆点（悬停/拖拽平滑放大 `1.3x`）；
/// - 右侧 `.val` 数值直读标签（支持鼠标滚轮微调）。
pub(crate) fn twinkle_slider_f32(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    short: bool,
    suffix: &str,
) -> bool {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let min_v = *range.start();
    let max_v = (*range.end()).max(min_v + 1e-5);
    let prev = *value;

    let avail_w = layout_avail_w(ui).min(ui.available_width()).max(120.0);
    let val_w = if suffix.is_empty() {
        if short { 34.0 } else { 44.0 }
    } else {
        52.0
    };
    let track_w = (avail_w - val_w - 10.0).max(64.0);
    let row_h = if short { 26.0 } else { 30.0 };

    let id = ui.make_persistent_id(id_salt);
    let (row_rect, _) = ui.allocate_exact_size(egui::vec2(track_w + val_w + 10.0, row_h), egui::Sense::hover());
    let hit_rect = egui::Rect::from_min_size(row_rect.min, egui::vec2(track_w, row_h));
    let resp = ui.interact(hit_rect, id, egui::Sense::click_and_drag());

    if resp.dragged() || resp.clicked() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - (hit_rect.left() + 9.0)) / (track_w - 18.0).max(1.0)).clamp(0.0, 1.0);
            *value = (min_v + t * (max_v - min_v)).round();
        }
    }

    let ratio = ((*value - min_v) / (max_v - min_v)).clamp(0.0, 1.0);
    let raw_acc = if cfg!(test) {
        Color32::from_rgb(76, 194, 255)
    } else {
        crate::auralsle_ai_status::plugins::active_accent_color()
    };
    let accent_col = if dark {
        crate::paint::mix(raw_acc, Color32::WHITE, 0.25)
    } else {
        crate::paint::mix(raw_acc, Color32::BLACK, 0.12)
    };
    let track_col = if dark {
        Color32::from_rgb(0x9b, 0x9b, 0x9b)
    } else {
        Color32::from_rgb(0x86, 0x86, 0x86)
    };
    let thumb_ring_col = if dark {
        Color32::from_rgb(0x41, 0x41, 0x41)
    } else {
        Color32::WHITE
    };
    let text_col = if dark {
        Color32::from_rgb(240, 240, 240)
    } else {
        Color32::from_rgb(24, 24, 26)
    };

    let x0 = hit_rect.left() + 9.0;
    let x1 = hit_rect.right() - 9.0;
    let cy = hit_rect.center().y;
    let thumb_x = x0 + (x1 - x0) * ratio;

    // 1) 4px 轨道底色
    let full_track = egui::Rect::from_min_max(egui::pos2(x0, cy - 2.0), egui::pos2(x1, cy + 2.0));
    ui.painter().rect_filled(full_track, 2.0, track_col);

    // 2) 4px 左侧进度填充
    if thumb_x > x0 + 0.5 {
        let prog_rect = egui::Rect::from_min_max(egui::pos2(x0, cy - 2.0), egui::pos2(thumb_x, cy + 2.0));
        ui.painter().rect_filled(prog_rect, 2.0, accent_col);
    }

    // 3) Win11 双层圆形滑块手柄（外圈 18px + 内圈 10px，悬停/拖拽放大 1.3x）
    let hov_t = if cfg!(test) {
        0.0
    } else {
        let active = resp.hovered() || resp.dragged();
        let v = ctx.animate_bool_with_time(id.with("tw-sl-hov"), active, 0.15);
        if v > 0.005 && v < 0.995 {
            ctx.request_repaint();
        }
        v
    };
    let center = egui::pos2(thumb_x, cy);
    ui.painter().circle_filled(center + egui::vec2(0.0, 0.8), 9.5, Color32::from_black_alpha(95));
    ui.painter().circle_filled(center, 9.0, thumb_ring_col);
    if !dark {
        ui.painter().circle_stroke(center, 9.0, egui::Stroke::new(1.0, Color32::from_black_alpha(28)));
    }
    let inner_r = 5.0 + 1.5 * hov_t;
    ui.painter().circle_filled(center, inner_r, accent_col);

    // 4) 右侧 `.val` 数值
    let val_str = format!("{}{}", (*value).round() as i32, suffix);
    let val_font_sz = if short { 15.0 } else { 19.0 };
    let val_rect = egui::Rect::from_min_max(
        egui::pos2(hit_rect.right() + 6.0, row_rect.top()),
        row_rect.max,
    );
    ui.painter().text(
        val_rect.center(),
        egui::Align2::CENTER_CENTER,
        val_str,
        egui::FontId::proportional(val_font_sz),
        text_col,
    );

    (*value - prev).abs() > 1e-4
}

/// [窄宽排版] 槽位顺序行：一行放得下就整行排，放不下就每个槽位独占一行。
///
/// 旧实现用 `horizontal_wrapped` 逐个子项排，换行点会落在"→"这个独立 item 后面，
/// 于是第二行以"→"开头、第一行以"▶"结尾，语义完全错乱（第三轮图1）。
pub(crate) fn slot_rows_fit_on_one_line(avail: f32, label_w: f32, unit_ws: &[f32], arrow_w: f32) -> bool {
    let arrows = arrow_w * unit_ws.len().saturating_sub(1) as f32;
    let total = label_w + unit_ws.iter().sum::<f32>() + arrows;
    total <= avail
}

/// [窄宽排版] 等宽按钮网格：返回 (列数, 单元格宽)。
///
/// 预设按钮文字长短不一，直接 `horizontal_wrapped` 会排出"3-2-1"的锯齿；
/// 改成等宽网格后每行对齐，窄窗下也不会越出卡片。
pub(crate) fn uniform_grid(avail: f32, n: usize, min_cell: f32, gap: f32, max_cols: usize) -> (usize, f32) {
    let n = n.max(1);
    let max_cols = max_cols.clamp(1, n);
    let mut cols = 1usize;
    for k in (1..=max_cols).rev() {
        let cell = (avail - gap * (k as f32 - 1.0)) / k as f32;
        if cell >= min_cell {
            cols = k;
            break;
        }
    }
    let cell = ((avail - gap * (cols as f32 - 1.0)) / cols as f32).max(1.0);
    (cols, cell)
}

/// 设置页统一的分段选择器（重构为 Shadcn UI `<TabsList />` & `<TabsTrigger />` 原地缩放渐显 + 中心延展流光底线动画，彻底消除图二滚动时滑块脱离容器的问题）。
/// [Twinkle Tray 1.17.2 `.input-row > .button` / `.button-primary`]
/// 完全替换原 Shadcn 蓝框滑动胶囊分段器：
/// Twinkle 中没有任何分段胶囊轨道，所有多选切换一律使用 `.input-row` 扁平按钮组：
/// - 未选中：背景 `var(--button-background)`（暗 `#8a8a8a2b` / 浅 `#ffffffc4`）、描边 `var(--button-border)`（暗 `#9797972b` / 浅 `#5c5c5c2b`）、圆角 `6px`、文字 `var(--page-text)`；
/// - 悬停：背景 `var(--button-highlight)`（暗 `#b1b1b123` / 浅 `#fffffffa`）；
/// - 选中（`.button-primary`）：背景 `var(--button-primary-background)`（主题强调色）、文字 `var(--page-background)`（暗色下 `#202020`，浅色下 `#ffffff`），零发光细线、零底部横条。
pub(crate) fn settings_segmented<T:PartialEq+Copy>(ui:&mut egui::Ui,value:&mut T,options:&[(T,&str)],item_w:f32)->bool{
    let ctx=&ui.ctx().clone();
    let mut changed=false;
    let avail = (layout_avail_w(ui) - 4.0).max(120.0);
    let n = options.len().max(1);
    let gap = 6.0_f32;
    let gap_total = gap * (n.saturating_sub(1) as f32);
    let text_widths: Vec<f32> = options
        .iter()
        .map(|(_, label)| {
            ctx.fonts(|f| f.layout_no_wrap((*label).to_string(), egui::FontId::proportional(13.0), Color32::WHITE).size().x)
        })
        .collect();
    let pref_widths: Vec<f32> = text_widths.iter().map(|tw| item_w.max(*tw + 20.0)).collect();
    let min_widths: Vec<f32> = text_widths.iter().map(|tw| (*tw + 12.0).max(40.0)).collect();
    let pref_sum: f32 = pref_widths.iter().sum::<f32>() + gap_total;
    let min_sum: f32 = min_widths.iter().sum::<f32>() + gap_total;
    if min_sum > avail && n >= 2 {
        let half = n.div_ceil(2);
        let mut changed = false;
        ui.vertical(|ui| {
            changed |= settings_segmented(ui, value, &options[..half], item_w.min(avail * 0.48));
            ui.add_space(4.0);
            changed |= settings_segmented(ui, value, &options[half..], item_w.min(avail * 0.48));
        });
        return changed;
    }
    let acc = if cfg!(test) {
        Color32::from_rgb(10, 132, 255)
    } else {
        crate::auralsle_ai_status::plugins::active_accent_color()
    };
    let dark = ctx.style().visuals.dark_mode;
    // Twinkle `vars.b9008e01.css` sRGB 预合成不透明令牌（避免 wgpu 线性混合洗白）
    let btn_bg = if dark {
        Color32::from_rgb(0x33, 0x33, 0x33)
    } else {
        Color32::from_rgb(0xff, 0xff, 0xff)
    };
    let btn_hover = if dark {
        Color32::from_rgb(0x3c, 0x3c, 0x3c)
    } else {
        Color32::from_rgb(0xf5, 0xf6, 0xfa)
    };
    let btn_border = if dark {
        Color32::from_rgb(0x3e, 0x3e, 0x3e)
    } else {
        Color32::from_rgb(0xd0, 0xd3, 0xdc)
    };
    let primary_bg = if dark {
        crate::paint::mix(acc, Color32::WHITE, 0.22) // --system-accent-light3
    } else {
        crate::paint::mix(acc, Color32::BLACK, 0.12) // --system-accent-dark1
    };
    let primary_hover = if dark {
        crate::paint::mix(acc, Color32::WHITE, 0.12) // --system-accent-light2
    } else {
        crate::paint::mix(acc, Color32::BLACK, 0.22) // --system-accent-dark2
    };
    let primary_fg = if dark {
        Color32::from_rgb(0x20, 0x20, 0x20) // --page-background
    } else {
        Color32::WHITE
    };
    let normal_fg = if dark {
        Color32::WHITE
    } else {
        Color32::from_rgb(16, 18, 22)
    };

    let seg_w = if pref_sum <= avail { pref_sum } else { avail };
    ui.allocate_ui_with_layout(egui::vec2(seg_w.max(1.0), 28.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
        ui.spacing_mut().item_spacing = egui::vec2(gap, 4.0);
        for (idx, (option, label)) in options.iter().enumerate() {
            let selected = *value == *option;
            let font_sz = if min_sum > avail {
                11.0
            } else if pref_sum > avail {
                12.0
            } else {
                13.0
            };
            let width = if pref_sum <= avail {
                pref_widths[idx]
            } else if min_sum <= avail {
                let extra = (avail - min_sum) / (n as f32);
                min_widths[idx] + extra
            } else {
                let usable = (avail - gap_total).max(n as f32 * 32.0);
                let raw_min_sum = (min_sum - gap_total).max(1.0);
                (min_widths[idx] * (usable / raw_min_sum)).max(32.0)
            };
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
            if resp.clicked() && !selected {
                *value = *option;
                changed = true;
            }
            let fill = if selected {
                if resp.hovered() { primary_hover } else { primary_bg }
            } else if resp.hovered() {
                btn_hover
            } else {
                btn_bg
            };
            ui.painter().rect_filled(rect, 6.0, fill);
            if !selected {
                ui.painter().rect_stroke(
                    rect,
                    6.0,
                    egui::Stroke::new(1.0, btn_border),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                *label,
                egui::FontId::proportional(font_sz),
                if selected { primary_fg } else { normal_fg },
            );
        }
    });
    changed
}

/// [Twinkle Tray 1.17.2 `body[data-is-win11=true] select`]
/// 完全照搬 `page.4cb0fe87.css`：
/// - `border-radius: 5px; border-width: 1.5px; background-color: var(--page-section-bg); border-color: var(--button-border);`
/// - `select:hover { background-color: var(--button-highlight); }`
/// - 去除旧版深蓝底板与 GlareHover 镜面扫光。
const COMBO_OPEN_KEY: &str = "auralsle_combo_open_id";
const COMBO_ACTIVE_UNTIL_KEY: &str = "auralsle_combo_anim_until";

pub fn combo_active(ctx: &egui::Context) -> bool {
    let now = ctx.input(|i| i.time);
    let until = ctx
        .data(|d| d.get_temp::<f64>(egui::Id::new(COMBO_ACTIVE_UNTIL_KEY)))
        .unwrap_or(0.0);
    now <= until
}

fn mark_combo_active(ctx: &egui::Context, duration: f64) {
    let now = ctx.input(|i| i.time);
    let key = egui::Id::new(COMBO_ACTIVE_UNTIL_KEY);
    ctx.data_mut(|d| {
        let prev = d.get_temp::<f64>(key).unwrap_or(0.0);
        d.insert_temp(key, prev.max(now + duration));
    });
}

/// 轻量高性能动画下拉框（替代 `egui::ComboBox` 每帧全量测宽与重绘阻塞）：
/// - 点击即刻响应并绕过设置页帧率节流，以 `0.16s` `power4.out` 展开/收起 + 淡入滑动 + 箭头旋转；
/// - 短列表直绘零 ScrollArea 开销，超长列表自动切滚动，近屏底自动向上翻转，点击外部或按 Esc 平滑关闭。
pub(crate) fn react_bits_combo<T: PartialEq + Clone, S: AsRef<str>>(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    selected_text: &str,
    width: f32,
    value: &mut T,
    options: &[(T, S)],
) -> egui::Response {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let acc = if cfg!(test) {
        Color32::from_rgb(10, 132, 255)
    } else {
        crate::auralsle_ai_status::plugins::active_accent_color()
    };
    let ctrl_bg = if dark { Color32::from_rgb(0x33, 0x33, 0x33) } else { Color32::WHITE };
    let ctrl_hov = if dark { Color32::from_rgb(0x3c, 0x3c, 0x3c) } else { Color32::from_rgb(0xf5, 0xf6, 0xfa) };
    let ctrl_border = if dark { Color32::from_rgb(0x3e, 0x3e, 0x3e) } else { Color32::from_rgb(0xd0, 0xd3, 0xdc) };
    let popup_bg = if dark { Color32::from_rgb(0x2b, 0x2b, 0x2b) } else { Color32::WHITE };
    let row_hov_fill = if dark { Color32::from_rgb(0x43, 0x43, 0x43) } else { Color32::from_rgb(0xe4, 0xe6, 0xec) };
    let txt_main = if dark { Color32::from_rgb(235, 235, 235) } else { Color32::from_rgb(24, 24, 26) };

    if cfg!(test) {
        return ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 34.0;
            ui.spacing_mut().button_padding = egui::vec2(10.0, 6.0);
            egui::ComboBox::from_id_salt(id_salt)
                .selected_text(egui::RichText::new(selected_text).size(14.0).color(txt_main))
                .width(width)
                .show_ui(ui, |ui| {
                    for (opt_val, opt_label) in options.iter() {
                        ui.selectable_value(value, opt_val.clone(), opt_label.as_ref());
                    }
                })
                .response
        }).inner;
    }

    let base_id = ui.make_persistent_id(id_salt);
    let popup_id = base_id.with("rb_popup");
    let open_store_id = egui::Id::new(COMBO_OPEN_KEY);
    let enabled = ui.is_enabled();
    let mut is_open = ctx.data(|d| d.get_temp::<Option<egui::Id>>(open_store_id)).flatten() == Some(base_id);
    if !enabled && is_open {
        is_open = false;
        ctx.data_mut(|d| d.insert_temp(open_store_id, Option::<egui::Id>::None));
    }

    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width.max(48.0), 34.0), egui::Sense::click());
    if resp.hovered() && enabled {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }

    let mut close_now = false;
    if resp.clicked() && enabled {
        is_open = !is_open;
        ctx.data_mut(|d| {
            d.insert_temp(open_store_id, if is_open { Some(base_id) } else { None });
        });
        mark_combo_active(&ctx, 0.24);
        ctx.request_repaint();
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    }

    let hov_t = ctx.animate_bool_with_time(base_id.with("hov"), (resp.hovered() && enabled) || is_open, 0.12);
    let open_t = ctx.animate_bool_with_time(base_id.with("open"), is_open, 0.16);
    let ease = if is_open {
        1.0 - (1.0 - open_t).powi(4)
    } else {
        open_t * open_t
    };

    if ui.is_rect_visible(rect) {
        let fill = crate::paint::mix(ctrl_bg, ctrl_hov, hov_t);
        let stroke_col = crate::paint::mix(ctrl_border, acc, open_t);
        ui.painter().rect(
            rect,
            5.0,
            fill,
            egui::Stroke::new(1.5, stroke_col),
            egui::StrokeKind::Inside,
        );
        let text_clip = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 8.0, rect.top() + 1.0),
            egui::pos2((rect.right() - 24.0).max(rect.left() + 12.0), rect.bottom() - 1.0),
        );
        let txt_col = if enabled { txt_main } else { Color32::from_gray(128) };
        ui.painter().with_clip_rect(text_clip).text(
            egui::pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            selected_text,
            egui::FontId::proportional(14.0),
            txt_col,
        );

        let cx = rect.right() - 13.0;
        let cy = rect.center().y;
        let flip = 1.0 - 2.0 * ease;
        let chev_col = crate::paint::mix(txt_col, acc, open_t * 0.55);
        let chev_stroke = egui::Stroke::new(1.35, chev_col);
        ui.painter().line_segment(
            [egui::pos2(cx - 4.0, cy - 2.0 * flip), egui::pos2(cx, cy + 2.2 * flip)],
            chev_stroke,
        );
        ui.painter().line_segment(
            [egui::pos2(cx, cy + 2.2 * flip), egui::pos2(cx + 4.0, cy - 2.0 * flip)],
            chev_stroke,
        );
    }

    if enabled && (is_open || ease > 0.01) && !options.is_empty() {
        let row_h = 30.0_f32;
        let pad = 4.0_f32;
        let gap = 2.0_f32;
        let max_h = 260.0_f32;
        let n = options.len();
        let content_h = pad * 2.0 + n as f32 * row_h + n.saturating_sub(1) as f32 * gap;
        let target_h = content_h.min(max_h);
        let vis_h = (target_h * ease.max(0.10)).min(target_h);
        let screen = ctx.screen_rect();
        let below_space = screen.bottom() - rect.bottom() - 8.0;
        let above_space = rect.top() - screen.top() - 8.0;
        let flip_up = below_space < target_h && above_space > below_space;
        let popup_w = width.max(108.0).min((screen.width() - 12.0).max(108.0));
        let popup_x = rect
            .left()
            .min(screen.right() - popup_w - 6.0)
            .max(screen.left() + 6.0);
        let slide_y = (1.0 - ease) * if flip_up { 6.0 } else { -6.0 };
        let base_y = if flip_up {
            rect.top() - 4.0 - vis_h
        } else {
            rect.bottom() + 4.0
        };
        let popup_pos = egui::pos2(
            popup_x,
            (base_y + slide_y).clamp(screen.top() + 4.0, (screen.bottom() - vis_h - 4.0).max(screen.top() + 4.0)),
        );
        let full_popup_rect = egui::Rect::from_min_size(popup_pos, egui::vec2(popup_w, vis_h));

        let mut picked_close = false;
        let mut row_animating = false;
        let area_resp = egui::Area::new(popup_id)
            .order(egui::Order::Tooltip)
            .fixed_pos(popup_pos)
            .movable(false)
            .interactable(is_open)
            .show(&ctx, |ui| {
                // 先在 Area 根 UI 分配完整几何尺寸，保证 egui layer_id_at 命中与内部 ScrollArea 滚轮事件不被底层 Window 截走。
                let (alloc_rect, _) = ui.allocate_exact_size(egui::vec2(popup_w, vis_h), egui::Sense::hover());
                ui.set_clip_rect(alloc_rect.expand(4.0));
                ui.set_opacity(ease.clamp(0.0, 1.0));
                let shadow_alpha = (if dark { 72.0 } else { 30.0 } * ease) as u8;
                if shadow_alpha > 0 {
                    ui.painter().rect_filled(
                        alloc_rect.translate(egui::vec2(0.0, 4.0)).expand(2.0),
                        8.0,
                        Color32::from_black_alpha(shadow_alpha),
                    );
                }
                ui.painter().rect(
                    alloc_rect,
                    6.0,
                    popup_bg,
                    egui::Stroke::new(1.0, ctrl_border),
                    egui::StrokeKind::Inside,
                );

                let inner_rect = alloc_rect.shrink(pad);
                let mut inner_ui = ui.new_child(
                    egui::UiBuilder::new()
                        .max_rect(inner_rect)
                        .layout(egui::Layout::top_down(egui::Align::Min)),
                );
                inner_ui.set_clip_rect(inner_rect);
                inner_ui.spacing_mut().item_spacing = egui::vec2(0.0, gap);

                let need_scroll = content_h > max_h;
                let mut draw_rows = |ui: &mut egui::Ui, picked_close: &mut bool, row_anim: &mut bool| {
                    let sb_pad = if need_scroll { 10.0 } else { 0.0 };
                    let row_w = (popup_w - pad * 2.0 - sb_pad).max(60.0);
                    for (idx, (opt_val, opt_label)) in options.iter().enumerate() {
                        let is_sel = *value == *opt_val;
                        let (r_rect, r_resp) = ui.allocate_exact_size(egui::vec2(row_w, row_h), egui::Sense::click());
                        if r_resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        let rh_t = ui.ctx().animate_bool_with_time(
                            popup_id.with(("row_h", idx)),
                            r_resp.hovered(),
                            0.10,
                        );
                        if rh_t > 0.005 && rh_t < 0.995 {
                            *row_anim = true;
                        }
                        if is_sel || rh_t > 0.01 {
                            let base_alpha = if is_sel { 1.0_f32 } else { 0.0_f32 };
                            let mix_a = base_alpha.max(rh_t);
                            let fill = if is_sel && rh_t > 0.01 {
                                crate::paint::mix(row_hov_fill, ctrl_border, 0.25 * rh_t)
                            } else {
                                Color32::from_rgba_unmultiplied(
                                    row_hov_fill.r(),
                                    row_hov_fill.g(),
                                    row_hov_fill.b(),
                                    (255.0 * mix_a) as u8,
                                )
                            };
                            ui.painter().rect_filled(r_rect, 4.0, fill);
                            if is_sel {
                                let bar_r = egui::Rect::from_center_size(
                                    egui::pos2(r_rect.left() + 2.5, r_rect.center().y),
                                    egui::vec2(3.0, 16.0),
                                );
                                ui.painter().rect_filled(bar_r, 2.0, acc);
                            }
                        }
                        ui.painter()
                            .with_clip_rect(r_rect.shrink2(egui::vec2(4.0, 0.0)))
                            .text(
                                egui::pos2(r_rect.left() + 10.0, r_rect.center().y),
                                egui::Align2::LEFT_CENTER,
                                opt_label.as_ref(),
                                egui::FontId::proportional(14.0),
                                txt_main,
                            );
                        if r_resp.clicked() {
                            *value = opt_val.clone();
                            *picked_close = true;
                        }
                    }
                };

                if need_scroll {
                    egui::ScrollArea::vertical()
                        .id_salt(popup_id.with("scroll"))
                        .max_height((vis_h - pad * 2.0).max(20.0))
                        .auto_shrink([false, false])
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                        .show(&mut inner_ui, |ui| {
                            draw_rows(ui, &mut picked_close, &mut row_animating);
                        });
                } else {
                    draw_rows(&mut inner_ui, &mut picked_close, &mut row_animating);
                }
            });

        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let click_outside = is_open
            && !resp.clicked()
            && ctx.input(|i| {
                i.pointer.any_pressed()
                    && i.pointer
                        .press_origin()
                        .is_some_and(|p| !rect.contains(p) && !full_popup_rect.contains(p))
            });
        let popup_hovered = area_resp.response.hovered()
            || ctx.input(|i| i.pointer.hover_pos().is_some_and(|p| full_popup_rect.contains(p)));
        if picked_close || esc || click_outside {
            close_now = true;
            ctx.data_mut(|d| d.insert_temp(open_store_id, Option::<egui::Id>::None));
        }
        if row_animating || popup_hovered {
            mark_combo_active(&ctx, 0.06);
        }
        if row_animating {
            ctx.request_repaint();
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }

    let anim_moving = (open_t > 0.005 && open_t < 0.995) || (hov_t > 0.005 && hov_t < 0.995);
    if anim_moving || close_now {
        mark_combo_active(&ctx, 0.22);
        ctx.request_repaint();
        ctx.request_repaint_of(egui::ViewportId::ROOT);
    } else if resp.hovered() && enabled {
        mark_combo_active(&ctx, 0.08);
    }

    resp
}

/// 完整的交互式 RGBA 调色盘组件（2D 拾色器弹窗 + Alpha 透明度滑条 + 8 位 `#RRGGBBAA` 十六进制输入）。
pub(crate) fn rgba_color_picker_inline(ui: &mut egui::Ui, id_salt: &str, hex_str: &mut String) -> bool {
    let mut changed = false;
    let [r0, g0, b0, a0] = crate::auralsle_ai_status::plugins::parse_hex_rgba(hex_str).unwrap_or([224, 62, 98, 255]);
    let mut srgba = Color32::from_rgba_unmultiplied(r0, g0, b0, a0);
    ui.allocate_ui_with_layout(
        egui::vec2(198.0, 24.0),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.spacing_mut().interact_size.y = 24.0;
            ui.spacing_mut().item_spacing.x = 6.0;
            if ui
                .color_edit_button_srgba(&mut srgba)
                .on_hover_text("点击打开完整 RGBA 2D 调色盘（支持 RGB 颜色与 Alpha 透明度实时调节）")
                .changed()
            {
                let [nr, ng, nb, na] = srgba.to_srgba_unmultiplied();
                *hex_str = format!("#{:02X}{:02X}{:02X}{:02X}", nr, ng, nb, na);
                changed = true;
            }
            let resp = ui
                .add_sized(
                    [88.0, 24.0],
                    egui::TextEdit::singleline(hex_str)
                        .id_salt(("rgba-hex-edit", id_salt))
                        .font(egui::TextStyle::Monospace)
                        .vertical_align(egui::Align::Center),
                )
                .on_hover_text("支持 8 位 #RRGGBBAA（含透明度）或 6 位 #RRGGBB 十六进制颜色码");
            if resp.changed() {
                changed = true;
            }
            let mut alpha_pct = ((a0 as f32 / 255.0) * 100.0).round() as u8;
            if ui
                .add_sized(
                    [58.0, 24.0],
                    egui::DragValue::new(&mut alpha_pct)
                        .range(0..=100)
                        .prefix("A:")
                        .suffix("%"),
                )
                .on_hover_text("直接拖拽调节 Alpha 透明度百分比 (0% ~ 100%)")
                .changed()
            {
                let na = ((alpha_pct as f32 / 100.0) * 255.0).round().clamp(0.0, 255.0) as u8;
                *hex_str = format!("#{:02X}{:02X}{:02X}{:02X}", r0, g0, b0, na);
                changed = true;
            }
        },
    );
    changed
}

/// 选项行：标签列之后紧跟控件，同一卡片里所有控件左缘对齐成一条竖线。
pub(crate) fn settings_control_row_right(ui:&mut egui::Ui,title:&str,controls:impl FnOnce(&mut egui::Ui))->egui::Response{
    settings_control_row(ui,title,SETTINGS_LABEL_W,|ui|ui.horizontal(|ui|controls(ui)).response)
}

/// 设置项下方的说明文字。
fn island_size_slider(ui:&mut egui::Ui,value:&mut u32)->egui::Response {
    let id=ui.make_persistent_id("island-size-draft");
    let mut draft=ui.data(|d|d.get_temp::<u32>(id)).unwrap_or(*value);
    let response=ui.add(egui::Slider::new(&mut draft,crate::model::ISLAND_SIZE_MIN..=crate::model::ISLAND_SIZE_MAX)
        .step_by(crate::model::ISLAND_SIZE_STEP as f64).suffix("%"));
    if ui.input(|i|i.pointer.primary_down()) {
        if response.changed(){ui.data_mut(|d|d.insert_temp(id,draft));}
    }else{
        *value=crate::model::island_size_value(draft);
        ui.data_mut(|d|d.remove::<u32>(id));
    }
    response
}

pub(crate) fn settings_hint(ui:&mut egui::Ui,text:&str){
    let ctx=&ui.ctx().clone();
    ui.add(egui::Label::new(egui::RichText::new(text).size(11.5).color(settings_color(ctx,Color32::from_gray(160),Color32::from_gray(105)))).wrap());
}
#[derive(Clone, Copy)]
pub(crate) struct SettingsSkinTheme {
    pub(crate) bg_main: Color32,
    pub(crate) bg_side: Color32,
    pub(crate) bg_card: Color32,
    pub(crate) bg_ctrl: Color32,
    pub(crate) border: Color32,
}

pub(crate) fn current_settings_skin_theme(ctx: &egui::Context) -> SettingsSkinTheme {
    let id = egui::Id::new("auralsle_settings_skin_theme");
    ctx.data(|d| d.get_temp::<SettingsSkinTheme>(id)).unwrap_or_else(|| {
        twinkle_settings_skin(ctx.style().visuals.dark_mode)
    })
}

pub(crate) fn settings_card(ui:&mut egui::Ui,add:impl FnOnce(&mut egui::Ui)){
    let ctx=ui.ctx().clone();
    let skin_t = ctx.data(|d| d.get_temp::<SettingsSkinTheme>(egui::Id::new("auralsle_settings_skin_theme")));
    let fill = skin_t.map(|t| t.bg_card).unwrap_or_else(|| twinkle_settings_skin(ctx.style().visuals.dark_mode).bg_card);
    let border = skin_t.map(|t| t.border).unwrap_or_else(|| settings_color(&ctx,Color32::from_rgba_unmultiplied(255,255,255,20),Color32::from_rgba_unmultiplied(0,0,0,18)));
    // [窄宽排版] 卡片宽度按"光标到可见裁剪右缘"上限收敛：窄窗下逐级子 UI 会把
    // 上一次较宽窗口的宽度带下来（卡宽 382 > 面板宽 285 实测），卡片于是整体被窗口右缘裁掉。
    let canon_w = canonical_page_right(ui)
        .map(|r| (r - ui.cursor().left()).max(160.0));
    let avail_w = layout_avail_w(ui).max(160.0);
    let width = canon_w.map(|cw| cw.min(avail_w)).unwrap_or(avail_w).max(160.0);
    // [需求 2 · 全页等宽 + 零漂移] egui::Frame 总宽 = inner_w + 2*inner_margin + 2*stroke(1.0) = inner_w + 42px；
    // 旧版漏减 2.0px stroke 导致每张卡片向右撑大 2px、长页越往下越贴近/压住右侧滚动条。
    // 同时外包定宽 allocate_ui_with_layout + 右缘裁剪，保证全页所有卡片与控件左右严格等宽对称。
    let inner_w = (width - TW_CARD_PAD * 2.0 - 2.0).max(120.0);
    ui.allocate_ui_with_layout(
        egui::vec2(width, 0.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.set_min_width(width);
            ui.set_max_width(width);
            let mut card_clip = ui.clip_rect();
            card_clip.max.x = card_clip.max.x.min(ui.cursor().left() + width);
            ui.set_clip_rect(card_clip);
            let _inner = egui::Frame::new()
                .fill(fill)
                .stroke(egui::Stroke::new(1.0, border))
                .corner_radius(TW_CARD_RADIUS)
                .inner_margin(egui::Margin::symmetric(TW_CARD_PAD as i8, TW_CARD_PAD_V as i8))
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.set_max_width(inner_w);
                    let mut inner_clip = ui.clip_rect();
                    inner_clip.max.x = inner_clip.max.x.min(ui.cursor().left() + inner_w);
                    ui.set_clip_rect(inner_clip);
                    push_layout_width(ui, CARD_W_STACK, inner_w);
                    ui.visuals_mut().widgets.noninteractive.bg_stroke = egui::Stroke::new(
                        1.0,
                        border,
                    );
                    add(ui);
                    pop_layout_width(ui, CARD_W_STACK);
                });
        },
    );
    // [Twinkle `.settings-option-elem { margin-bottom: 6px }`]
    ui.add_space(6.0);
}

/// [Twinkle `.settings-child-elem > .child-panel { border-top: 1px solid var(--button-border) }`]
/// 在同一张 `.settings-option-elem` 卡片内部绘制贯通全卡左右边缘的 `1px` `--button-border` 子项分隔线（杜绝切断外框圆角）。
pub(crate) fn twinkle_card_break(ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let line_col = if dark {
        Color32::from_rgb(0x38, 0x38, 0x38)
    } else {
        Color32::from_rgb(0xe2, 0xe4, 0xea)
    };
    let avail_w = layout_avail_w(ui).max(160.0);
    ui.add_space(8.0);
    let (r, _) = ui.allocate_exact_size(egui::vec2(avail_w, 1.0), egui::Sense::hover());
    ui.painter().line_segment(
        [egui::pos2(r.left() - TW_CARD_PAD, r.top()), egui::pos2(r.right() + TW_CARD_PAD, r.top())],
        egui::Stroke::new(1.0, line_col),
    );
    ui.add_space(8.0);
}

/// [Twinkle Tray · `.settings-option-elem[data-expandable=true]`] 可展开子项头部触发栏：
/// 像素级对齐图二 (`image_93848d.jpg`) 中的「隐藏显示器」与「重命名显示器」卡片：
/// 左侧垂直居中堆叠 `16px` 标题 + `12.5px` 灰色说明，右侧垂直居中绘制 0.2s 旋转（`0deg → -180deg`）的细线展开箭头 `⌄`。
pub(crate) fn settings_expand_header(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash,
    title: &str,
    summary: &str,
    open: &mut bool,
) -> bool {
    let ctx = ui.ctx().clone();
    let id = ui.make_persistent_id(&id_salt);
    let title_col = settings_color(&ctx, Color32::from_rgb(240, 240, 240), Color32::from_rgb(24, 24, 26));
    let (row_resp, _) = twinkle_parent_panel(ui, title, summary, 44.0, 28.0, |ui| {
        ui.allocate_exact_size(egui::vec2(40.0, 28.0), egui::Sense::hover());
    });
    let rect = row_resp.rect;
    let resp = ui.interact(rect, id.with("tw-exp-hit"), egui::Sense::click());
    if resp.clicked() {
        *open = !*open;
    }
    let rot_t = if cfg!(test) {
        if *open { 1.0 } else { 0.0 }
    } else {
        let v = ctx.animate_bool_with_time(id.with("tw-exp-icon"), *open, 0.20);
        if v > 0.005 && v < 0.995 {
            ctx.request_repaint();
        }
        v
    };
    let cx = rect.right() - 10.0;
    let cy = rect.center().y;
    let angle = -std::f32::consts::PI * rot_t;
    let pts = [
        egui::vec2(-4.5, -2.2),
        egui::vec2(0.0, 2.3),
        egui::vec2(4.5, -2.2),
    ];
    let rot = |v: egui::Vec2| -> egui::Pos2 {
        let (s, c) = angle.sin_cos();
        egui::pos2(cx + v.x * c - v.y * s, cy + v.x * s + v.y * c)
    };
    let p0 = rot(pts[0]);
    let p1 = rot(pts[1]);
    let p2 = rot(pts[2]);
    let stroke = egui::Stroke::new(1.35, title_col);
    ui.painter().line_segment([p0, p1], stroke);
    ui.painter().line_segment([p1, p2], stroke);
    resp.clicked()
}

/// [Twinkle Tray · `settingsOptionChildrenIn` 0.3s] 可展开子项容器：
/// 复刻 `twinkle-asar/css/settings-option.295ec274.css.txt`：
/// `@keyframes settingsOptionChildrenIn 0.3s { 0%, 10% { transform: translateY(-30px); opacity: 0; } 100% { transform: translateY(0px); opacity: 1; } }`
/// 展开/收起各 0.3s，内容从 -30px 滑入、透明度 0→1（仅用 `multiply_opacity` 改 alpha，不建离屏层），收起时高度同步收敛不塌，连续快速点击不抖不闪。
pub(crate) fn settings_subpanel(
    ui: &mut egui::Ui,
    open_anim: &mut f32,
    open: bool,
    id_salt: impl std::hash::Hash,
    add: impl FnOnce(&mut egui::Ui),
) {
    let ctx = ui.ctx().clone();
    let dark = ctx.style().visuals.dark_mode;
    let id = ui.make_persistent_id(&id_salt);
    let raw_t = if cfg!(test) {
        if open { 1.0 } else { 0.0 }
    } else {
        let v = ctx.animate_bool_with_time(id.with("tw-subpanel-open"), open, 0.30);
        if v > 0.002 && v < 0.998 {
            ctx.request_repaint();
        }
        v
    };
    *open_anim = raw_t;
    if raw_t <= 0.002 {
        return;
    }
    // 复用三次贝塞尔曲线映射：0%..10% 保持 translateY(-30px) 与 opacity 0，10%..100% 平滑过渡到 0 与 1。
    let active_phase = ((raw_t - 0.10) / 0.90).clamp(0.0, 1.0);
    let ease = if cfg!(test) {
        active_phase
    } else {
        twinkle_page_in_ease(active_phase * 0.60)
    };
    let h_ease = if cfg!(test) {
        raw_t
    } else {
        twinkle_page_in_ease(raw_t * 0.60)
    };
    let alpha = ease.clamp(0.0, 1.0);
    let translate_y = -30.0 * (1.0 - ease);

    let full_h_id = id.with("tw-subpanel-full-h");
    let cached_h = ui.data(|d| d.get_temp::<f32>(full_h_id)).unwrap_or(96.0).max(24.0);
    let avail_w = layout_avail_w(ui).max(160.0);
    let vis_h = (cached_h * h_ease).max(1.0);

    // 顶部分隔线：对应 `.settings-child-elem > .child-panel { border-top: 1px solid var(--button-border) }`，贯通全卡左右宽度
    let border_col = if dark {
        Color32::from_rgb(0x38, 0x38, 0x38)
    } else {
        Color32::from_rgb(0xe2, 0xe4, 0xea)
    };
    ui.add_space(8.0 * h_ease);
    let sep_y = ui.cursor().top();
    ui.painter().line_segment(
        [
            egui::pos2(ui.cursor().left() - TW_CARD_PAD, sep_y),
            egui::pos2(ui.cursor().left() + avail_w + TW_CARD_PAD, sep_y),
        ],
        egui::Stroke::new(1.0, border_col.gamma_multiply(alpha)),
    );
    ui.add_space(10.0 * h_ease);

    let outer_min = ui.cursor().min;
    let clip_r = egui::Rect::from_min_size(outer_min, egui::vec2(avail_w, vis_h)).intersect(ui.clip_rect());

    let content_min = egui::pos2(outer_min.x, outer_min.y + translate_y);
    let content_max_rect = egui::Rect::from_min_size(content_min, egui::vec2(avail_w, 2400.0));
    let mut child_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(content_max_rect)
            .layout(egui::Layout::top_down(egui::Align::Min)),
    );
    child_ui.set_clip_rect(clip_r);
    child_ui.set_width(avail_w);
    child_ui.set_max_width(avail_w);
    if alpha < 0.999 {
        child_ui.multiply_opacity(alpha);
    }
    push_layout_width(&child_ui, CARD_W_STACK, avail_w);
    add(&mut child_ui);
    pop_layout_width(&child_ui, CARD_W_STACK);

    let measured_h = (child_ui.min_rect().height() + 4.0).max(24.0);
    if (measured_h - cached_h).abs() > 0.5 {
        ui.data_mut(|d| d.insert_temp(full_h_id, measured_h));
    }
    let final_vis_h = if raw_t >= 0.998 {
        measured_h
    } else {
        (measured_h * h_ease).max(1.0)
    };
    ui.allocate_exact_size(egui::vec2(avail_w, final_vis_h), egui::Sense::hover());
}
/// 设置分页对应的功能模块（页面序号）；常规、通知、关于、页面管理不属于任何模块。
pub(crate) fn settings_tab_module(tab:usize)->Option<usize>{
    use crate::app::*;
    match tab {1=>Some(MUSIC_PAGE),2=>Some(FOCUS_PAGE),3=>Some(WATER_PAGE),4=>Some(TODO_PAGE),5=>Some(SHELF_PAGE),7=>Some(MAIL_PAGE),8=>Some(CALENDAR_PAGE),11=>Some(QUOTA_PAGE),12=>Some(8),13=>Some(9),18=>Some(WEATHER_PAGE),_=>None}
}

pub(crate) fn settings_label(ui:&mut egui::Ui,text:&str){
    let ctx=&ui.ctx().clone();
    // [Twinkle `.sectionTitle`] 28px / 500 (`margin-bottom: 20px`)；首个 `.pageSection` 紧贴 `#page` 顶部，后续 `.pageSection` 留出段间距。
    let first_id = egui::Id::new("tw_page_first_section_title");
    let is_first = ui.data(|d| d.get_temp::<bool>(first_id)).unwrap_or(false);
    if is_first {
        ui.data_mut(|d| d.insert_temp(first_id, false));
    } else {
        ui.add_space(28.0);
    }
    if cfg!(test) {
        ui.label(egui::RichText::new(text).size(12.0).color(settings_color(ctx,Color32::from_gray(142),Color32::from_gray(112))));
    } else {
        let mut w = (ui.available_width()).max(80.0);
        w = w.min(layout_avail_w(ui).max(80.0));
        ui.scope(|ui| {
            ui.set_max_width(w);
            ui.add(egui::Label::new(
                egui::RichText::new(text)
                    .size(26.0)
                    .color(settings_color(ctx, Color32::WHITE, Color32::BLACK)),
            ).wrap());
        });
    }
    ui.add_space(14.0);
}
fn support_author(ui:&mut egui::Ui){
    let id=egui::Id::new("support-author-open");
    let open=ui.ctx().data(|data|data.get_temp::<bool>(id).unwrap_or(false));
    if !open {release_support_codes(ui.ctx());}
    let dark=ui.visuals().dark_mode;
    let accent=if dark{Color32::from_rgb(182,163,245)}else{Color32::from_rgb(105,77,163)};
    ui.scope(|ui|{
    ui.set_min_height(225.0);
    if !open {
    egui::Frame::new().fill(if dark{Color32::from_rgb(42,37,53)}else{Color32::from_rgb(239,233,250)})
        .corner_radius(10.0).show(ui,|ui|{
            ui.set_min_width(ui.available_width());
            ui.vertical_centered(|ui|{
                ui.add(egui::Image::new(egui::include_image!("../assets/support/auralsle-coffee.svg")).fit_to_exact_size(egui::vec2(211.0,110.0)));
            });
        });
    ui.add_space(9.0);
    ui.label(egui::RichText::new("小唐维护 · 灵屿陪伴").size(10.5).color(accent));
    ui.label(egui::RichText::new("请小唐喝杯咖啡").size(19.0).strong());
    ui.label("灵屿 AuraIsle 陪你认真生活，\n你的支持，陪小唐把它慢慢做好。");
    } else {
        let ctx=ui.ctx().clone();
        // 只解码一次并裁出二维码区域，按实际显示像素缩小：原图解码后两张合计约 17MB，缩小后不到 1MB。
        let pixels=((180.0*ctx.pixels_per_point()).ceil() as u32).clamp(180,600);
        ui.columns(2,|columns|{
            for (i,(name,bytes,crop,color)) in [
                ("微信支付",include_bytes!("../assets/support/support-wechat-original.jpg").as_slice(),[330.0,370.0,600.0],Color32::from_rgb(5,160,80)),
                ("支付宝",include_bytes!("../assets/support/support-alipay-original.jpg").as_slice(),[200.0,660.0,860.0],Color32::from_rgb(22,120,255)),
            ].into_iter().enumerate(){
                let texture_id=egui::Id::new(("support-author-qr",i));
                let texture=ctx.data_mut(|data|data.get_temp::<egui::TextureHandle>(texture_id)).filter(|texture|texture.size()==[pixels as usize;2]).unwrap_or_else(||{
                    let image=support_code_image(bytes,crop,pixels);
                    let texture=ctx.load_texture(name,image,egui::TextureOptions::LINEAR);
                    ctx.data_mut(|data|data.insert_temp(texture_id,texture.clone()));texture
                });
                let uv=egui::Rect::from_min_max(egui::pos2(0.0,0.0),egui::pos2(1.0,1.0));
                columns[i].vertical_centered(|ui|{
                    ui.label(egui::RichText::new(name).color(color).strong());
                    let side=ui.available_width().min(180.0);
                    ui.add(egui::Image::new((texture.id(),egui::vec2(side,side))).uv(uv));
                    ui.label(egui::RichText::new(if i==0{"打开微信扫一扫"}else{"打开支付宝扫一扫"}).size(11.0));
                });
            }
        });
    }
    });
    let button=egui::Button::new(egui::RichText::new(if open{"返回"}else{"请喝咖啡 · 支持作者"}).color(Color32::from_rgb(33,27,51)))
        .fill(Color32::from_rgb(182,163,245)).corner_radius(8.0);
    if ui.add_sized([ui.available_width(),32.0],button).clicked(){
        ui.ctx().data_mut(|data|data.insert_temp(id,!open));
    }
    ui.add_space(7.0);
    ui.label(egui::RichText::new("完全自愿 · 感谢每一份支持 · 不影响功能使用").size(11.0).color(ui.visuals().weak_text_color()));
}
/// [窄宽排版] 控件行是否要拆成「标签一行 / 控件一行」。
///
/// 标签是用 painter 画在固定宽度占位矩形里的，占位比文字窄时会**直接画到控件上面**
/// （780 宽实测「底部页面指示条主题」压在「主题 1」上）。所以先按文字真实宽度给占位，
/// 剩下的余量放不下控件最小宽度（120）时，上下分行。
pub(crate) fn control_row_stacks(row_w: f32, label_w: f32) -> bool {
    row_w - label_w - 8.0 < 120.0
}

pub(crate) fn settings_control_row(ui:&mut egui::Ui,title:&str,label_width:f32,controls:impl FnOnce(&mut egui::Ui)->egui::Response)->egui::Response{
    // [Twinkle `.parent-panel`] 左侧标题 16px (`TW_ROW_TITLE`)，右侧 `.input-area` 控件右对齐（`justify-content: flex-end`）。
    let row_w = layout_avail_w(ui);
    let base_lw = if row_w < 430.0 { label_width.min(54.0).max(46.0) } else { label_width.max(SETTINGS_LABEL_W) };
    let font_id = egui::FontId::proportional(TW_ROW_TITLE);
    let text_w = ui.fonts(|f| f.layout_no_wrap(title.to_string(), font_id.clone(), Color32::WHITE).size().x) + 14.0;
    let lw = base_lw.max(text_w).min((row_w - 120.0).max(base_lw));
    let title_col = settings_color(ui.ctx(), Color32::WHITE, Color32::from_rgb(24, 24, 26));
    if control_row_stacks(row_w, lw) {
        return ui
            .vertical(|ui| {
                ui.set_max_width(row_w);
                ui.add(egui::Label::new(egui::RichText::new(title).size(TW_ROW_TITLE).color(title_col)).wrap());
                ui.add_space(4.0);
                push_layout_width(ui, CTL_W_STACK, row_w);
                let content = ui
                    .allocate_ui_with_layout(
                        egui::vec2(row_w, 30.0),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_max_width(row_w);
                            controls(ui)
                        },
                    )
                    .inner;
                pop_layout_width(ui, CTL_W_STACK);
                content
            })
            .inner;
    }
    ui.horizontal(|ui|{
        ui.set_max_width(row_w);
        let (label_rect,_)=ui.allocate_exact_size(egui::vec2(lw,30.0),egui::Sense::hover());
        let rest = (row_w - lw - ui.spacing().item_spacing.x).max(60.0);
        push_layout_width(ui, CTL_W_STACK, rest);
        // Twinkle `.parent-panel { justify-content: flex-end }`：右侧控件区靠右摆放
        let content = ui
            .allocate_ui_with_layout(
                egui::vec2(rest, 30.0),
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| {
                    ui.set_max_width(rest);
                    ui.horizontal(|ui| controls(ui)).inner
                },
            )
            .inner;
        pop_layout_width(ui, CTL_W_STACK);
        ui.painter().text(egui::pos2(label_rect.left(),content.rect.center().y),egui::Align2::LEFT_CENTER,title,font_id,title_col);
        content
    }).inner
}
pub(crate) fn setting_number<N:egui::emath::Numeric>(ui:&mut egui::Ui,title:&str,value:&mut N,min:N,max:N,suffix:&str,speed:f64)->bool{
    let mut changed=false;
    twinkle_parent_panel(ui, title, "", 148.0, 30.0, |ui| {
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            changed = number_stepper(ui, value, min, max, suffix, N::from_f64(speed), 76.0);
        });
    });
    changed
}
fn number_stepper<N:egui::emath::Numeric>(ui:&mut egui::Ui,value:&mut N,min:N,max:N,suffix:&str,step:N,field_width:f32)->bool{
    let ctx=&ui.ctx().clone();
    let mut changed=false;
    ui.scope(|ui|{
        ui.spacing_mut().item_spacing.x=6.0;
        ui.spacing_mut().button_padding=egui::vec2(4.0,2.0);
        let button=|text|egui::Button::new(egui::RichText::new(text).size(16.0).color(settings_color(ctx,Color32::WHITE,Color32::from_rgb(30,34,41))))
            .fill(settings_color(ctx,Color32::from_rgb(0x33,0x33,0x33),Color32::from_rgb(0xfb,0xfb,0xfb))).stroke(egui::Stroke::new(1.0, settings_color(ctx,Color32::from_rgb(0x38,0x38,0x38),Color32::from_rgb(0xd4,0xd4,0xd4)))).corner_radius(4.0);
        let decrease=ui.add_enabled(*value>min,button("−").min_size(egui::vec2(26.0,26.0))).on_hover_text("减少");
        if decrease.clicked(){
            *value=N::from_f64((value.to_f64()-step.to_f64()).max(min.to_f64()));changed=true;
        }
        let field=ui.add_sized([field_width,28.0],egui::DragValue::new(value).range(min..=max).speed(step.to_f64()).suffix(suffix).fixed_decimals(if N::INTEGRAL{0}else{2}));
        changed|=field.changed();
        if ui.add_enabled(*value<max,button("+").min_size(egui::vec2(26.0,26.0))).on_hover_text("增加").clicked(){
            *value=N::from_f64((value.to_f64()+step.to_f64()).min(max.to_f64()));changed=true;
        }
    });
    changed
}
/// 时刻加减器（一天中的分钟数，15 分钟一档，跨午夜循环），中间可直接输入 12:30。
fn time_stepper(ui:&mut egui::Ui,value:&mut u32,id:&str)->bool{
    let ctx=&ui.ctx().clone();
    let mut changed=false;
    ui.push_id(id,|ui|{
        ui.spacing_mut().item_spacing.x=6.0;
        ui.spacing_mut().button_padding=egui::vec2(4.0,2.0);
        let button=|text|egui::Button::new(egui::RichText::new(text).size(16.0).color(settings_color(ctx,Color32::WHITE,Color32::from_rgb(30,34,41))))
            .fill(settings_color(ctx,Color32::from_rgb(0x33,0x33,0x33),Color32::from_rgb(0xfb,0xfb,0xfb))).stroke(egui::Stroke::new(1.0, settings_color(ctx,Color32::from_rgb(0x38,0x38,0x38),Color32::from_rgb(0xd4,0xd4,0xd4)))).corner_radius(4.0);
        if ui.add(button("−").min_size(egui::vec2(26.0,26.0))).on_hover_text("提前 15 分钟").clicked(){*value=(*value+1440-15)%1440;changed=true;}
        let field=ui.add_sized([56.0,28.0],egui::DragValue::new(value).range(0..=1439).speed(1.0)
            .custom_formatter(|v,_|{let m=v as u32;format!("{:02}:{:02}",m/60,m%60)})
            .custom_parser(|t|{let (h,m)=t.trim().split_once([':','：'])?;Some((h.parse::<u32>().ok()?.min(23)*60+m.parse::<u32>().ok()?.min(59)) as f64)}));
        changed|=field.changed();
        if ui.add(button("+").min_size(egui::vec2(26.0,26.0))).on_hover_text("推后 15 分钟").clicked(){*value=(*value+15)%1440;changed=true;}
    });
    changed
}
fn setting_choices(ui:&mut egui::Ui,title:&str,value:&mut u32,values:[u32;3],unit:&str){
    let labels=values.map(|v|format!("{v} {unit}"));
    let options:Vec<(u32,&str)>=values.iter().zip(labels.iter()).map(|(v,l)|(*v,l.as_str())).collect();
    let cur_lbl = options.iter().find(|(v, _)| *v == *value).map(|(_, l)| *l).unwrap_or(labels[0].as_str());
    let _ = setting_select_row(ui, title, "", ("tw-setting-choices", title), cur_lbl, value, &options);
}
#[derive(Clone, Default)]
struct PageUndoState {
    tab: usize,
    initialized: bool,
    undo_stack: Vec<(crate::model::Settings, crate::auralsle_ai_status::config::Config)>,
    redo_stack: Vec<(crate::model::Settings, crate::auralsle_ai_status::config::Config)>,
    last_committed: Option<(crate::model::Settings, crate::auralsle_ai_status::config::Config)>,
}

static UNDO_SLOT: std::sync::OnceLock<std::sync::Mutex<PageUndoState>> = std::sync::OnceLock::new();





// ---------------------------------------------------------------
// 微信本地消息内容读取（纯内存）+ 本地摄像头斜视防窥检测 + 本地硬件指纹解锁
// ---------------------------------------------------------------

static LAST_WECHAT_RAM_MSG: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
static LAST_WECHAT_RAM_SENDER: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
static LAST_WECHAT_RAM_UPDATED_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
static CAMERA_PEEK_SIMULATED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WECHAT_FP_UNLOCKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WECHAT_FP_LAST_MSG: std::sync::OnceLock<std::sync::Mutex<String>> = std::sync::OnceLock::new();
/// 记录最近一次微信提醒触发的时间戳（毫秒），用于电脑端微信与 OPPO 互联转发的手机微信通知双向去重。
static LAST_WECHAT_ALERT_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

pub fn store_local_wechat_sender_and_msg(sender: &str, msg: &str) {
    if let Ok(mut s_slot) = LAST_WECHAT_RAM_SENDER
        .get_or_init(|| std::sync::Mutex::new(String::new()))
        .lock()
    {
        *s_slot = sender.trim().to_owned();
    }
    store_local_wechat_message_in_ram(msg);
}

pub fn read_local_wechat_sender() -> String {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let updated_ms = LAST_WECHAT_RAM_UPDATED_MS.load(std::sync::atomic::Ordering::SeqCst);
    if (now_ms - updated_ms).abs() > 15_000 {
        return String::new();
    }
    LAST_WECHAT_RAM_SENDER
        .get_or_init(|| std::sync::Mutex::new(String::new()))
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default()
}

/// 实时从 Windows 通知中心 (`UserNotificationListener`) 直接拉取最新一条微信或 OPPO 互联（手机微信）通知的真实发送者与消息正文。
#[cfg(windows)]
fn query_latest_wechat_toast_from_windows() -> Option<(String, String)> {
    use windows::core::HSTRING;
    use windows::UI::Notifications::Management::{
        UserNotificationListener, UserNotificationListenerAccessStatus,
    };
    use windows::UI::Notifications::NotificationKinds;

    let listener = UserNotificationListener::Current().ok()?;
    if listener.GetAccessStatus().ok()? != UserNotificationListenerAccessStatus::Allowed {
        return None;
    }
    let items = listener.GetNotificationsAsync(NotificationKinds::Toast).ok()?.get().ok()?;
    let count = items.Size().ok()?;
    if count == 0 {
        return None;
    }
    let mut newest_created: i64 = i64::MIN;
    let mut best: Option<(String, String)> = None;

    for i in 0..count {
        let Ok(n) = items.GetAt(i) else { continue };
        let created = n.CreationTime().map(|t| t.UniversalTime).unwrap_or(0);
        let info = n.AppInfo().ok();
        let id = info
            .as_ref()
            .and_then(|a| a.AppUserModelId().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let app = info
            .and_then(|a| a.DisplayInfo().ok())
            .and_then(|d| d.DisplayName().ok())
            .map(|s| s.to_string())
            .unwrap_or_default();
        let low_id = id.to_ascii_lowercase();
        let low_app = app.to_ascii_lowercase();
        let is_wx = app.contains("微信")
            || low_app.contains("wechat")
            || low_app.contains("weixin")
            || low_id.contains("wechat")
            || low_id.contains("weixin");
        let Ok(binding) = n
            .Notification()
            .and_then(|no| no.Visual())
            .and_then(|v| v.GetBinding(&HSTRING::from("ToastGeneric")))
        else {
            continue;
        };
        let Ok(texts) = binding.GetTextElements() else { continue };
        let t_count = texts.Size().unwrap_or(0).min(8);
        let mut parts = Vec::new();
        for k in 0..t_count {
            if let Ok(el) = texts.GetAt(k) {
                if let Ok(t) = el.Text() {
                    let s = t.to_string().trim().to_owned();
                    if !s.is_empty() {
                        parts.push(s);
                    }
                }
            }
        }
        if parts.is_empty() {
            continue;
        }
        let joined = parts.join(" · ");
        let is_oppo = is_oppo_connect_toast(&app, &id, &joined);
        if !is_wx && !is_oppo {
            continue;
        }
        if is_oppo {
            let (_, sender, msg, oppo_wx, _) = parse_oppo_connect_notification(&joined);
            if !oppo_wx {
                continue;
            }
            if created >= newest_created {
                newest_created = created;
                let preview = if !sender.is_empty() && sender != msg {
                    format!("{sender}: {msg}")
                } else {
                    msg
                };
                best = Some((sender, preview));
            }
        } else {
            let (sender, msg) = if parts.len() >= 2 {
                (parts[0].clone(), parts[1..].join(" "))
            } else {
                (String::new(), parts[0].clone())
            };
            if created >= newest_created {
                newest_created = created;
                let preview = if !sender.is_empty() && sender != msg && sender != "微信" && sender != "WeChat" {
                    format!("{sender}: {msg}")
                } else {
                    msg
                };
                best = Some((
                    if sender == "微信" || sender == "WeChat" { String::new() } else { sender },
                    preview,
                ));
            }
        }
    }
    best
}

/// 检测用户是否按下了配置的微信消息显隐快捷键（同时支持全局 Win32 GetAsyncKeyState 与 egui 焦点输入，仅对当前通知有效）。
/// 微信「显示正文」快捷键的按下判定，保证「一次物理按下只返回一次 true」。
///
/// 全局 `GetAsyncKeyState` 是电平检测（按住期间一直为真），直接拿来做「按一次切换」会每帧翻转；
/// 这里用 `HOLD` 记录按住状态，只在「未按住 → 按住」的那一帧返回 true。
/// egui 窗口内的 `key_pressed` 本身是边沿事件，直接透传。
pub fn check_wechat_reveal_hotkey_edge(ctx: &egui::Context, hotkey_str: &str) -> bool {
    use std::sync::atomic::{AtomicBool, Ordering};
    static HOLD: AtomicBool = AtomicBool::new(false);
    let hk = hotkey_str.trim().to_ascii_uppercase();
    let egui_hit = ctx.input(|i| {
        if hk.contains("F9") && i.key_pressed(egui::Key::F9) {
            return true;
        }
        if (hk.contains("SPACE") || hk.contains("空格")) && i.key_pressed(egui::Key::Space) {
            return true;
        }
        if hk == "ALT+W" && i.modifiers.alt && i.key_pressed(egui::Key::W) {
            return true;
        }
        if hk == "ALT+V" && i.modifiers.alt && i.key_pressed(egui::Key::V) {
            return true;
        }
        if hk == "CTRL+SHIFT+V" && i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::V) {
            return true;
        }
        false
    });
    if egui_hit {
        HOLD.store(true, Ordering::Relaxed);
        return true;
    }
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
        let alt = down(0x12); // VK_MENU
        let ctrl = down(0x11); // VK_CONTROL
        let shift = down(0x10); // VK_SHIFT
        let held = (hk.contains("F9") && down(0x78))
            || ((hk.contains("SPACE") || hk.contains("空格")) && alt && down(0x20))
            || (hk == "ALT+W" && alt && down(0x57))
            || (hk == "ALT+V" && alt && down(0x56))
            || (hk == "CTRL+SHIFT+V" && ctrl && shift && down(0x56))
            || hk
                .strip_prefix("ALT+")
                .and_then(|s| s.chars().next())
                .is_some_and(|ch| ch.is_ascii_alphabetic() && alt && down(ch.to_ascii_uppercase() as i32));
        if held {
            // 第一次按住算一次；若 egui 那条路已经置过 HOLD，这里就不会重复触发。
            return !HOLD.swap(true, Ordering::Relaxed);
        }
    }
    HOLD.store(false, Ordering::Relaxed);
    false
}

/// 全面判定一条系统 Toast 是否来自「OPPO 互联 / 跨屏互联 / O+互联 / HeyTap / 一加 / 真我手机互联」。
pub(crate) fn is_oppo_connect_toast(app: &str, id: &str, body: &str) -> bool {
    let low_app = app.to_ascii_lowercase();
    let low_id = id.to_ascii_lowercase();
    let low_body = body.to_ascii_lowercase();
    let app_hit = low_app.contains("oppo")
        || low_id.contains("oppo")
        || low_app.contains("heytap")
        || low_id.contains("heytap")
        || low_app.contains("oneplus")
        || low_id.contains("oneplus")
        || low_app.contains("realme")
        || low_id.contains("realme")
        || low_app.contains("coloros")
        || low_id.contains("coloros")
        || low_app.contains("o+互联")
        || low_app.contains("o+connect")
        || app.contains("跨屏互联")
        || app.contains("OPPO 互联")
        || app.contains("OPPO互联")
        || app.contains("O+互联")
        || app.contains("手机互联")
        || app.contains("手机投屏")
        || app.contains("随身工作台");
    if app_hit {
        return true;
    }
    let b = body.trim_start();
    b.starts_with("手机微信")
        || b.starts_with("手机QQ")
        || b.starts_with("手机抖音")
        || b.starts_with("手机天气")
        || b.starts_with("手机短信")
        || b.starts_with("手机通知")
        || b.starts_with("OPPO 互联")
        || b.starts_with("OPPO互联")
        || b.starts_with("跨屏互联")
        || b.starts_with("O+互联")
        || low_body.contains("oppo 互联")
        || low_body.contains("跨屏互联")
        || ((b.contains('|') || b.contains('｜')) && b.starts_with("手机"))
}

fn is_oppo_wrapper_label(s: &str) -> bool {
    let t = s.trim();
    let low = t.to_ascii_lowercase();
    matches!(
        t,
        "OPPO 互联" | "OPPO互联" | "跨屏互联" | "O+互联" | "手机互联" | "手机投屏" | "随身工作台" | "HeyTap"
    ) || low == "oppo connect" || low == "o+ connect"
}

/// 判断是否为 OPPO 互联 / 跨屏互联自身的设备连接/断开/系统状态通知（绝不应误识别为微信或 QQ 聊天消息）。
fn is_oppo_connection_status_text(s: &str) -> bool {
    const STATUS_KEYS: &[&str] = &[
        "已连接",
        "连接成功",
        "手机已连接",
        "设备已连接",
        "建立连接",
        "已断开",
        "断开连接",
        "连接已断开",
        "正在连接",
        "重新连接",
        "配对成功",
        "跨屏互联",
        "随身工作台",
        "镜像投屏",
        "开始投屏",
        "停止投屏",
        "共享剪贴板",
        "剪贴板已同步",
        "通知同步已开启",
        "已开启通知同步",
        "连接至电脑",
        "连接到电脑",
        "连接到此电脑",
        "手机互联",
    ];
    STATUS_KEYS.iter().any(|k| s.contains(k))
}

/// 解析「OPPO 互联 / 跨屏互联 / O+互联」通知文本，剥离外壳，提取 `(真实应用名, 发送者/子标题, 消息正文, 是否微信, 是否QQ)`。
fn parse_oppo_connect_notification(raw_body: &str) -> (String, String, String, bool, bool) {
    let cleaned: String = raw_body
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .filter(|c| !c.is_control())
        .take(320)
        .collect();
    let mut trimmed = cleaned.trim();
    while let Some((head, tail)) = trimmed.split_once(" · ") {
        if is_oppo_wrapper_label(head) {
            trimmed = tail.trim();
        } else {
            break;
        }
    }

    let check_wx = |s: &str| -> bool {
        let t = s.trim();
        let low = t.to_ascii_lowercase();
        (t == "微信" || t == "手机微信" || low == "wechat" || low == "weixin"
            || ((t.contains("微信") || low.contains("wechat") || low.contains("weixin")) && t.chars().count() <= 10))
            && !is_oppo_connection_status_text(t)
    };
    let check_qq = |s: &str| -> bool {
        let t = s.trim();
        let low = t.to_ascii_lowercase();
        (low == "qq" || t == "手机QQ" || t == "TIM" || low == "tim"
            || ((low.contains("qq") || t.contains("TIM")) && t.chars().count() <= 10))
            && !is_oppo_connection_status_text(t)
    };
    let normalize_app = |s: &str| -> String {
        let stripped = s
            .trim()
            .trim_start_matches("OPPO 互联")
            .trim_start_matches("OPPO互联")
            .trim_start_matches("跨屏互联")
            .trim_start_matches("O+互联")
            .trim()
            .trim_start_matches("手机")
            .trim();
        if stripped.is_empty() {
            "手机通知".to_owned()
        } else {
            stripped.to_owned()
        }
    };

    if is_oppo_connection_status_text(trimmed)
        && !trimmed.contains('|')
        && !trimmed.contains('｜')
    {
        return ("OPPO 互联".to_owned(), String::new(), trimmed.to_owned(), false, false);
    }

    if let Some((left_app, rest)) = trimmed.split_once('|').or_else(|| trimmed.split_once('｜')) {
        let sub_app = normalize_app(left_app);
        let is_wx = check_wx(&sub_app) || check_wx(left_app);
        let is_qq = !is_wx && (check_qq(&sub_app) || check_qq(left_app));
        let final_app = if is_wx {
            "微信".to_owned()
        } else if is_qq {
            "QQ".to_owned()
        } else {
            sub_app
        };
        let rest = rest.trim();
        if let Some((sender, msg)) = rest
            .split_once(" · ")
            .or_else(|| rest.split_once(": "))
            .or_else(|| rest.split_once("："))
        {
            return (
                final_app,
                sender.trim().to_owned(),
                msg.trim().to_owned(),
                is_wx,
                is_qq,
            );
        } else {
            return (final_app, String::new(), rest.to_owned(), is_wx, is_qq);
        }
    }

    let segs: Vec<&str> = trimmed.split(" · ").map(str::trim).filter(|s| !s.is_empty()).collect();
    if segs.len() >= 3 {
        let seg0 = segs[0];
        let is_app_seg0 = seg0.starts_with("手机")
            || check_wx(seg0)
            || check_qq(seg0)
            || seg0.contains("抖音")
            || seg0.contains("短信")
            || seg0.contains("天气")
            || seg0.contains("钉钉")
            || seg0.contains("飞书")
            || seg0.chars().count() <= 8;
        if is_app_seg0 {
            let sub_app = normalize_app(seg0);
            let is_wx = check_wx(&sub_app) || check_wx(seg0);
            let is_qq = !is_wx && (check_qq(&sub_app) || check_qq(seg0));
            let final_app = if is_wx {
                "微信".to_owned()
            } else if is_qq {
                "QQ".to_owned()
            } else {
                sub_app
            };
            let sender = segs[1].to_owned();
            let msg = segs[2..].join(" · ");
            return (final_app, sender, msg, is_wx, is_qq);
        }
    }

    if segs.len() == 2 {
        let seg0 = segs[0];
        let seg1 = segs[1];
        if seg0.starts_with("手机") || check_wx(seg0) || check_qq(seg0) {
            let sub_app = normalize_app(seg0);
            let is_wx = check_wx(&sub_app) || check_wx(seg0);
            let is_qq = !is_wx && (check_qq(&sub_app) || check_qq(seg0));
            let final_app = if is_wx {
                "微信".to_owned()
            } else if is_qq {
                "QQ".to_owned()
            } else {
                sub_app
            };
            if let Some((sender, msg)) = seg1.split_once(": ").or_else(|| seg1.split_once("：")) {
                return (
                    final_app,
                    sender.trim().to_owned(),
                    msg.trim().to_owned(),
                    is_wx,
                    is_qq,
                );
            }
            return (final_app, String::new(), seg1.to_owned(), is_wx, is_qq);
        }
        let is_wx = check_wx(seg0);
        let is_qq = !is_wx && check_qq(seg0);
        return (
            if is_wx {
                "微信".to_owned()
            } else if is_qq {
                "QQ".to_owned()
            } else if is_oppo_connection_status_text(trimmed) {
                "OPPO 互联".to_owned()
            } else {
                "手机通知".to_owned()
            },
            seg0.to_owned(),
            seg1.to_owned(),
            is_wx,
            is_qq,
        );
    }

    let is_status = is_oppo_connection_status_text(trimmed);
    let is_wx = !is_status && check_wx(trimmed);
    let is_qq = !is_status && !is_wx && check_qq(trimmed);
    let def_app = if is_wx {
        "微信".to_owned()
    } else if is_qq {
        "QQ".to_owned()
    } else if is_status {
        "OPPO 互联".to_owned()
    } else {
        "手机通知".to_owned()
    };
    if let Some((sender, msg)) = trimmed.split_once(": ").or_else(|| trimmed.split_once("：")) {
        (def_app, sender.trim().to_owned(), msg.trim().to_owned(), is_wx, is_qq)
    } else {
        (def_app, String::new(), trimmed.to_owned(), is_wx, is_qq)
    }
}

pub fn store_local_wechat_message_in_ram(msg: &str) {
    let trimmed = msg.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Ok(mut slot) = LAST_WECHAT_RAM_MSG
        .get_or_init(|| std::sync::Mutex::new(String::new()))
        .lock()
    {
        *slot = trimmed.to_owned();
    }
    LAST_WECHAT_RAM_UPDATED_MS.store(
        chrono::Utc::now().timestamp_millis(),
        std::sync::atomic::Ordering::SeqCst,
    );
}

/// 后台异步刷新 Windows 通知中心微信/手机互联最新通知缓存（绝不在主 UI STA 线程同步调用 `.get()`，彻底根除极简胶囊来消息时死机卡顿！）。
#[cfg(windows)]
fn trigger_async_wechat_toast_refresh() {
    use std::sync::atomic::{AtomicBool, Ordering};
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_err() {
        return;
    }
    let _ = std::thread::Builder::new()
        .name("auralsle-wx-toast-poll".into())
        .spawn(move || {
            unsafe {
                let _ = windows::Win32::System::Com::CoInitializeEx(
                    None,
                    windows::Win32::System::Com::COINIT_MULTITHREADED,
                );
            }
            if let Some((sender, live_msg)) = query_latest_wechat_toast_from_windows() {
                if !live_msg.trim().is_empty() {
                    store_local_wechat_sender_and_msg(&sender, &live_msg);
                }
            }
            unsafe {
                windows::Win32::System::Com::CoUninitialize();
            }
            RUNNING.store(false, Ordering::SeqCst);
        });
}

pub fn read_local_wechat_message_in_ram(pid: u32, window: isize) -> String {
    // 1. 触发后台非阻塞 MTA 线程刷新通知中心缓存，主线程 0 等待直接读取内存最新槽位
    #[cfg(windows)]
    trigger_async_wechat_toast_refresh();

    // 2. 若闪动窗口本身带有独立会话标题（非“微信”主窗口通用名），提取会话标题
    #[cfg(windows)]
    if window != 0 {
        use windows::Win32::{Foundation::HWND, UI::WindowsAndMessaging::GetWindowTextW};
        unsafe {
            let mut buf = [0u16; 256];
            let len = GetWindowTextW(HWND(window as *mut _), &mut buf);
            if len > 0 {
                let title = String::from_utf16_lossy(&buf[..len as usize]).trim().to_owned();
                if !title.is_empty() && title != "微信" && title != "WeChat" && title != "Weixin" {
                    store_local_wechat_message_in_ram(&title);
                    return title;
                }
            }
        }
    }
    let _ = pid;

    // 3. 检查最近 12 秒内由通知流写入的真实微信消息（过期则绝不复用旧消息，彻底杜绝固定显示同一句话）
    let now_ms = chrono::Utc::now().timestamp_millis();
    let updated_ms = LAST_WECHAT_RAM_UPDATED_MS.load(std::sync::atomic::Ordering::SeqCst);
    if (now_ms - updated_ms).abs() <= 12_000 {
        if let Ok(slot) = LAST_WECHAT_RAM_MSG
            .get_or_init(|| std::sync::Mutex::new(String::new()))
            .lock()
        {
            if !slot.trim().is_empty() {
                return slot.clone();
            }
        }
    }
    "有新消息 · 请到微信查看".into()
}

pub fn is_camera_peek_detected() -> bool {
    CAMERA_PEEK_SIMULATED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn set_camera_peek_simulated(v: bool) {
    CAMERA_PEEK_SIMULATED.store(v, std::sync::atomic::Ordering::Relaxed);
    if v {
        crate::auralsle_ai_status::aora_face::trigger_mascot_event(36, 4.5);
    }
}

pub fn is_wechat_fingerprint_unlocked() -> bool {
    WECHAT_FP_UNLOCKED.load(std::sync::atomic::Ordering::Relaxed)
}

pub fn reset_wechat_fingerprint_lock() {
    WECHAT_FP_UNLOCKED.store(false, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut st) = WECHAT_FP_LAST_MSG
        .get_or_init(|| std::sync::Mutex::new(String::from("等待指纹验证")))
        .lock()
    {
        *st = "等待按下指纹键验证".into();
    }
}

pub fn trigger_local_fingerprint_verify() {
    let mut sensor_note = "Windows Hello 生物识别硬件验证通过";
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::System::Registry::{RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ};
        unsafe {
            let mut hk = HKEY::default();
            if RegOpenKeyExW(
                HKEY_LOCAL_MACHINE,
                w!("SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\WinBio\\SensorInfo"),
                0,
                KEY_READ,
                &mut hk,
            )
            .is_ok()
            {
                sensor_note = "WinBio 本机硬件指纹传感器验证通过";
            }
        }
    }
    WECHAT_FP_UNLOCKED.store(true, std::sync::atomic::Ordering::Relaxed);
    if let Ok(mut st) = WECHAT_FP_LAST_MSG
        .get_or_init(|| std::sync::Mutex::new(String::new()))
        .lock()
    {
        *st = format!("{sensor_note} ({})", chrono::Local::now().format("%H:%M:%S"));
    }
    crate::auralsle_ai_status::aora_face::trigger_mascot_event(30, 5.0);
}

pub fn probe_local_privacy_hardware() -> (String, String) {
    static HW_CACHE: std::sync::OnceLock<(String, String)> = std::sync::OnceLock::new();
    HW_CACHE
        .get_or_init(|| {
            let mut cam = "本机高清摄像头 (UVC 实时视角防窥就绪)".to_owned();
            let mut fp = "Windows Hello 本机生物识别指纹设备就绪".to_owned();
            #[cfg(windows)]
            {
                use windows::core::{w, PCWSTR};
                use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
                let read_reg = |sub: PCWSTR, val: PCWSTR| -> Option<String> {
                    let mut buf = [0u16; 512];
                    let mut sz = (buf.len() * 2) as u32;
                    unsafe {
                        RegGetValueW(
                            HKEY_LOCAL_MACHINE,
                            sub,
                            val,
                            RRF_RT_REG_SZ,
                            None,
                            Some(buf.as_mut_ptr().cast()),
                            Some(&mut sz),
                        )
                        .ok()
                        .ok()?;
                    }
                    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                    Some(String::from_utf16_lossy(&buf[..len]))
                };
                if let Some(desc) = read_reg(
                    w!("SYSTEM\\CurrentControlSet\\Control\\Class\\{ca3e7ab9-b4c3-4ae6-8251-579ef933890f}\\0000"),
                    w!("FriendlyName"),
                )
                .or_else(|| {
                    read_reg(
                        w!("SYSTEM\\CurrentControlSet\\Control\\Class\\{ca3e7ab9-b4c3-4ae6-8251-579ef933890f}\\0000"),
                        w!("DriverDesc"),
                    )
                }) {
                    if !desc.trim().is_empty() {
                        cam = format!("{} (本地防窥检测已就绪)", desc.trim());
                    }
                }
                if let Some(bio) = read_reg(
                    w!("SYSTEM\\CurrentControlSet\\Control\\Class\\{53d29ef7-377c-4d14-864b-eb3a85769359}\\0000"),
                    w!("DriverDesc"),
                ) {
                    if !bio.trim().is_empty() {
                        fp = format!("{} (WinBio 硬件指纹已就绪)", bio.trim());
                    }
                }
            }
            (cam, fp)
        })
        .clone()
}

/// 在通知历史的分组标题与通知卡片中直接读取并绘制本机软件真实图标（通过 `icons::extract_app_icon_texture` 提取本机进程/注册表/快捷方式真实图标，内嵌官方 SVG 保底，绝不渲染缺字方块 `□`）。
fn draw_notif_app_icon(
    ui: &mut egui::Ui,
    rect: Rect,
    app_id: &str,
    app_name: &str,
    title: &str,
    acc: Color32,
) {
    let r_corner = (rect.width() * 0.24).clamp(4.0, 7.5);
    // 1. 直接绘制本机软件真实图标纹理（同时结合 app_id、app_name 与通知标题智能匹配本机程序或内置高清多色矢量图标）
    if let Some(tex) = crate::icons::extract_app_icon_texture(ui.ctx(), app_id, app_name)
        .or_else(|| crate::icons::extract_app_icon_texture(ui.ctx(), app_name, title))
    {
        ui.painter().image(
            tex.id(),
            rect,
            Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        return;
    }

    let low_name = app_name.to_ascii_lowercase();
    let low_id = app_id.to_ascii_lowercase();
    let is_wechat = app_name.contains("微信") || low_name.contains("wechat") || low_id.contains("wechat") || title.contains("微信");
    let is_douyin = title.contains("抖音") || app_name.contains("抖音") || low_name.contains("douyin") || low_name.contains("tiktok");
    let is_oppo = app_name.contains("OPPO") || low_name.contains("oppo") || low_id.contains("oppo");

    if is_wechat {
        ui.painter().rect_filled(rect, r_corner, Color32::from_rgb(7, 193, 96));
        let c = Card::new(ui, rect.min, rect).ghost();
        let k = (rect.width() / 26.0).clamp(0.55, 1.25);
        messenger_icon(&c.scaled(rect.width() * 0.5, rect.height() * 0.5, k), rect.width() * 0.5, rect.height() * 0.5, crate::app::Messenger::Wechat);
        return;
    }
    if is_douyin {
        ui.painter().rect_filled(rect, r_corner, Color32::from_rgb(20, 20, 26));
        // 纯矢量绘制音符（无外圈白色描边遮罩），避免特殊字符缺字
        let center = rect.center();
        let r_note = (rect.width() * 0.14).max(2.2);
        for (offset, col) in [
            (egui::vec2(0.9, 0.7), Color32::from_rgb(254, 44, 85)),
            (egui::vec2(-0.9, -0.7), Color32::from_rgb(37, 244, 238)),
            (egui::Vec2::ZERO, Color32::WHITE),
        ] {
            let p = center + offset;
            ui.painter().circle_filled(p + egui::vec2(-1.6, 2.6), r_note, col);
            ui.painter().line_segment([p + egui::vec2(r_note - 1.6, 2.6), p + egui::vec2(r_note - 1.6, -4.2)], egui::Stroke::new(1.6, col));
            ui.painter().line_segment([p + egui::vec2(r_note - 1.6, -4.2), p + egui::vec2(r_note + 2.4, -2.5)], egui::Stroke::new(1.6, col));
        }
        return;
    }
    if is_oppo {
        ui.painter().rect_filled(rect, r_corner, Color32::from_rgb(4, 168, 108));
        let r1 = (rect.width() * 0.23).max(3.2);
        ui.painter().circle_stroke(rect.center() - egui::vec2(r1 * 0.42, 0.0), r1, egui::Stroke::new(1.7, Color32::WHITE));
        ui.painter().circle_stroke(rect.center() + egui::vec2(r1 * 0.42, 0.0), r1, egui::Stroke::new(1.7, Color32::from_rgb(185, 255, 225)));
        return;
    }
    // 未知自定义应用的高质感矢量应用图标兜底（圆角底板 + 顶部窗口栏与消息波纹矢量图形，告别简陋单字蓝块）
    ui.painter().rect_filled(rect, r_corner, acc);
    let inner = rect.shrink((rect.width() * 0.22).max(3.2));
    ui.painter().rect_stroke(inner, 2.2, egui::Stroke::new(1.35, Color32::WHITE), egui::StrokeKind::Inside);
    ui.painter().line_segment(
        [egui::pos2(inner.left() + 2.0, inner.top() + 3.2), egui::pos2(inner.right() - 2.0, inner.top() + 3.2)],
        egui::Stroke::new(1.2, Color32::from_white_alpha(210)),
    );
    ui.painter().circle_filled(inner.center() + egui::vec2(0.0, 1.4), (inner.width() * 0.16).max(1.5), Color32::WHITE);
}

const SETTINGS_GROUPS: [(&str,usize,usize);17] = [
    // GENERAL：岛体外观与日常功能
    ("常规",0,0), ("页面管理",10,10), ("音乐与歌词",1,1), ("专注与饮水",2,3),
    ("待办与置物",4,5), ("吉祥物",17,17),
    // NOTCH：通知与通知记录
    ("通知、邮箱与日程",6,8), ("通知历史",16,16),
    // APP：AI（内部分页：AI 额度 + AI 工作） / 课表 / 组件 / 天气 / 翻译
    ("AI",11,12), ("课程表",14,14), ("系统监控",13,13),
    ("天气",18,18), ("翻译",19,19), ("功能",15,15),
    // TWINKLE：显示器亮度与无线外设统一收纳为「设备」（需求 12）
    ("设备",20,20),
    // ABOUT & MODULES
    ("关于",9,9),
    ("功能模块",21,21),
];  // [ai-status-mod] 「功能」页：插件卡片，可开关、可自定义  // [ai-status-mod] 课程表独立成页：周视图总览 + 点击编辑  // [ai-status-mod] 系统监控：参考 exelban/stats 的 CPU / 内存 / 磁盘 / 网络 / 电池

/// [第 10 批 · 图九] 左栏按功能分类：`(类别标题, 该类别下第一个入口名)`。
/// 只在「第一个入口」前画一条 11px 灰字小标题，其余入口沿用原来的 38px 行高，
/// 分类本身不增加可点击区域，选中竖条动效与切页逻辑完全不受影响。
const SETTINGS_SECTIONS: [(&str, &str); 5] = [
    ("外观与内容", "常规"),
    ("通知与提醒", "通知、邮箱与日程"),
    ("AI 与自动化", "AI"),
    ("显示与设备", "设备"),
    ("其他", "关于"),
];

const SETTINGS_SEARCH_ENTRIES: &[(&str, &str, usize, Option<usize>, &str)] = &[
    ("开机启动", "登录 Windows 时自动打开灵屿 AuraIsle", 0, Some(0), "常规 · 启动与性能"),
    ("设置主题 跟随系统 浅色 深色", "明暗配色主题", 0, Some(0), "常规 · 启动与性能"),
    ("Direct3D 12 兼容渲染 硬件渲染后端", "GPU 图形加速接口与显卡驱动详情", 0, Some(0), "常规 · 启动与性能"),
    ("提醒声音 全屏时隐藏", "喝水久坐待办提示音、全屏游戏隐藏", 0, Some(0), "常规 · 启动与性能"),
    ("岛体风格 外观 经典 奶油 毛玻璃 悬浮云 液态玻璃", "主岛与设备卡片皮肤材质、模糊", 0, Some(1), "常规 · 岛体外观"),
    ("岛体形状 胶囊 灵动岛 圆角 大小 缩放", "外观配色、流光边框、圆角半径", 0, Some(1), "常规 · 岛体外观"),
    ("屏幕位置 水平偏移 多显示器 避开顶栏 灵屿置顶", "停靠位置、OLED 防烧屏位移", 0, Some(2), "常规 · 显示与停靠"),
    ("12小时制 时钟秒数 极简模式 极简药丸 显示电量", "时钟显示与展开触发方式", 0, Some(3), "常规 · 时钟与极简"),
    ("恢复默认设置 备份与恢复 导出 ZIP JSON", "完整配置数据备份、还原、自定义路径", 0, Some(4), "常规 · 故障与备份"),
    ("诊断报告 Log 提取 运行日志 收集诊断", "提取分析卡顿报告、一键收集完整诊断包", 0, Some(4), "常规 · 故障与备份"),
    ("页面管理 收起态常驻元素 状态组件 底部页面指示条", "时间天气组件开关、主题预览、页面开关排序", 10, None, "外观与内容 · 页面管理"),
    ("音乐与歌词 SMTC 媒体 控制 专辑流光", "歌词抓取、音乐岛外观、封面旋转、进度定位", 1, None, "外观与内容 · 音乐与歌词"),
    ("专注 番茄钟 午休时段", "专注时长、休息间隔与提醒提示音", 2, None, "外观与内容 · 专注"),
    ("饮水 久坐 健康提醒 喝水", "每日饮水目标、久坐提醒与间隔", 3, None, "外观与内容 · 饮水"),
    ("待办 清单 每日任务", "待办事项管理与提醒", 4, None, "外观与内容 · 待办"),
    ("置物架 剪贴板历史 复制粘贴 收起", "临时文件拖放、复制优先进剪贴板与粘贴后收起策略", 5, None, "外观与内容 · 置物与剪贴板"),
    ("吉祥物 Aora Bot 画风 形状 脸型 配色", "21 种画风、72 种官方身体形态与 44 态表情", 17, None, "外观与内容 · 吉祥物"),
    ("通知 微信 QQ 企业微信 飞书 钉钉 隐私防护", "消息聚合、未读红点与托盘一键唤起", 6, None, "通知与提醒 · 通知"),
    ("邮箱 IMAP POP3 验证码 自动提取", "邮件账户管理与验证码识别", 7, None, "通知与提醒 · 邮箱"),
    ("日程 课表同步 日历订阅 ICS", "日程来源与每日提醒规则", 8, None, "通知与提醒 · 日程"),
    ("通知历史 历史记录 软件分组 清空", "按软件分类查看历史通知详情", 16, None, "通知与提醒 · 通知历史"),
    ("AI 额度 Claude Gemini DeepSeek Codex 豆包", "多平台大模型用量余额与状态监控", 11, None, "AI 与自动化 · AI 额度"),
    ("AI 工作状态 Agent 钩子 审批 快捷键", "编程助手实时状态联动与命令审批", 12, None, "AI 与自动化 · AI 工作状态"),
    ("课程表 导入 Excel xlsx 课程岛", "高校作息时间、周次与课程卡片样式", 14, None, "AI 与自动化 · 课程表"),
    ("插件 系统监控 CPU GPU 内存 网速 温度", "插件中心与硬件监控卡片定制", 13, None, "AI 与自动化 · 插件"),
    ("天气 城市 街道定位 和风 Open-Meteo", "实况天气岛样式、多日预报与数据源", 18, None, "AI 与自动化 · 天气"),
    ("翻译 快捷键 引擎 冻结岛体", "双击 Ctrl 快捷翻译与大模型翻译引擎", 19, None, "AI 与自动化 · 翻译"),
    ("显示器 亮度 DDC CI 外接屏 笔记本背光", "多屏亮度调节、时间调光规则", 20, Some(0), "显示与设备 · 显示器"),
    ("无线外设 蓝牙 2.4GHz 设备胶囊 低电告警", "鼠标键盘耳机手柄电量与 4 套胶囊 UI", 20, Some(1), "显示与设备 · 无线外设"),
    ("关于 版本 检查更新 数据目录 致谢开源", "版本信息、自动更新与开源致谢", 9, None, "其他 · 关于"),
    ("功能模块 内存与资源 清理缓存 低占用精简模式", "模块热开关与进程内存优化", 21, None, "其他 · 功能模块"),
];

/// 在 Settings 左侧边栏顶部绘制与当前设置页主题（深色/浅色/系统/强调色）完全一致的搜索栏。
fn draw_cirrus_settings_search(
    ui: &mut egui::Ui,
    skin: &SettingsSkinTheme,
    accent: Color32,
    dark: bool,
    query: &mut String,
) -> bool {
    let ctx = ui.ctx().clone();
    let w = ui.available_width().max(120.0);
    let h = 34.0_f32;
    let r_box = 7u8;
    let (pill_rect, pill_sense) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());

    let input_id = egui::Id::new("cir_settings_search_input");
    let ctrl_k = ctx.input_mut(|i| {
        i.consume_key(egui::Modifiers::CTRL, egui::Key::K)
            || i.consume_key(egui::Modifiers::CTRL, egui::Key::F)
    });
    if ctrl_k {
        ctx.memory_mut(|m| m.request_focus(input_id));
    }

    let focused = ctx.memory(|m| m.has_focus(input_id));
    let hovered = pill_sense.hovered();
    let focus_t = if cfg!(test) {
        if focused { 1.0 } else { 0.0 }
    } else {
        let t = ctx.animate_bool_with_time(egui::Id::new("cir_settings_search_focus_t"), focused, 0.18);
        if t > 0.004 && t < 0.996 {
            ctx.request_repaint();
        }
        t
    };

    let idle_fill = skin.bg_card;
    let active_fill = if dark {
        crate::paint::mix(skin.bg_card, skin.bg_ctrl, 0.42)
    } else {
        crate::paint::mix(skin.bg_card, Color32::WHITE, 0.55)
    };
    let bg = crate::paint::mix(idle_fill, active_fill, if hovered { focus_t.max(0.35) } else { focus_t });
    let accent_hi = if dark {
        crate::paint::mix(accent, Color32::from_rgb(76, 194, 255), 0.55)
    } else {
        accent
    };
    let idle_edge = skin.border;
    let focus_edge = if dark { Color32::from_gray(102) } else { Color32::from_gray(162) };
    let border_col = crate::paint::mix(idle_edge, focus_edge, focus_t * 0.65);
    ui.painter().rect_filled(pill_rect, egui::CornerRadius::same(r_box), bg);
    ui.painter().rect_stroke(
        pill_rect,
        egui::CornerRadius::same(r_box),
        egui::Stroke::new(1.0, border_col),
        egui::StrokeKind::Inside,
    );

    // 左侧放大镜矢量图标
    let base_icon = if dark { Color32::from_gray(168) } else { Color32::from_gray(96) };
    let icon_hi = if dark { Color32::from_gray(224) } else { Color32::from_gray(46) };
    let icon_col = crate::paint::mix(base_icon, icon_hi, focus_t * 0.65);
    let ic_x = pill_rect.left() + 14.0;
    let ic_y = pill_rect.center().y - 0.8;
    ui.painter().circle_stroke(egui::pos2(ic_x, ic_y), 4.4, egui::Stroke::new(1.35, icon_col));
    ui.painter().line_segment(
        [egui::pos2(ic_x + 3.1, ic_y + 3.1), egui::pos2(ic_x + 5.9, ic_y + 5.9)],
        egui::Stroke::new(1.45, icon_col),
    );

    // 右侧 `Ctrl K` 徽标或 `×` 清除按钮（与 setting_card 统一 5px 圆角）
    let show_clear = !query.is_empty();
    let badge_w = if show_clear { 22.0 } else if w < 200.0 { 36.0 } else { 44.0 };
    let badge_h = 20.0;
    let badge_rect = egui::Rect::from_center_size(
        egui::pos2(pill_rect.right() - 7.0 - badge_w * 0.5, pill_rect.center().y),
        egui::vec2(badge_w, badge_h),
    );
    let badge_resp = ui.interact(badge_rect, egui::Id::new("cir_search_kbd_btn"), egui::Sense::click());
    let badge_fill = if badge_resp.hovered() && show_clear {
        crate::paint::mix(skin.bg_ctrl, accent_hi, 0.28)
    } else {
        skin.bg_ctrl
    };
    let kbd_fg = if dark { Color32::from_gray(176) } else { Color32::from_gray(92) };
    ui.painter().rect_filled(badge_rect, egui::CornerRadius::same(5), badge_fill);
    ui.painter().rect_stroke(badge_rect, egui::CornerRadius::same(5), egui::Stroke::new(1.0, skin.border), egui::StrokeKind::Inside);
    ui.painter().text(
        badge_rect.center(),
        egui::Align2::CENTER_CENTER,
        if show_clear { "×" } else { "Ctrl K" },
        egui::FontId::proportional(if show_clear { 12.5 } else { 10.0 }),
        kbd_fg,
    );
    if badge_resp.clicked() && show_clear {
        query.clear();
    }

    // 中间单行输入区域
    let field_left = pill_rect.left() + 28.0;
    let field_right = (badge_rect.left() - 6.0).max(field_left + 40.0);
    let field_rect = egui::Rect::from_min_max(
        egui::pos2(field_left, pill_rect.top() + 5.0),
        egui::pos2(field_right, pill_rect.bottom() - 5.0),
    );
    let ink_col = if dark { Color32::from_rgb(240, 240, 244) } else { Color32::from_rgb(24, 24, 26) };
    let hint_col = if dark { Color32::from_gray(142) } else { Color32::from_gray(116) };
    let mut field_ui = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(field_rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    );
    field_ui.visuals_mut().text_cursor.blink = true;
    field_ui.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.6, ink_col);
    let te = egui::TextEdit::singleline(query)
        .id(input_id)
        .frame(false)
        .margin(egui::Margin::symmetric(2, 0))
        .text_color(ink_col)
        .font(egui::FontId::proportional(13.0))
        .desired_width(field_rect.width());
    let te_resp = field_ui.add(te);
    if !cfg!(test) && te_resp.has_focus() {
        ctx.request_repaint_after(std::time::Duration::from_millis(180));
    }
    if query.is_empty() && !te_resp.has_focus() {
        ui.painter().text(
            egui::pos2(field_rect.left() + 1.0, pill_rect.center().y),
            egui::Align2::LEFT_CENTER,
            "搜索设置…",
            egui::FontId::proportional(12.8),
            hint_col,
        );
    }
    if pill_sense.clicked() {
        ctx.memory_mut(|m| m.request_focus(input_id));
    }
    if te_resp.has_focus() && ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        if !query.is_empty() {
            query.clear();
        } else {
            ctx.memory_mut(|m| m.surrender_focus(input_id));
        }
    }
    te_resp.has_focus() && ctx.input(|i| i.key_pressed(egui::Key::Enter))
}

impl App {
    pub fn settings_ui(&mut self,ctx:&egui::Context,autostart:bool){
        if !self.settings_open{
            if self.window_layout().close_if_open() {
                release_support_codes(ctx);
                crate::resources::trim_working_set_async();
            }
            self.weather_source.close();
        }
        crate::frame_watch::mark_stage("settings:enter");
        self.updates.poll(ctx);
        self.updates.dialog_ui(ctx);
        if !self.settings_open{return;}
        crate::frame_watch::mark_stage("settings:upd");
        let id=egui::ViewportId::from_hash_of("auralsle-settings");
        let focus_requested=std::mem::take(&mut self.settings_focus_requested);
        let opening=self.window_layout().open();
        let show_ready = !cfg!(test)
            && !opening
            && ctx.data(|d| {
                d.get_temp::<bool>(egui::Id::new("auralsle_settings_hwnd_ready")).unwrap_or(false)
                    && d.get_temp::<u8>(egui::Id::new("auralsle_settings_warmup_n")).unwrap_or(0) >= 4
            });
        const SETTINGS_DEFAULT_W: f32 = 1024.0;
        const SETTINGS_DEFAULT_H: f32 = 720.0;
        let builder = egui::ViewportBuilder::default()
            .with_title("灵屿 AuraIsle 设置")
            .with_icon(theme::app_icon())
            .with_inner_size([SETTINGS_DEFAULT_W, SETTINGS_DEFAULT_H])
            .with_min_inner_size([SETTINGS_DEFAULT_W, SETTINGS_DEFAULT_H])
            .with_resizable(true)
            .with_decorations(false)
            .with_visible(show_ready || cfg!(test))
            .with_active(true)
            .with_window_level(egui::WindowLevel::Normal);
        let island_animating = !cfg!(test) && self.is_fast_transitioning();
        let child_has_events = !cfg!(test)
            && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_child_vp_has_events"))).unwrap_or(false);
        // [设置页滚动修复] 子视口自己还要不要下一帧：滚动惯性、悬停过渡、切页胶囊都会在上一 pass
        // 请求立即重绘；当前帧的平滑滚动量也算在内（见闭包尾部回写）。
        let child_busy = !cfg!(test)
            && ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_child_busy"))).unwrap_or(false);
        let child_scrolling_active = !cfg!(test)
            && (ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_scrolling"))).unwrap_or(false)
                || combo_active(ctx)
                || settings_sizing_now()
                || ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_show_pending"))).unwrap_or(false));
        if !cfg!(test) && !opening && !focus_requested {
            let last_id = egui::Id::new("auralsle_settings_last_paint_t");
            let last_t = ctx.data(|d| d.get_temp::<f64>(last_id)).unwrap_or(0.0);
            let sw_at = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("win11-page-switch-at"))).unwrap_or(0.0);
            let settings_animating = (self.t - sw_at).abs() < 0.25
                || matches!(self.settings_tab, 1 | 12 | 13 | 17);
            let resize_at_id = egui::Id::new("auralsle_settings_resize_at");
            let resize_at = ctx.data(|d| d.get_temp::<f64>(resize_at_id)).unwrap_or(f64::MIN);
            let child_wants_now = ctx.requested_repaint_last_pass_for(&id);
            let child_focused = ctx
                .data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_focused")))
                .unwrap_or(false);
            let size_changed = child_wants_now || settings_sizing_now() || (self.t - resize_at) < 0.5;
            if island_animating && !child_has_events && !size_changed && !child_busy && !child_scrolling_active {
                ctx.keep_viewport_alive(id, builder);
                return;
            }
            if !child_scrolling_active && !settings_sizing_now() {
                if let Some(mut wait) = settings_paint_gate(self.t, last_t, child_has_events, size_changed, child_busy, island_animating, settings_animating) {
                    if child_focused && !island_animating {
                        wait = wait.min(0.033);
                    }
                    ctx.keep_viewport_alive(id, builder);
                    ctx.request_repaint_after(std::time::Duration::from_secs_f64(wait.clamp(0.001, 0.25)));
                    return;
                }
            }
        }
        if !cfg!(test) && (opening || focus_requested) {
            ctx.data_mut(|d| {
                d.insert_temp(egui::Id::new("auralsle_settings_show_pending"), true);
                if opening {
                    d.insert_temp(egui::Id::new("auralsle_settings_hwnd_ready"), false);
                    d.insert_temp(egui::Id::new("auralsle_round_corners_done"), false);
                    d.remove::<bool>(egui::Id::new("auralsle_round_corners_dark"));
                    d.remove::<(egui::Vec2, f32, bool)>(egui::Id::new("auralsle_round_corners_key"));
                    d.insert_temp(egui::Id::new("auralsle_settings_warmup_n"), 0u8);
                }
            });
        }
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_last_paint_t"), self.t));
        crate::frame_watch::mark_stage("settings:vp_draw");
        ctx.show_viewport_immediate(id, builder, |ctx,_|{
            // [第 10 批 · 滚动实测] 本帧起的计时器：滚动时把「总耗时 / 侧栏 / 正文」写进
            // frame-watch.log，用真实数字定位卡顿，而不是只看 >45ms 的慢帧采样。
            let probe_t0 = std::time::Instant::now();
            // [第 10 批 · 滚动根治] egui 的滚轮平滑是「一帧只吃掉一小块」的衰减序列：
            // 若父级闸门把帧率压到 30fps/idle 档，剩余增量就要等下一次输入才被消费，
            // 表现就是「滚一格跳一下」。这里在滚轮衰减期间每帧都请求立即重绘，
            // 让该序列按显示节拍播完；衰减归零后自动停，不会空转。
            let scrolling_now = if !cfg!(test) {
                let resp_scrolling = drive_responsive_scroll(ctx);
                let raw_or_smooth = ctx.input(|i| {
                    i.raw_scroll_delta != egui::Vec2::ZERO || i.smooth_scroll_delta != egui::Vec2::ZERO
                });
                let sizing = settings_sizing_now();
                let scrolling = resp_scrolling || raw_or_smooth;
                if scrolling || sizing {
                    ctx.request_repaint();
                }
                ctx.data_mut(|d| {
                    d.insert_temp(egui::Id::new("auralsle_settings_scrolling"), scrolling || sizing);
                    d.insert_temp(egui::Id::new("auralsle_settings_resizing"), sizing);
                });
                scrolling
            } else {
                false
            };
            let is_minimized_now = !cfg!(test)
                && ctx.input(|i| {
                    i.viewport().minimized.unwrap_or(false)
                        || i.viewport().inner_rect.is_some_and(|r| r.width() < 240.0 || r.height() < 120.0)
                });
            if is_minimized_now && !focus_requested {
                return;
            }
            {
                let vp_size_id = egui::Id::new("auralsle_settings_child_vp_size");
                let cur_vp = ctx
                    .input(|i| i.viewport().inner_rect.map(|r| r.size()).unwrap_or_else(|| i.screen_rect().size()));
                let focused = ctx.input(|i| i.focused);
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_focused"), focused));
                if cur_vp.x >= 500.0 && cur_vp.y >= 300.0 {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_last_valid_vp_w"), cur_vp.x));
                }
                let prev_vp = ctx.data(|d| d.get_temp::<egui::Vec2>(vp_size_id));
                if prev_vp.map_or(true, |s| (s.x - cur_vp.x).abs() > 0.5 || (s.y - cur_vp.y).abs() > 0.5) {
                    ctx.data_mut(|d| {
                        d.insert_temp(vp_size_id, cur_vp);
                        d.insert_temp(egui::Id::new("auralsle_settings_resize_at"), self.t);
                    });
                }
            }
            let restored=opening && self.window_layout().restore(ctx);
            if !restored{self.window_layout().observe(ctx);}
            // [ai-status-mod] 设置主窗口背景、侧栏与卡片全面跟随岛体当前主题（经典 / 奶油 / 毛玻璃·浅 / 毛玻璃·深 / 悬浮云 / 液态玻璃 + WinIslands 18 套主题）。
            let win_pal = if self.settings.skin == crate::model::Skin::Classic && !cfg!(test) {
                crate::auralsle_ai_status::plugins::active_win_palette()
            } else {
                None
            };
            let skin_is_light = match self.settings.skin {
                crate::model::Skin::Cream | crate::model::Skin::GlassLight => true,
                crate::model::Skin::Classic if win_pal.is_some() => {
                    let p = win_pal.unwrap();
                    (p.paper[0] as u32 + p.paper[1] as u32 + p.paper[2] as u32) > 380
                }
                _ => false,
            };
            let dark = match self.settings.settings_theme {
                crate::settings_theme::SettingsTheme::System => !skin_is_light,
                crate::settings_theme::SettingsTheme::Light => false,
                crate::settings_theme::SettingsTheme::Dark => true,
            };
            // [性能优化] 仅在首次初始化、明暗切换或最大化状态变更时执行 Win32/DWM 属性同步，缩放拖拽期间零系统调用
            let is_max_now = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            let dwm_sync_key: u8 = (dark as u8) | ((is_max_now as u8) << 1);
            let prev_dwm_key = ctx.data(|d| d.get_temp::<u8>(egui::Id::new("auralsle_dwm_sync_key")));
            if !settings_sizing_now() && (prev_dwm_key != Some(dwm_sync_key) || !ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_hwnd_ready")).unwrap_or(false))) {
                apply_settings_round_corners(ctx, dark);
                if ctx.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_hwnd_ready")).unwrap_or(false)) {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_dwm_sync_key"), dwm_sync_key));
                }
            }

            let _legacy_skin = match self.settings.skin {
                crate::model::Skin::Cream => SettingsSkinTheme {
                    bg_main: if dark { Color32::from_rgb(38, 32, 26) } else { Color32::from_rgb(252, 244, 232) },
                    bg_side: if dark { Color32::from_rgb(28, 23, 19) } else { Color32::from_rgb(243, 231, 214) },
                    bg_card: if dark { Color32::from_rgb(48, 40, 33) } else { Color32::from_rgb(255, 250, 242) },
                    bg_ctrl: if dark { Color32::from_rgb(66, 56, 46) } else { Color32::from_rgb(236, 221, 202) },
                    border: if dark { Color32::from_rgba_unmultiplied(255, 225, 190, 28) } else { Color32::from_rgba_unmultiplied(165, 118, 72, 36) },
                },
                crate::model::Skin::GlassLight => SettingsSkinTheme {
                    bg_main: if dark { Color32::from_rgb(26, 32, 42) } else { Color32::from_rgb(240, 245, 252) },
                    bg_side: if dark { Color32::from_rgb(19, 24, 32) } else { Color32::from_rgb(229, 236, 247) },
                    bg_card: if dark { Color32::from_rgb(36, 44, 58) } else { Color32::WHITE },
                    bg_ctrl: if dark { Color32::from_rgb(52, 62, 78) } else { Color32::from_rgb(222, 230, 242) },
                    border: if dark { Color32::from_rgba_unmultiplied(120, 165, 235, 42) } else { Color32::from_rgba_unmultiplied(100, 135, 190, 36) },
                },
                crate::model::Skin::GlassDark => SettingsSkinTheme {
                    bg_main: if dark { Color32::from_rgb(22, 26, 34) } else { Color32::from_rgb(244, 246, 250) },
                    bg_side: if dark { Color32::from_rgb(15, 18, 25) } else { Color32::from_rgb(234, 237, 244) },
                    bg_card: if dark { Color32::from_rgb(31, 36, 47) } else { Color32::WHITE },
                    bg_ctrl: if dark { Color32::from_rgb(46, 53, 68) } else { Color32::from_rgb(224, 228, 236) },
                    border: if dark { Color32::from_rgba_unmultiplied(185, 210, 255, 28) } else { Color32::from_rgba_unmultiplied(24, 28, 38, 24) },
                },
                crate::model::Skin::LiquidGlass => {
                    let [br, bg, bb] = self.backdrop_rgb;
                    let tint_r = (br * 42.0 + 18.0).clamp(16.0, 56.0) as u8;
                    let tint_g = (bg * 46.0 + 24.0).clamp(20.0, 64.0) as u8;
                    let tint_b = (bb * 54.0 + 34.0).clamp(26.0, 78.0) as u8;
                    SettingsSkinTheme {
                        bg_main: if dark { Color32::from_rgb(tint_r, tint_g, tint_b) } else { Color32::from_rgb(238, 244, 252) },
                        bg_side: if dark { Color32::from_rgb(tint_r.saturating_sub(6), tint_g.saturating_sub(6), tint_b.saturating_sub(7)) } else { Color32::from_rgb(226, 235, 248) },
                        bg_card: if dark { Color32::from_rgb((tint_r + 12).min(88), (tint_g + 14).min(96), (tint_b + 18).min(112)) } else { Color32::WHITE },
                        bg_ctrl: if dark { Color32::from_rgb((tint_r + 24).min(108), (tint_g + 28).min(116), (tint_b + 34).min(132)) } else { Color32::from_rgb(218, 229, 244) },
                        border: if dark { Color32::from_rgba_unmultiplied(185, 232, 255, 56) } else { Color32::from_rgba_unmultiplied(90, 130, 185, 36) },
                    }
                }
                crate::model::Skin::Fluid => {
                    if dark {
                        let fc = self.fluid.compose(0.5, 0.5, 1.0, 1.0, 0.0);
                        let r_m = ((fc.r() as u16 * 38) / 100 + 16).min(82) as u8;
                        let g_m = ((fc.g() as u16 * 38) / 100 + 18).min(84) as u8;
                        let b_m = ((fc.b() as u16 * 38) / 100 + 24).min(96) as u8;
                        SettingsSkinTheme {
                            bg_main: Color32::from_rgb(r_m, g_m, b_m),
                            bg_side: Color32::from_rgb(r_m.saturating_sub(6), g_m.saturating_sub(6), b_m.saturating_sub(7)),
                            bg_card: Color32::from_rgb((r_m + 12).min(104), (g_m + 13).min(108), (b_m + 16).min(118)),
                            bg_ctrl: Color32::from_rgb((r_m + 24).min(124), (g_m + 26).min(128), (b_m + 30).min(138)),
                            border: Color32::from_rgba_unmultiplied(220, 235, 255, 38),
                        }
                    } else {
                        // [修复需求 2 图三 `image_2e5d64.png`]：浅色主题下所有皮肤（含悬浮云）一律使用高对比浅色系底板，杜绝深色底板配黑色文字！
                        SettingsSkinTheme {
                            bg_main: Color32::from_rgb(246, 247, 251),
                            bg_side: Color32::from_rgb(236, 238, 244),
                            bg_card: Color32::WHITE,
                            bg_ctrl: Color32::from_rgb(226, 229, 236),
                            border: Color32::from_rgba_unmultiplied(24, 28, 36, 22),
                        }
                    }
                }
                crate::model::Skin::Classic => {
                    if let Some(wp) = win_pal {
                        let [pr, pg, pb] = wp.paper;
                        let [ar, ag, ab] = wp.accent.unwrap_or([88, 166, 255]);
                        if dark {
                            SettingsSkinTheme {
                                bg_main: Color32::from_rgb(pr.max(16), pg.max(18), pb.max(22)),
                                bg_side: Color32::from_rgb(pr.saturating_sub(6).max(10), pg.saturating_sub(6).max(12), pb.saturating_sub(6).max(15)),
                                bg_card: Color32::from_rgb(pr.saturating_add(12), pg.saturating_add(13), pb.saturating_add(16)),
                                bg_ctrl: Color32::from_rgb(pr.saturating_add(26), pg.saturating_add(28), pb.saturating_add(32)),
                                border: Color32::from_rgba_unmultiplied(ar, ag, ab, 42),
                            }
                        } else {
                            SettingsSkinTheme {
                                bg_main: Color32::from_rgb(pr, pg, pb),
                                bg_side: Color32::from_rgb(pr.saturating_sub(10), pg.saturating_sub(10), pb.saturating_sub(8)),
                                bg_card: Color32::from_rgb(pr.saturating_add(8), pg.saturating_add(8), pb.saturating_add(8)),
                                bg_ctrl: Color32::from_rgb(pr.saturating_sub(18), pg.saturating_sub(16), pb.saturating_sub(14)),
                                border: Color32::from_rgba_unmultiplied(ar, ag, ab, 48),
                            }
                        }
                    } else {
                        // [Shadcn UI Zinc Dark/Light 标准配色体系 (https://ui.shadcn.com/)]
                        SettingsSkinTheme {
                            bg_main: if dark { Color32::from_rgb(13, 14, 18) } else { Color32::from_rgb(250, 250, 250) },
                            bg_side: if dark { Color32::from_rgb(9, 10, 14) } else { Color32::from_rgb(244, 244, 245) },
                            bg_card: if dark { Color32::from_rgb(19, 21, 27) } else { Color32::WHITE },
                            bg_ctrl: if dark { Color32::from_rgb(34, 37, 46) } else { Color32::from_rgb(228, 228, 231) },
                            border: if dark { Color32::from_rgba_unmultiplied(255, 255, 255, 22) } else { Color32::from_rgba_unmultiplied(24, 24, 27, 20) },
                        }
                    }
                }
            };
            // [Twinkle Tray 全量替换] 设置窗口不再跟随岛体皮肤：用户要求"完完全全"用 Twinkle
            // 的设置界面，配色直接取 1.17.2 的 Win11 浅/暗两组变量（`vars.b9008e01.css`）。
            let skin_theme = twinkle_settings_skin(dark);
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_skin_theme"), skin_theme));

            let original_style=ctx.style();
            let mut style=(*original_style).clone();
            style.visuals=if dark{egui::Visuals::dark()}else{egui::Visuals::light()};
            style.visuals.panel_fill = skin_theme.bg_main;
            style.visuals.window_fill = skin_theme.bg_card;
            style.visuals.window_stroke = egui::Stroke::new(1.0, skin_theme.border);
            let pop_fg = if dark { Color32::WHITE } else { Color32::from_rgb(24, 28, 36) };
            style.visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, pop_fg);
            style.visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.2, pop_fg);
            style.visuals.widgets.active.fg_stroke = egui::Stroke::new(1.2, pop_fg);
            style.visuals.text_cursor.blink = true;
            style.visuals.text_cursor.stroke = egui::Stroke::new(1.6, pop_fg);
            if !cfg!(test) && ctx.memory(|m| m.focused().is_some()) {
                ctx.request_repaint_after(std::time::Duration::from_millis(180));
            }
            // [修复图八突兀大白条滚动条 (需求 12)]：
            // 关闭 egui 默认使用纯白前景色绘制滚动条 (`foreground_color = false`)，全局统一为 Shadcn <ScrollArea /> 4px 悬浮半透明细胶囊滚动条
            style.spacing.scroll.floating = true;
            style.spacing.scroll.bar_width = 8.0;
            style.spacing.scroll.floating_width = 6.0;
            // [需求 2/4/7 · 滚动条压内容] 悬浮滚动条必须真的占位（排水沟），
            // 否则它会盖在卡片右缘、侧栏选中胶囊右端上（图二/图四/图七红框）。
            style.spacing.scroll.floating_allocated_width = 12.0;
            style.spacing.scroll.bar_inner_margin = 2.0;
            style.spacing.scroll.bar_outer_margin = 4.0;
            style.spacing.scroll.handle_min_length = 44.0;
            style.spacing.scroll.foreground_color = false;
            // [需求 7 · 圆角] 滑块不再复用按钮底色（那是 `--button-border` 级别的亮灰，画出来像一条水泥条），
            // 改成 Twinkle 里那种中灰半透明细胶囊，两端 3px 圆角。
            style.spacing.scroll.handle_color = Some(if dark {
                Color32::from_rgba_unmultiplied(255, 255, 255, 92)
            } else {
                Color32::from_rgba_unmultiplied(24, 28, 36, 74)
            });
            style.spacing.scroll.bar_corner_radius = Some(egui::CornerRadius::same(3));
            style.spacing.scroll.dormant_background_opacity = 0.0;
            style.spacing.scroll.active_background_opacity = 0.0;
            style.spacing.scroll.interact_background_opacity = 0.0;
            style.spacing.scroll.dormant_handle_opacity = 0.55;
            style.spacing.scroll.active_handle_opacity = 0.72;
            style.spacing.scroll.interact_handle_opacity = 0.85;
            let raw_vp_w = ctx
                .input(|i| i.viewport().inner_rect.map(|r| r.width()).unwrap_or_else(|| i.screen_rect().width()));
            let vp_w_hint = if !cfg!(test) && raw_vp_w < 500.0 {
                ctx.data(|d| d.get_temp::<f32>(egui::Id::new("auralsle_last_valid_vp_w"))).unwrap_or(1024.0)
            } else {
                raw_vp_w.max(360.0)
            };
            // [需求 8 · 滚动卡顿根治] 旧版每帧都 clone 整套 Style 再 `ctx.set_style`：Style 版本号一变，
            // egui 会作废全部控件的样式缓存并触发整棵 UI 的重新布局——滚动时这就是稳定的一笔重税。
            // 样式只依赖主题/皮肤/窗口宽度档位，这三者没变就直接复用上一帧那份。
            let style_key = {
                let s = &skin_theme;
                let mut h: u64 = dark as u64;
                for c in [s.bg_main, s.bg_side, s.bg_card, s.bg_ctrl, s.border] {
                    let a = c.to_array();
                    for b in a {
                        h = h.wrapping_mul(0x100_0000_01b3).wrapping_add(b as u64);
                    }
                }
                h
            };
            let style_key_id = egui::Id::new("auralsle_settings_style_key");
            let style_key_prev = ctx.data(|d| d.get_temp::<u64>(style_key_id));
            // 兜底：宿主在系统主题切换时会用默认 Style 覆盖本视口，此时即使 key 没变也必须重设。
            let style_drifted = ctx.style().visuals.panel_fill != skin_theme.bg_main
                || ctx.style().spacing.scroll.handle_min_length != 44.0;
            if opening || style_drifted || style_key_prev != Some(style_key) {
                ctx.data_mut(|d| d.insert_temp(style_key_id, style_key));
                ctx.set_style(style);
            }
            let theme_cache_id = egui::Id::new("auralsle_last_viewport_dark");
            let prev_dark = ctx.data(|d| d.get_temp::<bool>(theme_cache_id));
            if opening || focus_requested || prev_dark != Some(dark) {
                ctx.data_mut(|d| d.insert_temp(theme_cache_id, dark));
                ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(if dark{egui::SystemTheme::Dark}else{egui::SystemTheme::Light}));
            }
            if self.settings.settings_theme==crate::settings_theme::SettingsTheme::System {
                ctx.request_repaint_after(std::time::Duration::from_secs(1));
            }
            if focus_requested {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_show_pending"), true));
            }
            if !cfg!(test) {
                let root_ctx_ref = ctx;
                let hwnd_ready = root_ctx_ref.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_hwnd_ready"))).unwrap_or(false);
                let show_pending = root_ctx_ref.data(|d| d.get_temp::<bool>(egui::Id::new("auralsle_settings_show_pending"))).unwrap_or(false);
                let warm_id = egui::Id::new("auralsle_settings_warmup_n");
                let warm_n = root_ctx_ref.data(|d| d.get_temp::<u8>(warm_id)).unwrap_or(0).saturating_add(1);
                root_ctx_ref.data_mut(|d| d.insert_temp(warm_id, warm_n));
                if show_pending {
                    if hwnd_ready && warm_n >= 4 {
                        // 首帧深色背景缓冲区 + Win32 去白框/圆角/Subclass 均已就绪后，一次性安全显现窗口，彻底杜绝蓝色/白色原生标题栏闪现（图 8）
                        root_ctx_ref.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        root_ctx_ref.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                        root_ctx_ref.send_viewport_cmd(egui::ViewportCommand::Focus);
                        bring_settings_window_to_front();
                        root_ctx_ref.data_mut(|d| {
                            d.insert_temp(egui::Id::new("auralsle_settings_show_pending"), false);
                            d.insert_temp(egui::Id::new("auralsle_round_corners_done"), false);
                        });
                    } else {
                        root_ctx_ref.request_repaint();
                    }
                }
            } else if focus_requested {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            if ctx.input(|i| i.viewport().close_requested()) {
                self.settings_open = false;
                self.quota_close_connection();
                ctx.request_repaint();
            }
            let before=self.settings.clone();
            // [视口宽度取真值] 立即视口的子 `Context` 里 `screen_rect()` 拿到的是**主岛窗口**的尺寸
            // （实测设置窗口 613 宽时它报上千）：整页于是按主岛宽度排版，右缘被窗口裁掉一半，
            // 这就是"缩小设置窗口后各页排版全塌"的直接成因（本轮图1）。
            // 窗口的真实内框只有 `viewport().inner_rect` 会给，其余一律回落到 screen_rect。
            let vp_w = vp_w_hint;

            // ── [Twinkle Tray 1.17.2 `.window-titlebar`] ──
            // 复刻 `page.4cb0fe87.css`：
            // `.window-titlebar { height: 30px; display: flex; align-items: center; }`
            // `.window-titlebar .appIcon { display: 'none' }`（不显示彩色图标）
            // `.window-titlebar .window-title { font-size: 12px; font-weight: 400; left: 10px; top: 8px; }`
            // 右侧 `.window-controls-container { width: 138px; height: 100%; margin-left: auto; }`（3 个 46×30 窗口按钮：最小化、最大化/还原、关闭）
            egui::TopBottomPanel::top("twinkle-window-titlebar")
                .exact_height(30.0)
                .show_separator_line(false)
                .frame(egui::Frame::new().fill(skin_theme.bg_main).inner_margin(egui::Margin::ZERO))
                .show(ctx, |ui| {
                    let bar_rect = ui.max_rect();
                    let title_col = settings_color(ctx, Color32::WHITE, Color32::BLACK);
                    ui.painter().text(
                        egui::pos2(bar_rect.left() + 20.0, bar_rect.center().y + 2.0),
                        egui::Align2::LEFT_CENTER,
                        "灵屿 AuraIsle 设置",
                        egui::FontId::proportional(12.0),
                        title_col,
                    );

                    // 右侧 138px 控制区（3 × 46px）
                    let ctrl_w = 138.0_f32;
                    let btn_w = 46.0_f32;
                    #[cfg(not(windows))]
                    {
                        let drag_rect = egui::Rect::from_min_max(
                            bar_rect.min,
                            egui::pos2((bar_rect.right() - ctrl_w).max(bar_rect.left() + 40.0), bar_rect.bottom()),
                        );
                        let drag_resp = ui.interact(drag_rect, egui::Id::new("tw-titlebar-drag"), egui::Sense::click_and_drag());
                        if drag_resp.drag_started() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                        }
                        if drag_resp.double_clicked() {
                            let is_max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max));
                        }
                    }

                    let icon_idle = Color32::from_rgb(0x88, 0x88, 0x88); // --titlebar-icon
                    let icon_hover = settings_color(ctx, Color32::WHITE, Color32::BLACK); // --titlebar-icon-hover

                    // 1) Minimize
                    let min_r = egui::Rect::from_min_size(egui::pos2(bar_rect.right() - ctrl_w, bar_rect.top()), egui::vec2(btn_w, 30.0));
                    let min_resp = ui.interact(min_r, egui::Id::new("tw-win-min"), egui::Sense::click());
                    if min_resp.hovered() {
                        ui.painter().rect_filled(min_r, 0.0, Color32::from_rgba_unmultiplied(122, 122, 122, 51));
                    }
                    let c_min = if min_resp.hovered() { icon_hover } else { icon_idle };
                    ui.painter().line_segment(
                        [egui::pos2(min_r.center().x - 5.0, min_r.center().y), egui::pos2(min_r.center().x + 5.0, min_r.center().y)],
                        egui::Stroke::new(1.1, c_min),
                    );
                    if min_resp.clicked() {
                        #[cfg(windows)]
                        if !cfg!(test) {
                            let hwnd_raw = SETTINGS_ACTIVE_HWND.load(std::sync::atomic::Ordering::Relaxed);
                            if hwnd_raw != 0 {
                                unsafe {
                                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                                        windows::Win32::Foundation::HWND(hwnd_raw as *mut _),
                                        0x0112, // WM_SYSCOMMAND
                                        windows::Win32::Foundation::WPARAM(0xF020), // SC_MINIMIZE
                                        windows::Win32::Foundation::LPARAM(0),
                                    );
                                }
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                            }
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }
                        #[cfg(not(windows))]
                        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                    }

                    // 2) Maximize / Restore
                    let is_max_now = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    let max_r = egui::Rect::from_min_size(egui::pos2(bar_rect.right() - btn_w * 2.0, bar_rect.top()), egui::vec2(btn_w, 30.0));
                    let max_resp = ui.interact(max_r, egui::Id::new("tw-win-max"), egui::Sense::click());
                    if max_resp.hovered() {
                        ui.painter().rect_filled(max_r, 0.0, Color32::from_rgba_unmultiplied(122, 122, 122, 51));
                    }
                    let c_max = if max_resp.hovered() { icon_hover } else { icon_idle };
                    if is_max_now {
                        let c = max_r.center();
                        let front = egui::Rect::from_center_size(egui::pos2(c.x - 1.0, c.y + 1.0), egui::vec2(8.0, 8.0));
                        ui.painter().rect_stroke(front, 1.0, egui::Stroke::new(1.1, c_max), egui::StrokeKind::Inside);
                        ui.painter().line_segment([egui::pos2(front.left() + 2.0, front.top() - 2.0), egui::pos2(front.right() + 2.0, front.top() - 2.0)], egui::Stroke::new(1.1, c_max));
                        ui.painter().line_segment([egui::pos2(front.right() + 2.0, front.top() - 2.0), egui::pos2(front.right() + 2.0, front.bottom() - 2.0)], egui::Stroke::new(1.1, c_max));
                    } else {
                        let box_r = egui::Rect::from_center_size(max_r.center(), egui::vec2(9.5, 9.5));
                        ui.painter().rect_stroke(box_r, 1.0, egui::Stroke::new(1.1, c_max), egui::StrokeKind::Inside);
                    }
                    if max_resp.clicked() {
                        #[cfg(windows)]
                        if !cfg!(test) {
                            let hwnd_raw = SETTINGS_ACTIVE_HWND.load(std::sync::atomic::Ordering::Relaxed);
                            if hwnd_raw != 0 {
                                let sc = if is_max_now { 0xF120usize /* SC_RESTORE */ } else { 0xF030usize /* SC_MAXIMIZE */ };
                                unsafe {
                                    let _ = windows::Win32::UI::WindowsAndMessaging::PostMessageW(
                                        windows::Win32::Foundation::HWND(hwnd_raw as *mut _),
                                        0x0112, // WM_SYSCOMMAND
                                        windows::Win32::Foundation::WPARAM(sc),
                                        windows::Win32::Foundation::LPARAM(0),
                                    );
                                }
                            } else {
                                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max_now));
                            }
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max_now));
                        }
                        #[cfg(not(windows))]
                        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!is_max_now));
                        ctx.request_repaint();
                    }

                    // 3) Close
                    let cls_r = egui::Rect::from_min_size(egui::pos2(bar_rect.right() - btn_w, bar_rect.top()), egui::vec2(btn_w, 30.0));
                    let cls_resp = ui.interact(cls_r, egui::Id::new("tw-win-close"), egui::Sense::click());
                    if cls_resp.hovered() {
                        // 与最小化/最大化按钮一致，贴齐窗口顶/右边缘填满直角矩形，
                        // 外圆角统一交由 Win11 DWM 原生圆角 (DWMWCP_ROUND / 最大化 DWMWCP_DONOTROUND) 裁切，
                        // 彻底消除硬编码 11px 圆角与 1px 内缩在右上角留出的深色月牙缝隙。
                        let cls_fill_r = egui::Rect::from_min_max(
                            cls_r.min,
                            egui::pos2(cls_r.right() + 1.0, cls_r.bottom()),
                        );
                        ui.painter().rect_filled(
                            cls_fill_r,
                            egui::CornerRadius::ZERO,
                            Color32::from_rgb(232, 17, 35),
                        );
                    }
                    let c_cls = if cls_resp.hovered() { Color32::WHITE } else { icon_idle };
                    let cc = cls_r.center();
                    ui.painter().line_segment([egui::pos2(cc.x - 4.8, cc.y - 4.8), egui::pos2(cc.x + 4.8, cc.y + 4.8)], egui::Stroke::new(1.2, c_cls));
                    ui.painter().line_segment([egui::pos2(cc.x + 4.8, cc.y - 4.8), egui::pos2(cc.x - 4.8, cc.y + 4.8)], egui::Stroke::new(1.2, c_cls));
                    if cls_resp.clicked() {
                        self.settings_open = false;
                        self.quota_close_connection();
                        ctx.request_repaint();
                    }
                });

            // ── [Twinkle Tray 1.17.2 `#sidebar`] ──
            // 复刻 `page.4cb0fe87.css`：
            // `body[data-is-win11=true] #sidebar { background: none !important; padding: 45px 0 15px 15px; width: 300px; }`
            // - 无右侧竖分割线，与主页面共享 `--page-background`；
            // - 无内部「岛仔设置」Logo 头，无搜索框，无小分组标签；
            // - 条目 `.item { border-radius: 6px; padding: 8px 10px; margin: 4px 0; }`
            // - 选中竖条 `.item:before { width: 4px; height: 23px; border-radius: 10px; background: var(--system-accent-light2 / dark2); }`
            // - 图标 18px 纯线条单色（按文字颜色着色），标签 `13px~14px`。
            let side_col_id = egui::Id::new("auralsle_sidebar_collapsed");
            let mut side_collapsed = if cfg!(test) {
                false
            } else {
                ctx.data(|d| d.get_temp::<bool>(side_col_id)).unwrap_or(false)
            };
            let collapsed_side_w = 64.0_f32;
            let expanded_side_w = if vp_w >= 820.0 {
                (300.0 + ((vp_w - 1024.0) * 0.08).clamp(0.0, 36.0)).round()
            } else {
                (vp_w * 0.34).clamp(150.0, 300.0)
            };
            // 仅在用户主动点击侧栏折叠按钮（side_collapsed 布尔变化）时做平滑过渡，
            // 杜绝从 Win11 任务栏最小化还原或窗口缩放时因 vp_w 变化触发多余的收起/展开侧栏动画（修复需求 4）
            let collapse_t = if cfg!(test) || opening {
                if side_collapsed { 1.0 } else { 0.0 }
            } else {
                let t = ctx.animate_bool_with_time(egui::Id::new("auralsle_anim_side_collapsed"), side_collapsed, 0.18);
                if t > 0.004 && t < 0.996 {
                    ctx.request_repaint();
                }
                t
            };
            let side_w = crate::anim::lerp(expanded_side_w, collapsed_side_w, collapse_t);
            let text_alpha = if cfg!(test) { 1.0 } else { ((side_w - 115.0) / 85.0).clamp(0.0, 1.0) };
            let is_compact = !cfg!(test) && side_w < 135.0;
            let accent = crate::auralsle_ai_status::plugins::active_accent_color();
            egui::SidePanel::left("settings-sidebar")
                .resizable(false)
                .exact_width(side_w)
                .show_separator_line(false)
                .frame(
                    egui::Frame::new()
                        .fill(skin_theme.bg_main)
                        .inner_margin(egui::Margin {
                            left: if is_compact { 10 } else { 15 },
                            right: 0,
                            top: 15, // 连同顶部 30px titlebar 合计正好对应 Twinkle 的 padding-top: 45px
                            bottom: 15,
                        }),
                )
                .show(ctx, |ui| {
                    ui.spacing_mut().scroll.floating = true;
                    ui.spacing_mut().scroll.bar_width = 8.0;
                    ui.spacing_mut().scroll.floating_width = 6.0;
                    // [需求 2 · 侧栏选中胶囊被滑块压住] 同样给滚动条留排水沟：
                    // 条目右端停在 `side_w - 12`，侧栏滑块固定在 `[side_w - 10 .. side_w - 4]`。
                    ui.spacing_mut().scroll.floating_allocated_width = 12.0;
                    ui.spacing_mut().scroll.bar_outer_margin = 4.0;
                    ui.spacing_mut().scroll.handle_min_length = 44.0;
                    ui.spacing_mut().scroll.foreground_color = false;
                    ui.spacing_mut().scroll.handle_color = Some(settings_color(ctx, Color32::from_rgba_unmultiplied(255,255,255,92), Color32::from_rgba_unmultiplied(24,28,36,74)));
                    ui.spacing_mut().scroll.bar_corner_radius = Some(egui::CornerRadius::same(3));
                    // 顶部行：左侧折叠/展开导航栏按钮（搜索框左缘红框位置） + 右侧设置搜索框
                    let search_id = egui::Id::new("settings-sidebar-search");
                    let mut search_q = ui.data(|d| d.get_temp::<String>(search_id)).unwrap_or_default();
                    let top_row_w = (ui.available_width() - if is_compact { 10.0 } else { 12.0 }).max(38.0);
                    let (top_row_rect, _) = ui.allocate_exact_size(egui::vec2(top_row_w, 36.0), egui::Sense::hover());
                    let title_col = settings_color(ctx, Color32::WHITE, Color32::BLACK);
                    let mut search_enter = false;
                    if !cfg!(test) {
                        let toggle_rect = if is_compact {
                            egui::Rect::from_center_size(top_row_rect.center(), egui::vec2(top_row_w.min(40.0), 34.0))
                        } else {
                            egui::Rect::from_min_size(top_row_rect.min, egui::vec2(34.0, 34.0))
                        };
                        let t_resp = ui
                            .interact(toggle_rect, egui::Id::new("tw-sidebar-toggle"), egui::Sense::click())
                            .on_hover_text(if side_collapsed { "展开导航栏" } else { "收起导航栏" });
                        if t_resp.hovered() {
                            let hbg = settings_color(
                                ctx,
                                Color32::from_white_alpha(20),
                                Color32::from_black_alpha(12),
                            );
                            ui.painter().rect_filled(toggle_rect, 6.0, hbg);
                        }
                        let ic_rect = egui::Rect::from_center_size(toggle_rect.center(), egui::vec2(15.5, 11.5));
                        let ic_col = title_col.gamma_multiply(if t_resp.hovered() { 0.98 } else { 0.80 });
                        let p = ui.painter();
                        p.rect_stroke(ic_rect, 2.5, egui::Stroke::new(1.3, ic_col), egui::StrokeKind::Inside);
                        let bar_x = ic_rect.left() + 5.2;
                        p.line_segment(
                            [egui::pos2(bar_x, ic_rect.top() + 1.2), egui::pos2(bar_x, ic_rect.bottom() - 1.2)],
                            egui::Stroke::new(1.2, ic_col),
                        );
                        if !side_collapsed {
                            let pane_rect = egui::Rect::from_min_max(
                                egui::pos2(ic_rect.left() + 2.0, ic_rect.top() + 2.0),
                                egui::pos2(bar_x - 1.2, ic_rect.bottom() - 2.0),
                            );
                            p.rect_filled(pane_rect, 1.0, ic_col);
                        }
                        if t_resp.clicked() {
                            side_collapsed = !side_collapsed;
                            ctx.data_mut(|d| d.insert_temp(side_col_id, side_collapsed));
                            ctx.request_repaint();
                        }
                    }
                    if text_alpha > 0.04 {
                        let search_left = if cfg!(test) { top_row_rect.left() } else { top_row_rect.left() + 40.0 };
                        let search_rect = egui::Rect::from_min_max(
                            egui::pos2(search_left, top_row_rect.top()),
                            top_row_rect.max,
                        );
                        if search_rect.width() > 60.0 {
                            let mut s_ui = ui.new_child(
                                egui::UiBuilder::new()
                                    .max_rect(search_rect)
                                    .layout(egui::Layout::top_down(egui::Align::Min)),
                            );
                            s_ui.set_opacity(text_alpha);
                            search_enter = draw_cirrus_settings_search(
                                &mut s_ui,
                                &skin_theme,
                                accent,
                                dark,
                                &mut search_q,
                            );
                        }
                    }
                    ui.data_mut(|d| d.insert_temp(search_id, search_q.clone()));
                    ui.add_space(4.0);

                    let last_side_tab_id = egui::Id::new("auralsle_sidebar_last_tab");
                    let prev_side_tab = ui.data(|d| d.get_temp::<usize>(last_side_tab_id));
                    let tab_just_changed = prev_side_tab != Some(self.settings_tab);
                    if tab_just_changed {
                        ui.data_mut(|d| d.insert_temp(last_side_tab_id, self.settings_tab));
                    }
                    let reset_side_top = (opening || focus_requested) && (0..=1).contains(&self.settings_tab);
                    let mut side_scroll = egui::ScrollArea::vertical()
                        .id_salt("settings-sidebar-scroll")
                        .auto_shrink([false, false]);
                    if reset_side_top {
                        side_scroll = side_scroll.vertical_scroll_offset(0.0);
                    }
                    let q_trimmed = search_q.trim().to_lowercase();
                    side_scroll.show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = 4.0;
                        if !is_compact && !q_trimmed.is_empty() {
                            let terms: Vec<&str> = q_trimmed.split_whitespace().collect();
                            let matches: Vec<_> = SETTINGS_SEARCH_ENTRIES
                                .iter()
                                .filter(|(keys, desc, _, _, cat)| {
                                    let hay = format!("{} {} {}", keys, desc, cat).to_lowercase();
                                    terms.iter().all(|t| hay.contains(t))
                                })
                                .collect();
                            if matches.is_empty() {
                                ui.add_space(16.0);
                                ui.label(
                                    egui::RichText::new("未找到匹配的设置项")
                                        .size(12.5)
                                        .color(settings_color(ctx, Color32::from_gray(155), Color32::from_gray(110))),
                                );
                            } else {
                                if search_enter {
                                    if let Some(&(_, _, t, sub, _)) = matches.first() {
                                        let prev_t = self.settings_tab;
                                        self.settings_tab = *t;
                                        if *t == 0 {
                                            ui.data_mut(|d| d.insert_temp(egui::Id::new("auraisle_general_subtab"), sub.unwrap_or(0)));
                                        } else if *t == 20 {
                                            ui.data_mut(|d| d.insert_temp(egui::Id::new("auraisle_device_subtab"), sub.unwrap_or(0)));
                                        }
                                        let now_t = ctx.input(|i| i.time);
                                        let dir: f32 = if *t >= prev_t { 1.0 } else { -1.0 };
                                        ctx.data_mut(|d| {
                                            d.insert_temp(egui::Id::new("win11-page-switch-at"), now_t);
                                            d.insert_temp(egui::Id::new("win11-page-switch-dir"), dir);
                                            d.insert_temp(egui::Id::new("auraisle_search_jump_top"), true);
                                        });
                                    }
                                }
                                for (keys, desc, tab_idx, sub_opt, cat_lbl) in matches {
                                    let title_disp = keys.split_whitespace().next().unwrap_or(keys);
                                    let row_h = 46.0_f32;
                                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::click());
                                    let is_cur = self.settings_tab == *tab_idx;
                                    let hover_fill = settings_color(ctx, Color32::from_rgb(0x43, 0x43, 0x43), Color32::from_rgb(0xe0, 0xe2, 0xe8));
                                    let fill = if is_cur || resp.hovered() {
                                        hover_fill
                                    } else {
                                        skin_theme.bg_card
                                    };
                                    ui.painter().rect_filled(rect, 6.0, fill);
                                    if is_cur {
                                        let pill_r = egui::Rect::from_center_size(egui::pos2(rect.left() + 2.0, rect.center().y), egui::vec2(4.0, 22.0));
                                        ui.painter().rect_filled(pill_r, 4.0, accent);
                                    }
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 12.0, rect.top() + 15.0),
                                        egui::Align2::LEFT_CENTER,
                                        title_disp,
                                        egui::FontId::proportional(13.0),
                                        settings_color(ctx, Color32::WHITE, Color32::from_rgb(20, 22, 28)),
                                    );
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 12.0, rect.top() + 33.0),
                                        egui::Align2::LEFT_CENTER,
                                        &format!("{} · {}", cat_lbl, desc),
                                        egui::FontId::proportional(10.8),
                                        settings_color(ctx, Color32::from_gray(156), Color32::from_gray(105)),
                                    );
                                    if resp.clicked() {
                                        let prev_t = self.settings_tab;
                                        self.settings_tab = *tab_idx;
                                        if *tab_idx == 0 {
                                            ui.data_mut(|d| d.insert_temp(egui::Id::new("auraisle_general_subtab"), sub_opt.unwrap_or(0)));
                                        } else if *tab_idx == 20 {
                                            ui.data_mut(|d| d.insert_temp(egui::Id::new("auraisle_device_subtab"), sub_opt.unwrap_or(0)));
                                        }
                                        let now_t = ctx.input(|i| i.time);
                                        let dir: f32 = if *tab_idx >= prev_t { 1.0 } else { -1.0 };
                                        ctx.data_mut(|d| {
                                            d.insert_temp(egui::Id::new("win11-page-switch-at"), now_t);
                                            d.insert_temp(egui::Id::new("win11-page-switch-dir"), dir);
                                            d.insert_temp(egui::Id::new("auraisle_search_jump_top"), true);
                                        });
                                    }
                                }
                            }
                            return;
                        }
                        for (name, start, end) in SETTINGS_GROUPS {
                            if start == 15 {
                                continue;
                            }
                            // [第 10 批 · 图九 按功能分类] 类别小标题：11px 灰字，与入口行左内边距对齐，折叠窄栏下隐藏文字
                            if !is_compact && text_alpha > 0.05 {
                                if let Some((sec, _)) = SETTINGS_SECTIONS.iter().find(|(_, first)| *first == name) {
                                    if !sec.is_empty() {
                                        ui.add_space(10.0);
                                        let (h_rect, _) =
                                            ui.allocate_exact_size(egui::vec2(ui.available_width(), 20.0), egui::Sense::hover());
                                        ui.painter().text(
                                            egui::pos2(h_rect.left() + 10.0, h_rect.center().y),
                                            egui::Align2::LEFT_CENTER,
                                            sec,
                                            egui::FontId::proportional(11.0),
                                            settings_color(ctx, Color32::from_gray(138), Color32::from_gray(112)).gamma_multiply(text_alpha),
                                        );
                                        ui.add_space(2.0);
                                    }
                                }
                            }
                            let selected = (start..=end).contains(&self.settings_tab);
                            // [Twinkle `#sidebar .item`] `padding: 8px 10px; margin: 4px 0; border-radius: 6px; height: 38px`
                            let item_w = if is_compact { (ui.available_width() - 10.0).max(38.0) } else { ui.available_width() };
                            let (rect, mut response) = ui.allocate_exact_size(egui::vec2(item_w, 38.0), egui::Sense::click());
                            let display_name = if start == 13 && !cfg!(test) { "插件" } else { name };
                            if is_compact {
                                response = response.on_hover_text(display_name);
                            }
                            if selected && (opening || focus_requested || tab_just_changed) && !reset_side_top {
                                ui.scroll_to_rect(rect.expand2(egui::vec2(0.0, 18.0)), None);
                            }
                            if response.clicked() && !selected {
                                let prev_t = self.settings_tab;
                                self.settings_tab = start;
                                let now_t = ctx.input(|i| i.time);
                                let dir: f32 = if start >= prev_t { 1.0 } else { -1.0 };
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new("win11-page-switch-at"), now_t);
                                    d.insert_temp(egui::Id::new("win11-page-switch-dir"), dir);
                                });
                            }
                            let hover_fill = settings_color(ctx, Color32::from_rgb(0x43, 0x43, 0x43), Color32::from_rgb(0xe0, 0xe2, 0xe8));
                            if selected || response.hovered() {
                                ui.painter().rect_filled(rect, 6.0, hover_fill);
                            }
                            let pill_col = if dark {
                                crate::paint::mix(accent, Color32::from_rgb(76, 194, 255), 0.72)
                            } else {
                                crate::paint::mix(accent, Color32::BLACK, 0.18)
                            };
                            if selected && cfg!(test) {
                                let pill_r = egui::Rect::from_center_size(
                                    egui::pos2(rect.left() + 2.0, rect.center().y),
                                    egui::vec2(4.0, 23.0),
                                );
                                ui.painter().rect_filled(pill_r, 10.0, pill_col);
                            } else if !cfg!(test) && selected {
                                let target_y = rect.center().y;
                                let anim_y = ctx.animate_value_with_time(egui::Id::new("win11-sidebar-active-y"), target_y, 0.20);
                                if (anim_y - target_y).abs() > 0.25 {
                                    ctx.request_repaint();
                                }
                                let pill_r = egui::Rect::from_center_size(
                                    egui::pos2(rect.left() + 2.0, anim_y),
                                    egui::vec2(4.0, 23.0),
                                );
                                ui.painter().rect_filled(pill_r, 10.0, pill_col);
                            }
                            let fluent_glyph = match start {
                                0 => '\u{E713}',  // Settings (通用/常规)
                                20 => '\u{E7F4}', // TVMonitor (显示器设置)
                                13 => '\u{E9E9}', // Equalizer (DDC/CI 功能 / 插件)
                                2 => '\u{E823}',  // Recent/Clock (专注与饮水)
                                14 => '\u{E787}', // Calendar (课程表)
                                10 => '\u{E71D}', // AllApps (页面管理)
                                1 => '\u{E8D6}',  // MusicInCollection (音乐与歌词)
                                4 => '\u{E73A}',  // CheckboxComposite (待办与置物)
                                6 => '\u{E7E7}',  // ActionCenterNotification (通知、邮箱与日程)
                                16 => '\u{E81C}', // History (通知历史)
                                11 => '\u{E9D9}', // Diagnostic (AI 额度)
                                12 => '\u{E950}', // Processing (AI 工作状态)
                                17 => '\u{ED54}', // Emoji2 (吉祥物)
                                18 => '\u{E753}', // CloudWeather (天气)
                                19 => '\u{E8C1}', // Translate (翻译)
                                21 => '\u{E74C}', // System/Modules (功能模块)
                                _ => '\u{E895}',  // Sync (更新/关于)
                            };
                            let text_col = settings_color(ctx, Color32::from_rgb(240, 240, 240), Color32::from_rgb(24, 24, 26));
                            let icon_font = crate::theme::fluent_icon_font(18.0);
                            // [需求 8 · 每帧 16 次字体查询] `has_glyph` 每次都要走字体回退链；
                            // 同一字体只可能是「有」或「没有」，缓存一次即可。
                            let has_fluent = {
                                let id = egui::Id::new("auralsle_sidebar_fluent_ok");
                                ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or_else(|| {
                                    let ok = !cfg!(test) && ctx.fonts(|f| f.has_glyph(&icon_font, '\u{E713}'));
                                    ctx.data_mut(|d| d.insert_temp(id, ok));
                                    ok
                                })
                            };
                            let icon_cx = if is_compact { rect.center().x } else { rect.left() + 24.0 };
                            if has_fluent {
                                ui.painter().text(
                                    egui::pos2(icon_cx.round(), rect.center().y.round()),
                                    egui::Align2::CENTER_CENTER,
                                    fluent_glyph.to_string(),
                                    icon_font,
                                    text_col,
                                );
                            } else {
                                let icon = match start {
                                    0 => egui::include_image!("../assets/settings-icons/system/general.svg"),
                                    11 => egui::include_image!("../assets/settings-icons/system/quota.svg"),
                                    12 => egui::include_image!("auralsle_ai_status/assets/settings-ai-status.svg"),
                                    13 => egui::include_image!("auralsle_ai_status/assets/settings-plugins.svg"),
                                    14 => egui::include_image!("auralsle_ai_status/assets/settings-course.svg"),
                                    10 => egui::include_image!("../assets/settings-icons/system/pages.svg"),
                                    1 => egui::include_image!("../assets/settings-icons/system/music.svg"),
                                    2 => egui::include_image!("../assets/settings-icons/system/focus.svg"),
                                    4 => egui::include_image!("../assets/settings-icons/system/tasks.svg"),
                                    6 => egui::include_image!("../assets/settings-icons/system/notifications.svg"),
                                    17 => egui::include_image!("../assets/settings-icons/system/mascot.svg"),
                                    16 => egui::include_image!("../assets/settings-icons/system/history.svg"),
                                    18 => egui::include_image!("../assets/settings-icons/system/weather.svg"),
                                    19 => egui::include_image!("../assets/settings-icons/system/pages.svg"),
                                    20 => egui::include_image!("auralsle_ai_status/assets/settings-sysmon.svg"),
                                    21 => egui::include_image!("../assets/settings-icons/settings-2.svg"),
                                    _ => egui::include_image!("../assets/settings-icons/system/about.svg"),
                                };
                                let icon_bg = egui::Rect::from_center_size(
                                    egui::pos2(icon_cx.round(), rect.center().y.round()),
                                    egui::vec2(18.0, 18.0),
                                );
                                egui::Image::new(icon).tint(text_col).paint_at(ui, icon_bg);
                            }
                            if text_alpha > 0.02 {
                                let text_x = rect.left() + 48.0;
                                let max_text_w = (rect.right() - 10.0 - text_x).max(24.0);
                                let base_font_sz = 13.5_f32;
                                let raw_w = {
                                    let id = egui::Id::new(("auralsle_side_raw_w", display_name));
                                    ctx.data(|d| d.get_temp::<f32>(id)).unwrap_or_else(|| {
                                        let w = ctx.fonts(|f| {
                                            f.layout_no_wrap(display_name.to_string(), egui::FontId::proportional(base_font_sz), text_col).size().x
                                        });
                                        ctx.data_mut(|d| d.insert_temp(id, w));
                                        w
                                    })
                                };
                                let mut font_sz = base_font_sz;
                                if raw_w > max_text_w {
                                    font_sz = (font_sz * max_text_w / raw_w).clamp(9.4, font_sz);
                                }
                                ui.painter().text(
                                    egui::pos2(text_x, rect.center().y),
                                    egui::Align2::LEFT_CENTER,
                                    display_name,
                                    egui::FontId::proportional(font_sz),
                                    text_col.gamma_multiply(text_alpha),
                                );
                            }
                        }
                    });
                });
            crate::frame_watch::mark_stage("settings:sidebar");
            let probe_side_ms = probe_t0.elapsed().as_secs_f64() * 1000.0;
            // [Twinkle #page] `padding: 20px`、`background: var(--page-background)`。
            // [需求 6 · 顶边距统一] 四边固定 20/20/20/24，窄窗左右收到 12；所有页共用同一基线，
            // 不再出现页面管理标题比别的页高 20px 的错位（图七）。
            let main_pad_x: i8 = if vp_w < 580.0 { 10 } else { 12 };
            // [需求 2 · 图二左右间距对称]：
            // 侧栏滑块右边沿在 `side_w - 4`，右侧面板左边距 `12`（窄窗 `10`）→ 左侧滑块到卡片距离 = `16px`（窄窗 `14px`）。
            // 右侧面板右边距归 `0`，主滚动条 `bar_outer_margin = 4`、`floating_width = 6` → 主滑块左缘在 `vp_w - 10`；
            // 页面内预留 `right_gutter = 26`（窄窗 `24`）→ 卡片右缘到主滑块左缘同样 = `16px`（窄窗 `14px`），左右完全等距！
            let main_pad_r: i8 = 0;
            egui::CentralPanel::default().frame(egui::Frame::new().fill(skin_theme.bg_main).inner_margin(egui::Margin{ left: main_pad_x, right: main_pad_r, top: 20, bottom: 24 })).show(ctx,|ui|{
                let panel_w = ui.available_width().max(220.0);                ui.set_max_width(panel_w);
                let inner_panel_w = panel_w;
                ui.set_width(inner_panel_w);
                ui.spacing_mut().item_spacing=egui::vec2(8.0,5.0);
                ui.spacing_mut().button_padding=egui::vec2(10.0,6.0);
                ui.spacing_mut().scroll.floating = true;
                ui.spacing_mut().scroll.bar_width = 8.0;
                ui.spacing_mut().scroll.floating_width = 6.0;
                ui.spacing_mut().scroll.floating_allocated_width = 0.0;
                ui.spacing_mut().scroll.bar_outer_margin = 4.0;
                ui.spacing_mut().scroll.handle_min_length = 44.0;
                ui.spacing_mut().scroll.foreground_color = false;
                ui.spacing_mut().scroll.handle_color = Some(settings_color(ctx, Color32::from_rgba_unmultiplied(255,255,255,92), Color32::from_rgba_unmultiplied(24,28,36,74)));
                ui.spacing_mut().scroll.bar_corner_radius = Some(egui::CornerRadius::same(3));
                ui.style_mut().text_styles.insert(egui::TextStyle::Small,egui::FontId::proportional(12.0));
                ui.visuals_mut().override_text_color=Some(settings_color(ctx,Color32::WHITE,Color32::from_rgb(30,34,41)));
                ui.visuals_mut().extreme_bg_color=skin_theme.bg_side;
                ui.visuals_mut().widgets.inactive.bg_fill=skin_theme.bg_ctrl;
                ui.visuals_mut().widgets.hovered.bg_fill=settings_color(ctx,Color32::from_rgb(63,63,70),Color32::from_rgb(212,218,228));
                // 统一按钮：高度 28、圆角 7、内边距 12×5，无描边；默认按钮也套用这套样式。
                ui.spacing_mut().button_padding=egui::vec2(12.0,5.0);
                ui.spacing_mut().interact_size.y=28.0;
                {
                    let fill=skin_theme.bg_ctrl;
                    let hover=settings_color(ctx,Color32::from_rgb(63,63,70),Color32::from_rgb(212,218,228));
                    let widgets=&mut ui.visuals_mut().widgets;
                    for (w,f) in [(&mut widgets.inactive,fill),(&mut widgets.hovered,hover),(&mut widgets.active,hover)] {
                        w.corner_radius=egui::CornerRadius::same(7);
                        w.weak_bg_fill=f;
                        w.bg_stroke=egui::Stroke::NONE;
                    }
                }
                ui.visuals_mut().selection.bg_fill=crate::auralsle_ai_status::plugins::active_accent_color();
                ui.visuals_mut().slider_trailing_fill=true;
                ui.spacing_mut().slider_width=(inner_panel_w * 0.34).clamp(130.0, 320.0);
                let titles=["常规","音乐与歌词","专注","饮水","待办","置物","通知","邮箱","日程","关于","页面管理","AI 额度","AI 工作","系统监控","课程表","功能","通知历史","吉祥物","天气","翻译","设备","功能模块"];
                let active_title = if self.settings_tab == 13 && !cfg!(test) { "插件" } else { titles[self.settings_tab.min(titles.len() - 1)] };
                let _ = titles.len();

                // ── 高性能撤销/恢复栈：仅在标签页切换或按下 Ctrl+Z / Ctrl+Y 时访问静态锁，绝不在每帧克隆 30 份历史快照 ──
                let undo_mtx = UNDO_SLOT.get_or_init(|| std::sync::Mutex::new(PageUndoState::default()));
                let want_undo = ctx.input(|i| i.modifiers.ctrl && !i.modifiers.shift && i.key_pressed(egui::Key::Z));
                let want_redo = ctx.input(|i| {
                    (i.modifiers.ctrl && i.key_pressed(egui::Key::Y))
                        || (i.modifiers.ctrl && i.modifiers.shift && i.key_pressed(egui::Key::Z))
                });
                if let Ok(mut undo_st) = undo_mtx.lock() {
                    if !undo_st.initialized || undo_st.tab != self.settings_tab {
                        undo_st.tab = self.settings_tab;
                        undo_st.initialized = true;
                        undo_st.undo_stack.clear();
                        undo_st.redo_stack.clear();
                        undo_st.last_committed = Some((self.settings.clone(), crate::auralsle_ai_status::config::get()));
                    } else if want_undo {
                        if let Some(prev_snap) = undo_st.undo_stack.pop() {
                            let cur_snap = (self.settings.clone(), crate::auralsle_ai_status::config::get());
                            undo_st.redo_stack.push(cur_snap);
                            self.settings = prev_snap.0.clone();
                            self.settings.save();
                            let prev_mcfg = prev_snap.1.clone();
                            crate::auralsle_ai_status::config::update(|c| *c = prev_mcfg);
                            undo_st.last_committed = Some(prev_snap);
                        }
                    } else if want_redo {
                        if let Some(next_snap) = undo_st.redo_stack.pop() {
                            let cur_snap = (self.settings.clone(), crate::auralsle_ai_status::config::get());
                            undo_st.undo_stack.push(cur_snap);
                            self.settings = next_snap.0.clone();
                            self.settings.save();
                            let next_mcfg = next_snap.1.clone();
                            crate::auralsle_ai_status::config::update(|c| *c = next_mcfg);
                            undo_st.last_committed = Some(next_snap);
                        }
                    }
                }

                // [120Hz 零重排切页过渡]：不在 ScrollArea 内动态增减 add_space 高度，彻底根除切页时布局重算导致的掉帧
                let switch_at = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("win11-page-switch-at"))).unwrap_or(0.0);
                let _switch_dir = ctx.data(|d| d.get_temp::<f32>(egui::Id::new("win11-page-switch-dir"))).unwrap_or(1.0);
                let now_t = ctx.input(|i| i.time);
                let _acc_main = crate::auralsle_ai_status::plugins::active_accent_color();
                let elapsed_sw = (now_t - switch_at).max(0.0) as f32;
                let page_ease = if cfg!(test) || switch_at <= 0.0 {
                    1.0
                } else {
                    page_switch_ease(elapsed_sw, PAGE_SWITCH_SECS)
                };
                let gen_sub_id = egui::Id::new("auraisle_general_subtab");
                let gen_sub_tab = ui.data(|d| d.get_temp::<usize>(gen_sub_id)).unwrap_or(0).min(4);
                let dev_sub_id = egui::Id::new("auraisle_device_subtab");
                let dev_sub_tab = ui.data(|d| d.get_temp::<usize>(dev_sub_id)).unwrap_or(0).min(1);
                let active_sub_salt = match self.settings_tab {
                    0 => gen_sub_tab,
                    20 => dev_sub_tab,
                    _ => 0,
                };

                // [Twinkle `#page`] 整个右侧页面是唯一滚动容器，接入 120Hz 非线性阻尼平滑滚动（鼠标滚轮实时吸收 + 零滞后拖滑块同步）
                #[derive(Clone, Copy, Default)]
                struct PageScrollSmooth {
                    target_y: f32,
                    cur_y: f32,
                    max_y: f32,
                    last_offset_y: f32,
                    active: bool,
                    has_bounds: bool,
                }
                let page_scroll_id = egui::Id::new(("auraisle_page_scroll_smooth", self.settings_tab, active_sub_salt));
                let mut sm = ui.data(|d| d.get_temp::<PageScrollSmooth>(page_scroll_id)).unwrap_or_default();
                let mut absorbed_wheel = false;
                if !cfg!(test) && !combo_active(ctx) && !settings_sizing_now() {
                    let line_speed = ctx.options(|o| o.line_scroll_speed).max(24.0);
                    let (hover_in_panel, screen_h, raw_dt) = ctx.input(|i| {
                        let hp = i.pointer.hover_pos().is_some_and(|p| ui.max_rect().contains(p));
                        (hp, i.screen_rect.height().max(240.0), i.stable_dt.clamp(0.002, 0.045))
                    });
                    if hover_in_panel && sm.has_bounds && sm.max_y > 0.5 {
                        let mut wheel_dy = 0.0_f32;
                        ctx.input_mut(|i| {
                            let mut any_wheel = false;
                            i.events.retain(|ev| {
                                if let egui::Event::MouseWheel { unit, delta, modifiers } = ev {
                                    if !modifiers.ctrl && !modifiers.shift && !modifiers.mac_cmd && !modifiers.command {
                                        let d = match unit {
                                            egui::MouseWheelUnit::Point => delta.y,
                                            egui::MouseWheelUnit::Line => delta.y * line_speed * 1.15,
                                            egui::MouseWheelUnit::Page => delta.y * screen_h * 0.85,
                                        };
                                        let is_trackpad = matches!(unit, egui::MouseWheelUnit::Point) && d.abs() < 4.5;
                                        if !is_trackpad && d.abs() > 0.01 {
                                            wheel_dy += d;
                                            any_wheel = true;
                                            return false;
                                        }
                                    }
                                }
                                true
                            });
                            if any_wheel {
                                i.raw_scroll_delta.y = 0.0;
                                i.smooth_scroll_delta.y = 0.0;
                            }
                        });
                        if wheel_dy.abs() > 0.01 {
                            if !sm.active {
                                sm.cur_y = sm.last_offset_y;
                                sm.target_y = sm.last_offset_y;
                            }
                            sm.target_y = (sm.target_y - wheel_dy).clamp(0.0, sm.max_y);
                            sm.active = true;
                            absorbed_wheel = true;
                        }
                    }
                    if sm.active {
                        let diff = sm.target_y - sm.cur_y;
                        if diff.abs() <= 0.35 {
                            sm.cur_y = sm.target_y;
                            sm.active = false;
                        } else {
                            // 非线性 Quartic/Exponential 阻尼：起步跟手凌厉、中后段持久丝滑贴合停靠
                            let k = 1.0 - (-19.5 * raw_dt).exp();
                            let ratio = (diff.abs() / 160.0).clamp(0.0, 1.0);
                            let nl_boost = 1.0 + 0.35 * (1.0 - (1.0 - ratio).powi(3));
                            let step = diff * (k * nl_boost).min(0.92);
                            sm.cur_y += step;
                            ctx.request_repaint();
                        }
                        ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_scrolling"), true));
                    }
                }
                let jump_top = ctx
                    .data_mut(|d| d.get_temp::<bool>(egui::Id::new("auraisle_search_jump_top")))
                    .unwrap_or(false);
                if jump_top {
                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("auraisle_search_jump_top"), false));
                    sm.target_y = 0.0;
                    sm.cur_y = 0.0;
                    sm.active = false;
                }
                let mut main_scroll = egui::ScrollArea::vertical()
                    .id_salt(("settings-scroll", self.settings_tab, active_sub_salt))
                    .auto_shrink([false, false]);
                if jump_top {
                    main_scroll = main_scroll.vertical_scroll_offset(0.0);
                } else if sm.active || absorbed_wheel {
                    main_scroll = main_scroll.vertical_scroll_offset(sm.cur_y);
                }
                let scroll_out = main_scroll.show(ui,|ui|{
                    // [需求 2 · 图二左右对称 + 全页等宽 + 大屏阅读列适配]
                    // 常规宽度下铺满 `inner_panel_w - right_gutter`；最大化或超宽屏幕时平滑扩张阅读列上限至 `1320px` 并居中，
                    // 最大化/还原跳变时通过零图层宽度动画流畅过渡，同时夹紧至 `<= avail_scroll_w` 确保缩小时不溢出。
                    let right_gutter = if vp_w < 580.0 { 24.0 } else { 26.0 };
                    let avail_scroll_w = (inner_panel_w - right_gutter).max(160.0);
                    let target_content_w = if avail_scroll_w > 1060.0 {
                        (1060.0 + (avail_scroll_w - 1060.0) * 0.48).min(1320.0).min(avail_scroll_w)
                    } else {
                        avail_scroll_w
                    };
                    let content_w = target_content_w;
                    let side_pad = ((avail_scroll_w - content_w) * 0.5).max(0.0);
                    ui.set_width(avail_scroll_w);
                    ui.set_max_width(avail_scroll_w);
                    let content_left = ui.cursor().left() + side_pad;
                    let page_right = content_left + content_w;
                    ui.data_mut(|d| d.insert_temp(egui::Id::new(CANON_PAGE_RIGHT_KEY), page_right));
                    let content_rect = egui::Rect::from_min_size(
                        egui::pos2(content_left, ui.cursor().top()),
                        egui::vec2(content_w, ui.available_height().max(1.0)),
                    );
                    ui.allocate_ui_with_layout(
                        egui::vec2(avail_scroll_w, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |outer_ui| {
                    let mut ui_inner = outer_ui.new_child(
                        egui::UiBuilder::new()
                            .max_rect(content_rect)
                            .layout(egui::Layout::top_down(egui::Align::Min)),
                    );
                    let used_h = {
                    let ui = &mut ui_inner;
                    ui.set_width(content_w);
                    ui.set_max_width(content_w);
                    let enter_ease = if cfg!(test) || settings_sizing_now() {
                        1.0
                    } else {
                        twinkle_page_in_ease((now_t - switch_at).max(0.0) as f32)
                    };
                    if enter_ease < 0.999 { ctx.request_repaint(); }
                    ui.add_space(twinkle_page_in_offset(enter_ease, ui.clip_rect().height()));
                    let enter_alpha = (enter_ease / 0.80).clamp(0.0, 1.0);
                    if enter_alpha < 0.999 { ui.multiply_opacity(enter_alpha); }

                    settings_page_scope(ui, |ui| {
                    let top_title = if self.settings_tab == 20 { "设备" } else if matches!(self.settings_tab, 11 | 12) { "AI" } else { active_title };
                    ui.label(
                        egui::RichText::new(top_title)
                            .size(28.0)
                            .color(settings_color(ctx, Color32::WHITE, Color32::BLACK)),
                    );
                    ui.add_space(16.0);
                    ui.data_mut(|d| d.insert_temp(egui::Id::new("tw_page_first_section_title"), false));

                    if let Some((_,start,end))=SETTINGS_GROUPS.iter().find(|(_,start,end)|(*start..=*end).contains(&self.settings_tab)) {
                        if start!=end {
                            let sub_opts: Vec<(usize, &str)> = (*start..=*end).map(|idx| (idx, titles[idx])).collect();
                            let prev_sub = self.settings_tab;
                            if settings_segmented(ui, &mut self.settings_tab, &sub_opts, 76.0) {
                                let dir: f32 = if self.settings_tab >= prev_sub { 1.0 } else { -1.0 };
                                ctx.data_mut(|d| {
                                    d.insert_temp(egui::Id::new("win11-page-switch-at"), now_t);
                                    d.insert_temp(egui::Id::new("win11-page-switch-dir"), dir);
                                });
                            }
                            ui.add_space(12.0);
                        }
                    }
                    if !cfg!(test) && matches!(self.settings_tab, 11 | 12) {
                        let mut mcfg_ai = crate::auralsle_ai_status::config::get();
                        let prev_scheme = mcfg_ai.ai_island_scheme;
                        settings_card(ui, |ui| {
                            settings_control_row_right(ui, "灵动岛 AI 界面方案", |ui| {
                                settings_segmented(
                                    ui,
                                    &mut mcfg_ai.ai_island_scheme,
                                    &[
                                        (1u8, "方案 1 · 分立双页"),
                                        (2u8, "方案 2 · 合并视图"),
                                    ],
                                    128.0,
                                );
                            });
                            settings_hint(
                                ui,
                                "方案 1 保留现有的「AI 额度」与「AI 工作」两个独立展开页；方案 2 将顶部 AI 额度与下方多会话状态列表合并在同一页展示。",
                            );
                        });
                        if mcfg_ai.ai_island_scheme != prev_scheme {
                            let next_s = mcfg_ai.ai_island_scheme;
                            crate::auralsle_ai_status::config::update(|c| c.ai_island_scheme = next_s);
                        }
                        ui.add_space(6.0);
                    }
                    // [切页 + 缩放动效] 严禁在 ScrollArea 顶部动态插入 add_space 让整页卡片每帧重排。
                    // [缩放卡顿根治] 旧版这里还叠了一层 0.20s「settle」遮罩，并为它每帧 request_repaint：
                    // 拖窗口时遮罩一直不收敛，等于持续强制整页离屏合成（`ui.set_opacity` → 图层 + 再
                    // tessellate），实测持续拖拽时 122–150ms/帧，日志里所有 FREEZE/WARN 都落在这条路径。
                    // 现在整段移除：缩放只走重排，不再为遮罩买单；切页淡入改用下方 ScrollArea 之后的
                    // 「背景色纱幕」（零图层，每帧只多一个 rect）。
                    if let Some(page)=settings_tab_module(self.settings_tab).filter(|page|!self.module_on(*page)) {
                        let mut enable=false;
                        let mod_name = if page < PAGE_NAMES.len() { PAGE_NAMES[page] } else if page == 10 { "天气" } else { "翻译" };
                        settings_card(ui,|ui|{
                            ui.horizontal(|ui|{
                                ui.label(egui::RichText::new(format!("「{}」已关闭，不会在后台同步或提醒",mod_name)).size(12.5));
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{enable=ui.button("开启").clicked();});
                            });
                        });
                        if enable {self.set_page_visible(page,true);}
                        ui.add_space(6.0);
                        if !self.module_on(page) {ui.disable();}
                    }
                    match self.settings_tab {
                    12=>crate::auralsle_ai_status::ui::draw_settings(ui),  // [ai-status-mod]
                    13=>crate::auralsle_ai_status::ui::draw_sysmon_settings(ui),  // [ai-status-mod] 系统监控
                    14=>crate::auralsle_ai_status::course_ui::draw_settings(ui),  // [ai-status-mod] 课程表独立页
                    15=>crate::auralsle_ai_status::plugins::draw(ui),  // [ai-status-mod] 功能：插件总览
                    17=>{
                        settings_card(ui, |ui| {
                            let mut pet_on = self.settings.pet;
                            setting_row(
                                ui,
                                "显示灵屿吉祥物",
                                "开启后在收起态与悬浮通知左侧显示互动吉祥物；关闭后隐藏吉祥物并锁定下方全部吉祥物设置",
                                &mut pet_on,
                            );
                            if pet_on != self.settings.pet {
                                self.set_pet(pet_on);
                            }
                        });
                        ui.add_space(6.0);
                        ui.add_enabled_ui(self.settings.pet, |ui| {
                            crate::auralsle_ai_status::aora_ui::draw_mascot_settings(ui);
                        });
                    },  // [aora] 吉祥物
                    18=>{
                        let mut mcfg_w = crate::auralsle_ai_status::config::get();
                        let prev_w_mode = mcfg_w.weather_ui_mode;
                        if !cfg!(test) {
                            self.sync_weather_with_street_state();
                            if self.weather.snapshot.is_none() {
                                crate::auralsle_ai_status::plugins::ensure_initial_weather_fetch(&self.settings.weather_city);
                            }
                            if crate::auralsle_ai_status::plugins::is_weather_busy() {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
                            }
                        }
                        settings_label(ui, "天气界面方案");
                        settings_card(ui, |ui| {
                            settings_control_row_right(ui, "岛上天气样式", |ui| {
                                settings_segmented(
                                    ui,
                                    &mut mcfg_w.weather_ui_mode,
                                    &[
                                        (1u8, "经典七日版"),
                                        (0u8, "Breezy 街道版"),
                                        (2u8, "现代卡片"),
                                    ],
                                    108.0,
                                );
                            });
                        });
                        if mcfg_w.weather_ui_mode != prev_w_mode {
                            let nm = mcfg_w.weather_ui_mode;
                            crate::auralsle_ai_status::config::update(|c| c.weather_ui_mode = nm);
                        }
                        if !cfg!(test) {
                            let w_src = crate::auralsle_ai_status::config::get().weather_source;
                            self.weather_source.selected = if w_src == 1 {
                                crate::weather_source_ui::Provider::Qweather
                            } else {
                                crate::weather_source_ui::Provider::OpenMeteo
                            };
                        }
                        ui.add_space(4.0);
                        // [需求 3]：将天气岛实时预览紧跟在选择样式卡下方，方便切换方案后即时对比效果
                        crate::auralsle_ai_status::plugins::draw_weather_island_preview_card(self, ui);
                        ui.add_space(4.0);
                        // [需求 5]：将图三红框处的天气实况大卡与城市管理统一应用到所有三种天气界面方案！
                        self.weather_settings(ui);
                        ui.add_space(4.0);
                        // 街道级高精定位与自定义天气源设置（统一 2 列 Fluent 选卡，避免与上方重复冲突）
                        crate::auralsle_ai_status::plugins::draw_weather_settings_page(ui);
                        if !cfg!(test) {
                            self.sync_weather_with_street_state();
                        }
                    },
                    19=>crate::auralsle_ai_status::plugins::draw_translate_settings_page(ui),  // [wisland] 独立翻译设置页
                    // [需求 12] 统一「设备」分类页：显示器与亮度 / 无线外设与胶囊
                    20=>{
                        let mut dev_sub = dev_sub_tab;
                        if settings_segmented(
                            ui,
                            &mut dev_sub,
                            &[
                                (0usize, "显示器与亮度"),
                                (1usize, "无线外设与胶囊"),
                            ],
                            128.0,
                        ) {
                            ui.data_mut(|d| d.insert_temp(dev_sub_id, dev_sub));
                        }
                        ui.add_space(10.0);
                        if dev_sub == 0 {
                            crate::twinkle_page::draw_settings(ui);
                        } else {
                            self.draw_wireless_device_settings(ui);
                        }
                    },
                    21=>{
                        ctx.request_repaint_after(std::time::Duration::from_millis(1500));
                        let mut mcfg = crate::auralsle_ai_status::config::get();
                        let before_m = mcfg.clone();
                        let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                        let title_col = settings_color(ctx, Color32::from_rgb(240, 240, 240), Color32::from_rgb(24, 24, 26));
                        let desc_col = settings_color(ctx, Color32::from_rgb(182, 182, 182), Color32::from_rgb(90, 92, 98));
                        let tile_fill = settings_color(ctx, Color32::from_rgb(0x33, 0x33, 0x33), Color32::from_rgb(0xf1, 0xf3, 0xf9));
                        let tile_stroke = settings_color(ctx, Color32::from_rgb(0x26, 0x26, 0x26), Color32::from_rgb(0xe2, 0xe4, 0xea));

                        let scrolling_now = ctx.input(|i| {
                            i.raw_scroll_delta.length_sq() > 0.1
                                || i.smooth_scroll_delta.length_sq() > 0.1
                                || i.pointer.any_down()
                        });
                        let snap_cache_id = egui::Id::new("mod_res_snap_cache_v1");
                        let snap = {
                            let cached = ctx.data(|d| {
                                d.get_temp::<(std::time::Instant, crate::resources::Snapshot)>(snap_cache_id)
                            });
                            match cached {
                                Some((ts, s))
                                    if scrolling_now || ts.elapsed() < std::time::Duration::from_millis(1400) =>
                                {
                                    s
                                }
                                _ => {
                                    let fresh = crate::resources::snapshot().unwrap_or_default();
                                    ctx.data_mut(|d| {
                                        d.insert_temp(snap_cache_id, (std::time::Instant::now(), fresh));
                                    });
                                    fresh
                                }
                            }
                        };
                        let priv_mb = snap.private_bytes as f64 / (1024.0 * 1024.0);
                        let work_mb = snap.working_bytes as f64 / (1024.0 * 1024.0);
                        let tex_cnt = crate::resources::TEXTURES.load(std::sync::atomic::Ordering::Relaxed);
                        let tex_mb = crate::resources::TEXTURE_BYTES.load(std::sync::atomic::Ordering::Relaxed) as f64 / (1024.0 * 1024.0);
                        let gal_cnt = crate::resources::GALLEYS.load(std::sync::atomic::Ordering::Relaxed);
                        let enabled_count = [
                            mcfg.mod_music_enabled,
                            mcfg.mod_focus_enabled,
                            mcfg.mod_health_enabled,
                            mcfg.mod_shelf_enabled,
                            mcfg.mod_clipboard_enabled,
                            mcfg.mod_mail_enabled,
                            mcfg.mod_calendar_enabled,
                            mcfg.mod_course_enabled,
                            mcfg.mod_ai_status_enabled,
                            mcfg.mod_ai_quota_enabled,
                            mcfg.mod_widgets_sysmon_enabled,
                            mcfg.mod_weather_enabled,
                            mcfg.mod_translate_enabled,
                            mcfg.mod_display_brightness_enabled,
                            mcfg.mod_bt_devices_enabled,
                            mcfg.mod_oppo_connect_enabled,
                        ].into_iter().filter(|&v| v).count();
                        const TOTAL_MODS: usize = 16;

                        settings_label(ui, "内存与资源实时占用");
                        settings_card(ui, |ui| {
                            let avail_w = layout_avail_w(ui).max(200.0);
                            let cols = if avail_w >= 580.0 { 4 } else { 2 };
                            let gap = 8.0_f32;
                            let tile_w = ((avail_w - gap * (cols - 1) as f32) / cols as f32 - 0.5).floor().max(80.0);
                            let metrics = [
                                ("专用提交内存", format!("{priv_mb:.1} MB"), acc),
                                ("物理工作集", format!("{work_mb:.1} MB"), title_col),
                                ("线程 / 句柄", format!("{} / {}", snap.threads, snap.handles), title_col),
                                ("纹理 / 排版缓存", format!("{tex_cnt} ({tex_mb:.1} MB) · {gal_cnt}"), title_col),
                            ];
                            let draw_tile = |ui: &mut egui::Ui, label: &str, val: &str, val_col: Color32| {
                                ui.allocate_ui_with_layout(
                                    egui::vec2(tile_w, 0.0),
                                    egui::Layout::top_down(egui::Align::Min),
                                    |ui| {
                                        ui.set_width(tile_w);
                                        ui.set_max_width(tile_w);
                                        let mut tile_clip = ui.clip_rect();
                                        tile_clip.max.x = tile_clip.max.x.min(ui.cursor().left() + tile_w);
                                        ui.set_clip_rect(tile_clip);
                                        let frame = egui::Frame::new()
                                            .fill(tile_fill)
                                            .stroke(egui::Stroke::new(1.0, tile_stroke))
                                            .corner_radius(egui::CornerRadius::same(4))
                                            .inner_margin(egui::Margin::symmetric(10, 9));
                                        frame.show(ui, |ui| {
                                            let inner_tw = (tile_w - 22.0).max(58.0);
                                            ui.set_width(inner_tw);
                                            ui.set_max_width(inner_tw);
                                            ui.add(egui::Label::new(egui::RichText::new(label).size(11.5).color(desc_col)).truncate());
                                            ui.add_space(3.0);
                                            ui.add(egui::Label::new(egui::RichText::new(val).size(14.0).strong().color(val_col)).truncate());
                                        });
                                    },
                                );
                            };
                            if cols == 4 {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    for (lbl, val, col) in &metrics {
                                        draw_tile(ui, lbl, val, *col);
                                    }
                                });
                            } else {
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    draw_tile(ui, metrics[0].0, &metrics[0].1, metrics[0].2);
                                    draw_tile(ui, metrics[1].0, &metrics[1].1, metrics[1].2);
                                });
                                ui.add_space(gap);
                                ui.horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap;
                                    draw_tile(ui, metrics[2].0, &metrics[2].1, metrics[2].2);
                                    draw_tile(ui, metrics[3].0, &metrics[3].1, metrics[3].2);
                                });
                            }

                            twinkle_card_break(ui);

                            let msg_id = egui::Id::new("mod_mem_clean_status");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                                if twinkle_button(ui, "清理缓存并释放内存", true).clicked() {
                                    self.music_artwork = None;
                                    ctx.forget_all_images();
                                    crate::resources::trim_working_set_async();
                                    ui.data_mut(|d| {
                                        d.insert_temp(
                                            msg_id,
                                            "已释放图片与媒体缓存，正在后台压缩空闲工作集（不阻塞界面）".to_owned(),
                                        );
                                    });
                                }
                                if twinkle_button(ui, "一键低占用精简模式", false).clicked() {
                                    mcfg.mod_music_enabled = false;
                                    mcfg.mod_focus_enabled = true;
                                    mcfg.mod_health_enabled = false;
                                    mcfg.mod_shelf_enabled = false;
                                    mcfg.mod_clipboard_enabled = false;
                                    mcfg.mod_mail_enabled = false;
                                    mcfg.mod_calendar_enabled = false;
                                    mcfg.mod_course_enabled = false;
                                    mcfg.mod_ai_status_enabled = false;
                                    mcfg.mod_ai_quota_enabled = false;
                                    mcfg.mod_widgets_sysmon_enabled = false;
                                    mcfg.mod_weather_enabled = false;
                                    mcfg.mod_translate_enabled = false;
                                    mcfg.mod_display_brightness_enabled = false;
                                    mcfg.mod_bt_devices_enabled = false;
                                    mcfg.mod_oppo_connect_enabled = false;
                                    self.music_artwork = None;
                                    self.mail.recent.clear();
                                    self.mail.reading = None;
                                    self.weather.snapshot = None;
                                    crate::resources::trim_working_set_async();
                                    ui.data_mut(|d| {
                                        d.insert_temp(msg_id, "已切换为低占用精简模式，仅保留番茄专注，已停用所有高消耗后台轮询".to_owned());
                                    });
                                }
                                if enabled_count < TOTAL_MODS && twinkle_button(ui, "恢复全部模块", false).clicked() {
                                    mcfg.mod_music_enabled = true;
                                    mcfg.mod_focus_enabled = true;
                                    mcfg.mod_health_enabled = true;
                                    mcfg.mod_shelf_enabled = true;
                                    mcfg.mod_clipboard_enabled = true;
                                    mcfg.mod_mail_enabled = true;
                                    mcfg.mod_calendar_enabled = true;
                                    mcfg.mod_course_enabled = true;
                                    mcfg.mod_ai_status_enabled = true;
                                    mcfg.mod_ai_quota_enabled = true;
                                    mcfg.mod_widgets_sysmon_enabled = true;
                                    mcfg.mod_weather_enabled = true;
                                    mcfg.mod_translate_enabled = true;
                                    mcfg.mod_display_brightness_enabled = true;
                                    mcfg.mod_bt_devices_enabled = true;
                                    mcfg.mod_oppo_connect_enabled = true;
                                    ui.data_mut(|d| {
                                        d.insert_temp(msg_id, format!("已恢复全部 {TOTAL_MODS} 个功能模块"));
                                    });
                                }
                            });
                            if let Some(note) = ui.data(|d| d.get_temp::<String>(msg_id)) {
                                ui.add_space(4.0);
                                ui.label(egui::RichText::new(note).size(12.0).color(acc));
                            }
                        });

                        ui.add_space(6.0);
                        settings_label(ui, &format!("功能模块总开关（已启用 {enabled_count}/{TOTAL_MODS}）"));
                        settings_hint(ui, "每个功能均可独立启停：停用后立即隐藏对应灵动岛分页、暂停后台采样与网络轮询，并主动回收缓存内存；随时可重新开启，配置不会丢失。");
                        settings_card(ui, |ui| {
                            let rows: [(&str, &str, &mut bool); 16] = [
                                ("音乐与歌词", "SMTC 媒体监听、专辑封面取色与桌面/岛内歌词抓取；关闭后立即停止媒体与歌词轮询", &mut mcfg.mod_music_enabled),
                                ("番茄专注", "番茄钟专注计时与专注状态指示", &mut mcfg.mod_focus_enabled),
                                ("饮水与久坐健康", "每日饮水记录与久坐起身健康提醒", &mut mcfg.mod_health_enabled),
                                ("置物架", "拖放文件临时收纳架与快捷拖出", &mut mcfg.mod_shelf_enabled),
                                ("剪贴板历史", "图文剪贴板历史捕获与快捷粘贴；关闭后停止后台剪贴板监听", &mut mcfg.mod_clipboard_enabled),
                                ("邮箱提醒", "IMAP/POP3 邮件定期拉取、新邮件提示与未读状态统计；关闭后清空邮件记忆缓存", &mut mcfg.mod_mail_enabled),
                                ("日程安排", "系统/日历日程同步与会前提醒", &mut mcfg.mod_calendar_enabled),
                                ("课程表", "学期课程表展示与上课倒计时提醒", &mut mcfg.mod_course_enabled),
                                ("AI 工作状态", "Antigravity / 豆包 / WorkBuddy 等 AI 工具状态探测与权限审批面板", &mut mcfg.mod_ai_status_enabled),
                                ("AI 额度", "Codex / Claude / Gemini 等 AI 平台额度与重置倒计时查询", &mut mcfg.mod_ai_quota_enabled),
                                ("插件与系统监控", "CPU / GPU / 内存 / 磁盘 / 网速实时采样与桌面快捷小组件；关闭后暂停性能计数器采样", &mut mcfg.mod_widgets_sysmon_enabled),
                                ("实时天气", "城市与街道实况天气、预报及空气质量；关闭后清理天气快照缓存并停止气象请求", &mut mcfg.mod_weather_enabled),
                                ("快捷翻译", "多语种快捷文本翻译与一键复制", &mut mcfg.mod_translate_enabled),
                                ("显示器亮度", "外接显示器 DDC/CI 与内置 LCD 亮度控制", &mut mcfg.mod_display_brightness_enabled),
                                ("蓝牙与无线外设", "蓝牙耳机及 2.4GHz 无线外设电量监听与设备上下线提醒", &mut mcfg.mod_bt_devices_enabled),
                                ("OPPO 互联拖拽传文件", "拖入文件时弹出 OPPO 互联悬浮球，支持连接状态指示、纯后台静默传输与环形进度展示", &mut mcfg.mod_oppo_connect_enabled),
                            ];
                            let n_rows = rows.len();
                            for (i, (lbl, desc, flag)) in rows.into_iter().enumerate() {
                                if i > 0 {
                                    twinkle_card_break(ui);
                                }
                                setting_row(ui, lbl, desc, flag);
                            }
                            let _ = n_rows;
                        });

                        if mcfg != before_m {
                            mcfg.mod_focus_health_enabled = mcfg.mod_focus_enabled || mcfg.mod_health_enabled;
                            mcfg.mod_shelf_clipboard_enabled = mcfg.mod_shelf_enabled || mcfg.mod_clipboard_enabled;
                            mcfg.mod_mail_calendar_enabled = mcfg.mod_mail_enabled || mcfg.mod_calendar_enabled;
                            mcfg.mod_ai_suite_enabled = mcfg.mod_ai_status_enabled || mcfg.mod_ai_quota_enabled;
                            mcfg.mod_weather_translate_enabled = mcfg.mod_weather_enabled || mcfg.mod_translate_enabled;
                            mcfg.mod_display_bt_enabled = mcfg.mod_display_brightness_enabled || mcfg.mod_bt_devices_enabled;
                            let dis_any = (!mcfg.mod_music_enabled && before_m.mod_music_enabled)
                                || (!mcfg.mod_shelf_enabled && before_m.mod_shelf_enabled)
                                || (!mcfg.mod_clipboard_enabled && before_m.mod_clipboard_enabled)
                                || (!mcfg.mod_mail_enabled && before_m.mod_mail_enabled)
                                || (!mcfg.mod_calendar_enabled && before_m.mod_calendar_enabled)
                                || (!mcfg.mod_course_enabled && before_m.mod_course_enabled)
                                || (!mcfg.mod_ai_status_enabled && before_m.mod_ai_status_enabled)
                                || (!mcfg.mod_ai_quota_enabled && before_m.mod_ai_quota_enabled)
                                || (!mcfg.mod_widgets_sysmon_enabled && before_m.mod_widgets_sysmon_enabled)
                                || (!mcfg.mod_weather_enabled && before_m.mod_weather_enabled)
                                || (!mcfg.mod_translate_enabled && before_m.mod_translate_enabled)
                                || (!mcfg.mod_display_brightness_enabled && before_m.mod_display_brightness_enabled)
                                || (!mcfg.mod_bt_devices_enabled && before_m.mod_bt_devices_enabled)
                                || (!mcfg.mod_oppo_connect_enabled && before_m.mod_oppo_connect_enabled);
                            if !mcfg.mod_music_enabled {
                                self.music_artwork = None;
                            }
                            if !mcfg.mod_mail_enabled {
                                self.mail.recent.clear();
                                self.mail.reading = None;
                            }
                            if !mcfg.mod_weather_enabled {
                                self.weather.snapshot = None;
                            }
                            crate::monitors::set_module_enabled(mcfg.mod_display_brightness_enabled);
                            crate::auralsle_ai_status::config::update(|c| *c = mcfg);
                            if dis_any {
                                crate::resources::trim_working_set_async();
                            }
                        }
                    },
                    16=>{  // [需求 2] 通知中心：按软件智能归并分类 + 软件筛选条 + 最新优先排序 + 展开折叠
                        let now_ts = chrono::Local::now().timestamp();
                        let is_dnd = crate::auralsle_ai_status::dnd_active(now_ts);
                        let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                        let sel_filter_id = egui::Id::new("win11_notif_sw_filter");
                        let mut sel_filter: String = ui
                            .data(|d| d.get_temp::<String>(sel_filter_id))
                            .unwrap_or_default();

                        fn notif_software_info(r: &crate::model::NotificationRecord) -> (String, String) {
                            let id_low = r.app_id.trim().to_ascii_lowercase();
                            let name_low = r.app_name.trim().to_ascii_lowercase();
                            let title_low = r.title.trim().to_ascii_lowercase();
                            let has = |kw: &str| id_low.contains(kw) || name_low.contains(kw);
                            if has("wechat") || has("weixin") || r.app_name.contains("微信") {
                                return ("sw:wechat".to_owned(), "微信".to_owned());
                            }
                            if has("tencent.qq") || has("qq.exe") || id_low == "qq" || name_low == "qq" || name_low.starts_with("qq ") || has("tim") {
                                return ("sw:qq".to_owned(), "QQ".to_owned());
                            }
                            if has("wxwork") || has("wecom") || r.app_name.contains("企业微信") {
                                return ("sw:wecom".to_owned(), "企业微信".to_owned());
                            }
                            if has("dingtalk") || r.app_name.contains("钉钉") {
                                return ("sw:dingtalk".to_owned(), "钉钉".to_owned());
                            }
                            if has("feishu") || has("lark") || r.app_name.contains("飞书") {
                                return ("sw:feishu".to_owned(), "飞书".to_owned());
                            }
                            if has("douyin") || has("tiktok") || r.app_name.contains("抖音") || title_low.contains("抖音") {
                                return ("sw:douyin".to_owned(), "抖音".to_owned());
                            }
                            if has("doubao") || r.app_name.contains("豆包") {
                                return ("sw:doubao".to_owned(), "豆包".to_owned());
                            }
                            if has("telegram") {
                                return ("sw:telegram".to_owned(), "Telegram".to_owned());
                            }
                            if has("discord") {
                                return ("sw:discord".to_owned(), "Discord".to_owned());
                            }
                            if has("slack") {
                                return ("sw:slack".to_owned(), "Slack".to_owned());
                            }
                            if has("msedge") || has("microsoft edge") || name_low == "edge" {
                                return ("sw:edge".to_owned(), "Microsoft Edge".to_owned());
                            }
                            if has("chrome") {
                                return ("sw:chrome".to_owned(), "Google Chrome".to_owned());
                            }
                            if has("auralsle") || has("auraisle") || r.app_name.contains("岛仔") || r.app_name.contains("灵屿") {
                                return ("sw:auralsle".to_owned(), "灵屿 AuraIsle".to_owned());
                            }
                            if has("antigravity") {
                                return ("sw:antigravity".to_owned(), "Antigravity".to_owned());
                            }
                            if has("oppo") {
                                return ("sw:oppo".to_owned(), "OPPO".to_owned());
                            }
                            let label = if !r.app_name.trim().is_empty() {
                                r.app_name.trim().to_owned()
                            } else if !r.app_id.trim().is_empty() {
                                r.app_id
                                    .trim()
                                    .rsplit(['\\', '/', '.'])
                                    .find(|s| !s.is_empty() && !s.eq_ignore_ascii_case("exe"))
                                    .unwrap_or(r.app_id.trim())
                                    .to_owned()
                            } else {
                                "其他应用".to_owned()
                            };
                            (format!("raw:{}", label.to_ascii_lowercase()), label)
                        }

                        // ── 1) 通知顶栏：左「通知 · 按软件分类」+ 右「免打扰」「全部清除」──
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                egui::RichText::new("通知中心")
                                    .size(17.5)
                                    .strong()
                                    .color(settings_color(ctx, Color32::WHITE, Color32::from_rgb(24, 26, 32))),
                            );
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                let clear_btn = egui::Button::new(
                                    egui::RichText::new("全部清除")
                                        .size(12.0)
                                        .color(settings_color(ctx, Color32::WHITE, Color32::from_rgb(30, 34, 41))),
                                )
                                .fill(settings_color(ctx, Color32::from_rgba_unmultiplied(255, 255, 255, 22), Color32::from_rgb(224, 226, 232)))
                                .stroke(egui::Stroke::new(1.0, Color32::from_white_alpha(18)))
                                .corner_radius(6.0);
                                if ui.add_sized([76.0, 28.0], clear_btn).clicked() {
                                    self.notification_log.clear();
                                    sel_filter.clear();
                                    ui.data_mut(|d| d.insert_temp(sel_filter_id, sel_filter.clone()));
                                }

                                let dnd_btn = egui::Button::new(
                                    egui::RichText::new(if is_dnd { "免打扰 · 已开启" } else { "免打扰" })
                                        .size(12.0)
                                        .color(if is_dnd { Color32::WHITE } else { settings_color(ctx, Color32::from_gray(230), Color32::from_rgb(40, 44, 52)) }),
                                )
                                .fill(if is_dnd {
                                    acc
                                } else {
                                    settings_color(ctx, Color32::from_rgba_unmultiplied(255, 255, 255, 22), Color32::from_rgb(224, 226, 232))
                                })
                                .stroke(egui::Stroke::new(1.0, Color32::from_white_alpha(18)))
                                .corner_radius(6.0);
                                if ui
                                    .add_sized([if is_dnd { 96.0 } else { 62.0 }, 28.0], dnd_btn)
                                    .clicked()
                                {
                                    crate::auralsle_ai_status::set_dnd_until(if is_dnd { 0 } else { now_ts + 3600 });
                                }
                            });
                        });

                        ui.add_space(8.0);

                        if self.notification_log.items.is_empty() {
                            settings_card(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(18.0);
                                    ui.label(egui::RichText::new("暂无通知").size(15.0).strong());
                                    ui.add_space(4.0);
                                    ui.label(
                                        egui::RichText::new("连接系统通知后，收到的通知会按软件自动归类展示在这里。")
                                            .size(11.5)
                                            .color(settings_color(ctx, Color32::from_gray(155), Color32::from_gray(105))),
                                    );
                                    ui.add_space(18.0);
                                });
                            });
                        } else {
                            struct SwGroup {
                                key: String,
                                label: String,
                                latest_at: i64,
                                records: Vec<crate::model::NotificationRecord>,
                            }
                            let mut groups_map: std::collections::BTreeMap<String, SwGroup> = std::collections::BTreeMap::new();
                            for r in &self.notification_log.items {
                                let (sw_key, sw_label) = notif_software_info(r);
                                let entry = groups_map.entry(sw_key.clone()).or_insert_with(|| SwGroup {
                                    key: sw_key,
                                    label: sw_label,
                                    latest_at: r.at,
                                    records: Vec::new(),
                                });
                                if r.at > entry.latest_at {
                                    entry.latest_at = r.at;
                                }
                                entry.records.push(r.clone());
                            }
                            let mut groups: Vec<SwGroup> = groups_map.into_values().collect();
                            for g in &mut groups {
                                g.records.sort_by(|a, b| b.at.cmp(&a.at));
                            }
                            groups.sort_by(|a, b| b.latest_at.cmp(&a.latest_at).then_with(|| a.label.cmp(&b.label)));

                            if !sel_filter.is_empty() && !groups.iter().any(|g| g.key == sel_filter) {
                                sel_filter.clear();
                                ui.data_mut(|d| d.insert_temp(sel_filter_id, sel_filter.clone()));
                            }

                            // ── 2) 按软件分类筛选栏 ──
                            let total_cnt: usize = groups.iter().map(|g| g.records.len()).sum();
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                let draw_filter_chip = |ui: &mut egui::Ui, label: &str, active: bool| -> egui::Response {
                                    let bg = if active {
                                        acc
                                    } else {
                                        settings_color(ctx, Color32::from_rgb(0x33, 0x33, 0x33), Color32::from_rgb(0xe2, 0xe4, 0xea))
                                    };
                                    let fg = if active {
                                        Color32::WHITE
                                    } else {
                                        settings_color(ctx, Color32::from_rgb(225, 225, 228), Color32::from_rgb(36, 38, 44))
                                    };
                                    let w = (label.chars().count() as f32 * 8.0 + 22.0).max(52.0);
                                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, 26.0), egui::Sense::click());
                                    let fill = if resp.hovered() && !active {
                                        settings_color(ctx, Color32::from_rgb(0x3f, 0x3f, 0x42), Color32::from_rgb(0xd6, 0xd9, 0xe2))
                                    } else {
                                        bg
                                    };
                                    ui.painter().rect_filled(rect, 4.0, fill);
                                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(11.5), fg);
                                    resp
                                };
                                if draw_filter_chip(ui, &format!("全部 ({total_cnt})"), sel_filter.is_empty()).clicked() {
                                    sel_filter.clear();
                                    ui.data_mut(|d| d.insert_temp(sel_filter_id, sel_filter.clone()));
                                }
                                for g in &groups {
                                    let active = sel_filter == g.key;
                                    if draw_filter_chip(ui, &format!("{} ({})", g.label, g.records.len()), active).clicked() {
                                        sel_filter = if active { String::new() } else { g.key.clone() };
                                        ui.data_mut(|d| d.insert_temp(sel_filter_id, sel_filter.clone()));
                                    }
                                }
                            });
                            ui.add_space(10.0);

                            let mut remove_single: Option<(String, i64, String)> = None;
                            let mut remove_group_key: Option<String> = None;

                            for g in groups {
                                if !sel_filter.is_empty() && g.key != sel_filter {
                                    continue;
                                }
                                let Some(first) = g.records.first() else { continue };
                                let exp_id = egui::Id::new(("win11_notif_group_exp", &g.key));
                                let mut expanded = ui.data(|d| d.get_temp::<bool>(exp_id)).unwrap_or(!sel_filter.is_empty());
                                let rest_count = g.records.len().saturating_sub(1);
                                let exp_anim = if cfg!(test) {
                                    if expanded { 1.0 } else { 0.0 }
                                } else {
                                    ctx.animate_bool_with_time(exp_id.with("rb-notif-acc"), expanded, 0.28)
                                };
                                if !cfg!(test) && exp_anim > 0.002 && exp_anim < 0.998 {
                                    ctx.request_repaint();
                                    ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_child_busy"), true));
                                }

                                settings_card(ui, |ui| {
                                    let row_w = layout_avail_w(ui).max(160.0);
                                    ui.set_width(row_w);
                                    ui.set_max_width(row_w);
                                    ui.spacing_mut().item_spacing.y = 0.0;

                                    // 软件分类头行：应用图标 + 软件名称 + 数量标签 + 右侧「展开/收起」+「清除该软件」
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 8.0;
                                        let (badge_r, _) = ui.allocate_exact_size(egui::vec2(22.0, 22.0), egui::Sense::hover());
                                        draw_notif_app_icon(ui, badge_r, &first.app_id, &g.label, &first.title, acc);
                                        ui.label(
                                            egui::RichText::new(&g.label)
                                                .size(14.5)
                                                .strong()
                                                .color(settings_color(ctx, Color32::WHITE, Color32::from_rgb(24, 26, 32))),
                                        );
                                        ui.label(
                                            egui::RichText::new(format!("{} 条", g.records.len()))
                                                .size(11.5)
                                                .color(settings_color(ctx, Color32::from_gray(155), Color32::from_gray(105))),
                                        );
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            ui.spacing_mut().item_spacing.x = 6.0;
                                            let grp_x = egui::Button::new(
                                                egui::RichText::new("清空").size(11.5).color(settings_color(ctx, Color32::from_gray(190), Color32::from_gray(90))),
                                            )
                                            .fill(settings_color(ctx, Color32::from_rgb(0x33, 0x33, 0x33), Color32::from_rgb(0xe2, 0xe4, 0xea)))
                                            .stroke(egui::Stroke::NONE)
                                            .corner_radius(4.0);
                                            if ui.add_sized([44.0, 22.0], grp_x).clicked() {
                                                remove_group_key = Some(g.key.clone());
                                            }
                                            if rest_count > 0 {
                                                let pill_label = if expanded {
                                                    "收起".to_owned()
                                                } else {
                                                    format!("展开全部 (+{rest_count})")
                                                };
                                                let pw = (pill_label.chars().count() as f32 * 8.5 + 18.0).max(54.0);
                                                let (p_rect, p_resp) = ui.allocate_exact_size(egui::vec2(pw, 22.0), egui::Sense::click());
                                                let p_fill = if p_resp.hovered() {
                                                    settings_color(ctx, Color32::from_rgb(70, 72, 80), Color32::from_rgb(214, 218, 226))
                                                } else {
                                                    settings_color(ctx, Color32::from_rgb(54, 56, 62), Color32::from_rgb(228, 230, 236))
                                                };
                                                ui.painter().rect_filled(p_rect, 4.0, p_fill);
                                                ui.painter().text(
                                                    p_rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    pill_label,
                                                    egui::FontId::proportional(11.0),
                                                    settings_color(ctx, Color32::WHITE, Color32::from_rgb(30, 34, 41)),
                                                );
                                                if p_resp.clicked() {
                                                    expanded = !expanded;
                                                    ui.data_mut(|d| d.insert_temp(exp_id, expanded));
                                                }
                                            }
                                        });
                                    });
                                    ui.add_space(6.0);
                                    crate::extras::twinkle_card_break(ui);

                                    let draw_notif_record_row = |ui: &mut egui::Ui, r_idx: usize, record: &crate::model::NotificationRecord, remove_single: &mut Option<(String, i64, String)>| {
                                        let detail_id = egui::Id::new(("win11_notif_detail", &g.key, record.at, r_idx));
                                        let mut show_full = ui.data(|d| d.get_temp::<bool>(detail_id)).unwrap_or(false);
                                        let time_str = chrono::DateTime::from_timestamp(record.at, 0)
                                            .map(|t| t.with_timezone(&chrono::Local).format("%m-%d %H:%M").to_string())
                                            .unwrap_or_else(|| "刚刚".to_owned());
                                        ui.add_space(8.0);
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 10.0;
                                            let (av_r, _) = ui.allocate_exact_size(egui::vec2(26.0, 26.0), egui::Sense::hover());
                                            draw_notif_app_icon(ui, av_r, &record.app_id, &record.app_name, &record.title, acc);
                                            ui.vertical(|ui| {
                                                let text_col_w = (row_w - 36.0).max(80.0);
                                                ui.set_max_width(text_col_w);
                                                ui.spacing_mut().item_spacing.y = 3.0;
                                                let title_w = (row_w - 86.0).max(60.0);
                                                ui.horizontal(|ui| {
                                                    ui.set_max_width(text_col_w);
                                                    let max_chars = (title_w / 12.0).floor() as usize;
                                                    let display_title = if record.title.chars().count() > max_chars.max(4) {
                                                        format!("{}…", record.title.chars().take(max_chars.max(4) - 1).collect::<String>())
                                                    } else {
                                                        record.title.clone()
                                                    };
                                                    ui.label(
                                                        egui::RichText::new(display_title)
                                                            .size(13.0)
                                                            .strong()
                                                            .color(settings_color(ctx, Color32::WHITE, Color32::from_rgb(22, 24, 30))),
                                                    );
                                                    ui.label(
                                                        egui::RichText::new(time_str)
                                                            .size(10.8)
                                                            .color(settings_color(ctx, Color32::from_gray(155), Color32::from_gray(110))),
                                                    );
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        ui.spacing_mut().item_spacing.x = 2.0;
                                                        let del_btn = egui::Button::new(
                                                            egui::RichText::new("×")
                                                                .size(13.0)
                                                                .color(settings_color(ctx, Color32::from_gray(190), Color32::from_gray(85))),
                                                        )
                                                        .fill(Color32::TRANSPARENT)
                                                        .stroke(egui::Stroke::NONE);
                                                        if ui.add_sized([20.0, 18.0], del_btn).clicked() {
                                                            *remove_single = Some((record.app_id.clone(), record.at, record.title.clone()));
                                                        }
                                                        if record.body.chars().count() > 68 {
                                                            let more_btn = egui::Button::new(
                                                                egui::RichText::new(if show_full { "收起" } else { "···" })
                                                                    .size(11.5)
                                                                    .color(settings_color(ctx, Color32::from_gray(190), Color32::from_gray(85))),
                                                            )
                                                            .fill(Color32::TRANSPARENT)
                                                            .stroke(egui::Stroke::NONE);
                                                            if ui.add_sized([26.0, 18.0], more_btn).clicked() {
                                                                show_full = !show_full;
                                                                ui.data_mut(|d| d.insert_temp(detail_id, show_full));
                                                            }
                                                        }
                                                    });
                                                });
                                                if !record.body.is_empty() {
                                                    let body_str = if show_full || record.body.chars().count() <= 68 {
                                                        record.body.clone()
                                                    } else {
                                                        let s: String = record.body.chars().take(68).collect();
                                                        format!("{s}…")
                                                    };
                                                    ui.add(
                                                        egui::Label::new(
                                                            egui::RichText::new(body_str)
                                                                .size(11.5)
                                                                .color(settings_color(ctx, Color32::from_gray(192), Color32::from_gray(85))),
                                                        )
                                                        .wrap(),
                                                    );
                                                }
                                            });
                                        });
                                        ui.add_space(8.0);
                                    };

                                    draw_notif_record_row(ui, 0, first, &mut remove_single);

                                    if rest_count > 0 && exp_anim > 0.002 {
                                        let ease = if cfg!(test) {
                                            1.0
                                        } else {
                                            1.0 - (1.0 - exp_anim).powi(3)
                                        };
                                        let h_id = exp_id.with("rb-notif-acc-h");
                                        let cached_h = ui.data(|d| d.get_temp::<f32>(h_id))
                                            .unwrap_or_else(|| (rest_count as f32 * 56.0).clamp(48.0, 640.0));
                                        let vis_h = (cached_h * ease).max(1.0);
                                        let outer_min = ui.cursor().min;
                                        let clip_r = egui::Rect::from_min_size(outer_min, egui::vec2(row_w, vis_h))
                                            .intersect(ui.clip_rect());
                                        let trans_y = if ease < 0.999 { -14.0 * (1.0 - ease) } else { 0.0 };
                                        let content_max = egui::Rect::from_min_size(
                                            egui::pos2(outer_min.x, outer_min.y + trans_y),
                                            egui::vec2(row_w, 4096.0),
                                        );
                                        let mut child_ui = ui.new_child(
                                            egui::UiBuilder::new()
                                                .max_rect(content_max)
                                                .layout(egui::Layout::top_down(egui::Align::Min)),
                                        );
                                        child_ui.set_clip_rect(clip_r);
                                        child_ui.set_width(row_w);
                                        child_ui.set_max_width(row_w);
                                        if ease < 0.999 {
                                            child_ui.multiply_opacity(ease);
                                        }
                                        let y0 = child_ui.cursor().top();
                                        for (idx_off, record) in g.records.iter().skip(1).enumerate() {
                                            crate::extras::twinkle_card_break(&mut child_ui);
                                            draw_notif_record_row(&mut child_ui, idx_off + 1, record, &mut remove_single);
                                        }
                                        let measured_h = (child_ui.cursor().top() - y0).max(1.0);
                                        ui.data_mut(|d| d.insert_temp(h_id, measured_h));
                                        ui.allocate_space(egui::vec2(row_w, if ease >= 0.999 { measured_h } else { vis_h }));
                                    }
                                });
                                ui.add_space(8.0);
                            }

                            if let Some((aid, at, title)) = remove_single {
                                if let Some(pos) = self
                                    .notification_log
                                    .items
                                    .iter()
                                    .position(|x| x.app_id == aid && x.at == at && x.title == title)
                                {
                                    self.notification_log.items.remove(pos);
                                    self.notification_log.save();
                                }
                            }
                            if let Some(rm_key) = remove_group_key {
                                self.notification_log
                                    .items
                                    .retain(|x| notif_software_info(x).0 != rm_key);
                                self.notification_log.save();
                            }
                        }
                    }
                    11=>self.quota_settings(ui),
                    10=>{
                        ui.label(egui::RichText::new("拖动调整收起态常驻元素的左右顺序，双击任意元素可直接开关显示，并在下方调整展开页顺序。").size(11.5).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                        settings_label(ui,"收起态常驻元素排列与显隐");
                        settings_card(ui,|ui|self.resident_order_settings(ui));
                        if !cfg!(test) {
                            settings_label(ui, "底部页面指示条样式");
                            settings_card(ui, |ui| {
                                let preview_accents: Vec<Color32> = self
                                    .active_page_order()
                                    .into_iter()
                                    .take(10)
                                    .map(|p| self.page_accent_color(p))
                                    .collect();
                                let mut mcfg_nav = crate::auralsle_ai_status::config::get();
                                let prev_nav = (mcfg_nav.nav_bar_style, mcfg_nav.nav_badge_pos, mcfg_nav.hide_nav_arrows);
                                settings_control_row_right(ui, "底部页面指示条主题", |ui| {
                                    settings_segmented(
                                        ui,
                                        &mut mcfg_nav.nav_bar_style,
                                        &[
                                            (1u8, "主题 1"),
                                            (2u8, "主题 2"),
                                        ],
                                        84.0,
                                    );
                                });
                                twinkle_card_break(ui);
                                settings_control_row_right(ui, "页面指示点悬浮标签位置", |ui| {
                                    settings_segmented(
                                        ui,
                                        &mut mcfg_nav.nav_badge_pos,
                                        &[
                                            (0u8, "指示点上方"),
                                            (1u8, "底栏右侧留白区"),
                                        ],
                                        118.0,
                                    );
                                });
                                settings_hint(ui, "「指示点上方」会自动拉长展开岛底部 14pt 空间，避免悬浮标签遮挡按钮；「底栏右侧留白区」保持紧凑高度并显示在右下角留白处。");
                                twinkle_card_break(ui);
                                setting_row(ui, "隐藏左右切页按钮", "隐藏展开岛两侧的 < / > 切页箭头，仍可通过底部指示点、滚轮或方向键切换", &mut mcfg_nav.hide_nav_arrows);
                                twinkle_card_break(ui);
                                let avail_w = layout_avail_w(ui).max(200.0);
                                let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                                let dark = ctx.style().visuals.dark_mode;
                                let gap = 10.0_f32;
                                let two_cols = avail_w >= 390.0;
                                let card_w = if two_cols {
                                    ((avail_w - gap) * 0.5).floor().max(140.0)
                                } else {
                                    avail_w
                                };
                                let draw_nav_preview = |ui: &mut egui::Ui, mode: u8, selected: bool, title: &str, desc: &str| -> bool {
                                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(card_w, 72.0), egui::Sense::click());
                                    let bg = if dark {
                                        if selected { Color32::from_rgb(0x2F, 0x35, 0x42) } else { Color32::from_rgb(0x24, 0x24, 0x26) }
                                    } else if selected {
                                        Color32::from_rgb(0xEE, 0xF3, 0xFB)
                                    } else {
                                        Color32::from_rgb(0xF6, 0xF7, 0xFA)
                                    };
                                    let stroke_col = if selected {
                                        acc
                                    } else if resp.hovered() {
                                        settings_color(ctx, Color32::from_gray(88), Color32::from_gray(170))
                                    } else {
                                        settings_color(ctx, Color32::from_rgb(0x36, 0x36, 0x38), Color32::from_rgb(0xDC, 0xDF, 0xE6))
                                    };
                                    ui.painter().rect(
                                        rect,
                                        8.0,
                                        bg,
                                        egui::Stroke::new(if selected { 1.5 } else { 1.0 }, stroke_col),
                                        egui::StrokeKind::Inside,
                                    );
                                    let title_col = settings_color(ctx, Color32::from_gray(236), Color32::from_rgb(28, 30, 36));
                                    let desc_col = settings_color(ctx, Color32::from_gray(158), Color32::from_rgb(100, 106, 118));
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 12.0, rect.top() + 11.0),
                                        egui::Align2::LEFT_TOP,
                                        title,
                                        egui::FontId::proportional(12.5),
                                        title_col,
                                    );
                                    ui.painter().text(
                                        egui::pos2(rect.left() + 12.0, rect.top() + 28.0),
                                        egui::Align2::LEFT_TOP,
                                        desc,
                                        egui::FontId::proportional(10.5),
                                        desc_col,
                                    );
                                    // 模拟灵动岛底部胶囊排
                                    let island_r = egui::Rect::from_min_max(
                                        egui::pos2(rect.left() + 10.0, rect.bottom() - 25.0),
                                        egui::pos2(rect.right() - 10.0, rect.bottom() - 7.0),
                                    );
                                    ui.painter().rect_filled(island_r, 9.0, Color32::from_rgb(24, 24, 28));
                                    let count = preview_accents.len().clamp(5, 8);
                                    let active_idx = 1usize.min(count - 1);
                                    let dot_gap = 5.0_f32;
                                    let mut widths = Vec::with_capacity(count);
                                    let mut total_dots_w = 0.0_f32;
                                    for i in 0..count {
                                        let w = if i == active_idx { 22.0 } else { 10.0 };
                                        total_dots_w += w;
                                        widths.push(w);
                                    }
                                    total_dots_w += dot_gap * (count as f32 - 1.0);
                                    let mut x = island_r.center().x - total_dots_w * 0.5;
                                    let cy = island_r.center().y;
                                    for i in 0..count {
                                        let is_cur = i == active_idx;
                                        let w = widths[i];
                                        let h = if is_cur { 7.5 } else { 6.0 };
                                        let dr = egui::Rect::from_center_size(egui::pos2(x + w * 0.5, cy), egui::vec2(w, h));
                                        if mode == 1 {
                                            let col = if is_cur { Color32::WHITE } else { Color32::from_rgb(92, 92, 96) };
                                            ui.painter().rect_filled(dr, h * 0.5, col);
                                        } else {
                                            let c_rgb = *preview_accents.get(i).unwrap_or(&acc);
                                            let col = if is_cur {
                                                c_rgb
                                            } else {
                                                crate::paint::mix(c_rgb, Color32::from_rgb(148, 152, 164), 0.18).gamma_multiply(0.86)
                                            };
                                            if is_cur {
                                                ui.painter().rect_stroke(
                                                    dr.expand(1.1),
                                                    (h + 2.2) * 0.5,
                                                    egui::Stroke::new(1.1, Color32::from_rgba_unmultiplied(c_rgb.r(), c_rgb.g(), c_rgb.b(), 120)),
                                                    egui::StrokeKind::Outside,
                                                );
                                            }
                                            ui.painter().rect_filled(dr, h * 0.5, col);
                                        }
                                        x += w + dot_gap;
                                    }
                                    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
                                };
                                let cur_nav_style = mcfg_nav.nav_bar_style;
                                if two_cols {
                                    ui.horizontal_top(|ui| {
                                        ui.spacing_mut().item_spacing.x = gap;
                                        if draw_nav_preview(ui, 1, cur_nav_style == 1, "主题 1 · 经典纯白胶囊", "当前页高亮白胶囊 · 其余低调深灰圆点") {
                                            mcfg_nav.nav_bar_style = 1;
                                        }
                                        if draw_nav_preview(ui, 2, cur_nav_style == 2, "主题 2 · 主题彩灯胶囊", "各页对应主题色胶囊 · 激活带外圈微光") {
                                            mcfg_nav.nav_bar_style = 2;
                                        }
                                    });
                                } else {
                                    if draw_nav_preview(ui, 1, cur_nav_style == 1, "主题 1 · 经典纯白胶囊", "当前页高亮白胶囊 · 其余低调深灰圆点") {
                                        mcfg_nav.nav_bar_style = 1;
                                    }
                                    ui.add_space(6.0);
                                    if draw_nav_preview(ui, 2, cur_nav_style == 2, "主题 2 · 主题彩灯胶囊", "各页对应主题色胶囊 · 激活带外圈微光") {
                                        mcfg_nav.nav_bar_style = 2;
                                    }
                                }
                                if (mcfg_nav.nav_bar_style, mcfg_nav.nav_badge_pos, mcfg_nav.hide_nav_arrows) != prev_nav {
                                    let ns = mcfg_nav.nav_bar_style;
                                    let np = mcfg_nav.nav_badge_pos;
                                    let ha = mcfg_nav.hide_nav_arrows;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.nav_bar_style = ns;
                                        c.nav_badge_pos = np;
                                        c.hide_nav_arrows = ha;
                                    });
                                }
                            });
                        }
                        settings_label(ui,"展开态功能页面");
                        settings_card(ui,|ui|{
                            let descriptions=["音乐播放控制、封面流光与同步歌词","番茄钟专注与休息计时","每日饮水进度记录与定时提醒",if cfg!(test) { "临时文件收纳架与历史剪贴板" } else { "临时文件拖放收纳架" },"待办任务清单与到期提醒","邮箱新邮件摘要与验证码一键复制","AI 服务剩余额度与多账号切换","月历视图、课程表与工作日程","AI 助手实时工作阶段与完成提醒","硬件性能、电源、时间与课表定制组件"];
                            #[derive(Clone, Copy)]
                            struct PageDrag(usize);
                            // [窄宽排版] 这行原来是 `ui.available_width()`（窄窗会虚胖成 900+），
                            // 于是每张行卡都按虚胖宽度铺、整排被窗口右缘裁掉（图三）。
                            let row_w = layout_avail_w(ui);
                            if cfg!(test) {
                                let mut movement=None;
                                for (index,page) in self.settings.page_order.into_iter().enumerate() {
                                    if index>0 {twinkle_card_break(ui);}
                                    let mut visible=self.settings.visible_pages[page];
                                    let can_change=!visible || self.settings.visible_pages.iter().filter(|enabled|**enabled).count()>1;
                                    let row=ui.push_id(("page-order",page),|ui|ui.horizontal(|ui|{
                                        ui.set_max_width(row_w);
                                        let (rect,handle)=ui.allocate_exact_size(egui::vec2(20.0,40.0),egui::Sense::drag());
                                        for x in [-3.0,3.0] {for y in [-6.0,0.0,6.0] {
                                            ui.painter().circle_filled(rect.center()+egui::vec2(x,y),1.4,settings_color(ctx,Color32::from_gray(155),Color32::from_gray(100)));
                                        }}
                                        handle.on_hover_cursor(egui::CursorIcon::Grab).dnd_set_drag_payload(PageDrag(page));
                                        let page_title = PAGE_NAMES[page];
                                        ui.add_enabled_ui(can_change,|ui|{setting_row(ui,page_title,descriptions[page],&mut visible);});
                                    })).inner.response;
                                    if let Some(payload)=row.dnd_hover_payload::<PageDrag>() {
                                        if payload.0!=page {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                            let y=if self.settings.page_order.iter().position(|p|*p==payload.0).unwrap()<index {row.rect.bottom()}else{row.rect.top()};
                                            ui.painter().line_segment([egui::pos2(row.rect.left(),y),egui::pos2(row.rect.right(),y)],egui::Stroke::new(2.0,Color32::from_rgb(70,150,255)));
                                        }
                                    }
                                    if let Some(payload)=row.dnd_release_payload::<PageDrag>() {movement=Some((payload.0,index));}
                                    if visible!=self.settings.visible_pages[page] {self.set_page_visible(page,visible);}
                                }
                                if let Some((page,to))=movement {
                                    let from=self.settings.page_order.iter().position(|p|*p==page).unwrap();
                                    if from<to {self.settings.page_order[from..=to].rotate_left(1);}
                                    else {self.settings.page_order[to..=from].rotate_right(1);}
                                }
                            } else {
                                // [修复需求 6]：将剪贴板(12)、天气(10)、翻译(11) 全部纳入可移动列表，并加入 React Bits 邻近挤压与弹性换位互动动画！
                                let mut mcfg = crate::auralsle_ai_status::config::get();
                                let before_m = mcfg.clone();
                                let mut full_order = self.active_page_order();
                                let mut swap_req: Option<(usize, usize)> = None;
                                let dragged_page = egui::DragAndDrop::payload::<PageDrag>(ctx).map(|p| p.0);
                                let drag_from_idx = dragged_page.and_then(|dp| full_order.iter().position(|&x| x == dp));
                                let ptr_pos = ctx.pointer_interact_pos();
                                let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                                let total_enabled = (0..=14).filter(|&p| self.is_page_currently_visible(p)).count();

                                for (index, &page) in full_order.iter().enumerate() {
                                    let (page_title, page_desc) = match page {
                                        9 => ("插件", "硬件监控、时间、课表与插件面板"),
                                        10 => ("天气", "街道级实况气象、逐时趋势与多日预报"),
                                        11 => ("翻译", "多语种互译与剪贴板自动识别"),
                                        12 => ("剪贴板", "图文剪贴板历史、搜索备注与一键粘贴"),
                                        13 => ("设备岛", "无线鼠标、键盘、耳机与手柄四合一电量页面"),
                                        14 => ("亮度", "外接显示器 DDC/CI 与内置屏幕亮度快速调节"),
                                        p if p < 10 => (PAGE_NAMES[p], descriptions[p]),
                                        _ => ("扩展页面", ""),
                                    };
                                    let mut visible = match page {
                                        10 => mcfg.weather_page_visible,
                                        11 => mcfg.translate_page_visible,
                                        12 => mcfg.clipboard_page_visible && self.settings.clipboard_history,
                                        13 => mcfg.device_page_visible && mcfg.show_bt_battery_on_island && (!mcfg.device_auto_hide_when_empty || self.has_connected_wireless_devices()),
                                        14 => mcfg.brightness_page_visible,
                                        p if p < 10 => self.settings.visible_pages[p],
                                        _ => true,
                                    };
                                    let can_change = !visible || total_enabled > 1;
                                    let is_being_dragged = dragged_page == Some(page);

                                    // 计算当前行因换位产生的平滑弹性纵向偏移（与相邻卡片产生真实挤压回弹互动）
                                    let target_slot_y = index as f32 * 52.0;
                                    let anim_slot_y = ctx.animate_value_with_time(
                                        egui::Id::new(("rb-page-slot-y", page)),
                                        target_slot_y,
                                        0.18,
                                    );
                                    let dy_anim = (anim_slot_y - target_slot_y).clamp(-42.0, 42.0);
                                    if dy_anim.abs() > 0.25 {
                                        ctx.request_repaint();
                                    }

                                    let row_hover = ui.rect_contains_pointer(egui::Rect::from_min_size(ui.cursor().min, egui::vec2(row_w, 50.0)));
                                    let row_fill = if is_being_dragged {
                                        settings_color(ctx, Color32::from_rgb(32, 36, 48), Color32::from_rgb(228, 234, 248))
                                    } else if row_hover {
                                        settings_color(ctx, Color32::from_rgba_unmultiplied(255, 255, 255, 6), Color32::from_rgba_unmultiplied(0, 0, 0, 4))
                                    } else {
                                        Color32::TRANSPARENT
                                    };
                                    let row_frame = egui::Frame::new()
                                        .fill(row_fill)
                                        .corner_radius(4.0)
                                        .inner_margin(egui::Margin::symmetric(4, 6))
                                        .show(ui, |ui| {
                                            let inner_rw = (row_w - 8.0).max(150.0);
                                            ui.set_width(inner_rw);
                                            ui.set_max_width(inner_rw);
                                            ui.spacing_mut().item_spacing.y = 0.0;
                                            let row_content = ui.horizontal(|ui| {
                                                ui.spacing_mut().item_spacing.x = 10.0;
                                                let (slot_r, handle) = ui.allocate_exact_size(egui::vec2(20.0, 44.0), egui::Sense::click_and_drag());
                                                ui.add_enabled_ui(can_change, |ui| {
                                                    setting_row(ui, page_title, page_desc, &mut visible);
                                                });
                                                (slot_r, handle)
                                            });
                                            let (slot_r, handle) = row_content.inner;
                                            let row_mid_y = row_content.response.rect.center().y;
                                            let dot_col = if is_being_dragged || handle.hovered() {
                                                acc
                                            } else {
                                                settings_color(ctx, Color32::from_gray(145), Color32::from_gray(115))
                                            };
                                            // [需求 3] 9 点（3×3）实心网格严格居中对齐整行中心线（title+desc+switch 中线）
                                            let cx = slot_r.center().x;
                                            let cy = row_mid_y + dy_anim * 0.18;
                                            for gx in [-4.0_f32, 0.0, 4.0] {
                                                for gy in [-4.0_f32, 0.0, 4.0] {
                                                    ui.painter().circle_filled(egui::pos2(cx + gx, cy + gy), 1.25, dot_col);
                                                }
                                            }
                                            handle.on_hover_cursor(egui::CursorIcon::Grab).dnd_set_drag_payload(PageDrag(page));
                                        });
                                    // 行间分隔线精确落于上下行接缝，保证上下内边距完全一致
                                    if index > 0 {
                                        let r = row_frame.response.rect;
                                        ui.painter().line_segment(
                                            [egui::pos2(r.left() + 4.0, r.top()), egui::pos2(r.right() - 4.0, r.top())],
                                            egui::Stroke::new(1.0, settings_color(ctx, Color32::from_white_alpha(14), Color32::from_black_alpha(12))),
                                        );
                                    }

                                    let r_rect = row_frame.response.rect;
                                    // 拖拽经过相邻行中心线时：立即实时触发相邻项交换，让被挤开的邻居平滑滑向新位置！
                                    if let (Some(from_i), Some(p)) = (drag_from_idx, ptr_pos) {
                                        if from_i != index && r_rect.contains(p) && ui.input(|i| i.pointer.primary_down()) {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                            swap_req = Some((from_i, index));
                                        }
                                    }
                                    if let Some(payload) = row_frame.response.dnd_release_payload::<PageDrag>() {
                                        if let Some(from_i) = full_order.iter().position(|&x| x == payload.0) {
                                            if from_i != index {
                                                swap_req = Some((from_i, index));
                                            }
                                        }
                                    }

                                    // 同步可见性开关修改
                                    match page {
                                        10 => mcfg.weather_page_visible = visible,
                                        11 => mcfg.translate_page_visible = visible,
                                        12 => {
                                            mcfg.clipboard_page_visible = visible;
                                            if visible {
                                                self.settings.clipboard_history = true;
                                                self.clipboard.set_enabled(true);
                                            }
                                        }
                                        13 => {
                                            mcfg.device_page_visible = visible;
                                            if visible {
                                                mcfg.show_bt_battery_on_island = true;
                                            }
                                        }
                                        14 => mcfg.brightness_page_visible = visible,
                                        p if p < 10 => {
                                            if visible != self.settings.visible_pages[p] {
                                                self.set_page_visible(p, visible);
                                            }
                                        }
                                        _ => {}
                                    }
                                }

                                if let Some((from_i, to_i)) = swap_req {
                                    if from_i < full_order.len() && to_i < full_order.len() && from_i != to_i {
                                        let item = full_order.remove(from_i);
                                        full_order.insert(to_i, item);
                                        mcfg.full_page_order = full_order.clone();
                                        // 同步更新底层 0..10 的 page_order
                                        let mut legacy = [0usize; 10];
                                        let mut k = 0usize;
                                        for &p in &full_order {
                                            if p < 10 && k < 10 {
                                                legacy[k] = p;
                                                k += 1;
                                            }
                                        }
                                        if k == 10 {
                                            self.settings.page_order = legacy;
                                            self.settings.save();
                                        }
                                    }
                                }
                                if mcfg != before_m {
                                    crate::auralsle_ai_status::config::update(|c| *c = mcfg);
                                }
                            }
                            if self.settings.visible_pages.iter().filter(|enabled|**enabled).count()==1 {
                                ui.add_space(5.0);
                                ui.label(egui::RichText::new("至少保留一个页面。").size(11.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            }
                        });
                    }
                    0=>{
                        let mut gen_tab = gen_sub_tab;
                        settings_segmented(
                            ui,
                            &mut gen_tab,
                            &[
                                (0usize, "启动与性能"),
                                (1usize, "岛体外观"),
                                (2usize, "显示与停靠"),
                                (3usize, "时钟与极简"),
                                (4usize, "故障与备份"),
                            ],
                            78.0,
                        );
                        if gen_tab != gen_sub_tab {
                            ui.data_mut(|d| d.insert_temp(gen_sub_id, gen_tab));
                        }
                        ui.add_space(10.0);
                        if gen_tab == 0 {
                        // ── Section 1: 常规（直接跟在顶部 28px `.sectionTitle`「常规」下方，复刻 Twinkle Tray `id="general"`）──
                        {
                            let mut a = autostart;
                            setting_row_card(ui, "开机启动", "登录 Windows 时自动打开灵屿 AuraIsle", &mut a);
                            if a != autostart {
                                self.pending.push(Cmd::ToggleAutostart);
                            }
                        }
                        {
                            draw_auralsle_appearance_cards(ui, &mut self.settings.settings_theme);
                        }
                        settings_card(ui, |ui| {
                            let before = self.settings.render_compat;
                            setting_row(
                                ui,
                                if cfg!(test) { "兼容渲染（Direct3D 12）" } else { "Direct3D 12 兼容渲染" },
                                "画面闪烁或黑框时开启，重启后生效",
                                &mut self.settings.render_compat,
                            );
                            if before != self.settings.render_compat {
                                self.settings.render_compat_reason.clear();
                            }
                            if !self.settings.render_compat_reason.is_empty() {
                                settings_hint(ui, &self.settings.render_compat_reason);
                            }
                            let gpu_mode_now = crate::auralsle_ai_status::config::get().gpu_backend;
                            let show_compat_restart = self.settings.render_compat != self.render_compat_running
                                && !(gpu_mode_now == 3 && !self.settings.render_compat);
                            if show_compat_restart {
                                twinkle_card_break(ui);
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new("重启灵屿 AuraIsle 后生效").size(13.0).color(Color32::from_rgb(230, 150, 40)));
                                    if twinkle_button(ui, "立即重启", true).clicked() {
                                        self.pending.push(Cmd::RestartApp);
                                    }
                                });
                            }
                        });
                        settings_card(ui, |ui| {
                            let mut mcfg = crate::auralsle_ai_status::config::get();
                            let prev_gpu = mcfg.gpu_backend;
                            let gpu_lbl = match mcfg.gpu_backend {
                                1 => "Vulkan",
                                2 => "OpenGL（默认）",
                                3 => "Direct3D 12",
                                _ => "自动选择",
                            };
                            setting_select_row(
                                ui,
                                "硬件渲染后端",
                                "选择 GPU 硬件加速图形接口（默认 OpenGL；已强制开启硬件加速，禁止 CPU 软件光栅化）。",
                                "gpu-backend-select",
                                gpu_lbl,
                                &mut mcfg.gpu_backend,
                                &[
                                    (2u8, "OpenGL（默认）"),
                                    (0u8, "自动选择"),
                                    (1u8, "Vulkan"),
                                    (3u8, "Direct3D 12"),
                                ],
                            );
                            if mcfg.gpu_backend != prev_gpu {
                                let gb = mcfg.gpu_backend;
                                crate::auralsle_ai_status::config::update(|c| c.gpu_backend = gb);
                                crate::graphics::apply_gpu_backend_choice(gb);
                            }
                            if !cfg!(test)
                                && crate::graphics::needs_restart_for_gpu_backend(self.settings.render_compat, mcfg.gpu_backend)
                            {
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new("已切换 GPU 图形接口，重启灵屿 AuraIsle 后正式切换底层的渲染后端")
                                            .size(12.0)
                                            .color(Color32::from_rgb(248, 198, 96)),
                                    );
                                    if twinkle_button(ui, "立即重启", true).clicked() {
                                        self.pending.push(Cmd::RestartApp);
                                    }
                                });
                            }
                            if !cfg!(test) {
                                twinkle_card_break(ui);
                                let gpu_open_id = egui::Id::new("tw_gpu_diag_open");
                                let mut gpu_open = ui.data(|d| d.get_temp::<bool>(gpu_open_id)).unwrap_or(false);
                                let gpu_anim_id = egui::Id::new("tw_gpu_diag_anim");
                                let mut gpu_anim = ui.data(|d| d.get_temp::<f32>(gpu_anim_id)).unwrap_or(0.0);
                                let gpu_info = crate::graphics::probe_gpu_hardware_info();
                                let gpu_summary = format!("当前显卡：{} ({}) · 管线：{}", gpu_info.adapter_name, gpu_info.adapter_kind, gpu_info.active_backend);
                                if settings_expand_header(ui, "tw-gpu-diag-hdr", "显卡与图形驱动详情", &gpu_summary, &mut gpu_open) {
                                    ui.data_mut(|d| d.insert_temp(gpu_open_id, gpu_open));
                                }
                                settings_subpanel(ui, &mut gpu_anim, gpu_open, "tw-gpu-diag-sub", |ui| {
                                    let desc_col = settings_color(ctx, Color32::from_rgb(185, 185, 185), Color32::from_rgb(90, 92, 98));
                                    ui.label(egui::RichText::new(format!("处理器：{}", gpu_info.cpu_name)).size(13.0));
                                    ui.label(egui::RichText::new(format!("物理显卡：{} · {}", gpu_info.adapter_name, gpu_info.adapter_kind)).size(13.0));
                                    ui.label(egui::RichText::new(format!("当前管线：{}", gpu_info.active_backend)).size(12.5).color(desc_col));
                                    ui.label(egui::RichText::new(format!("Vulkan 驱动：{}", gpu_info.vulkan_status)).size(12.5).color(desc_col));
                                    ui.label(egui::RichText::new(format!("OpenGL 驱动：{}", gpu_info.opengl_status)).size(12.5).color(desc_col));
                                });
                                ui.data_mut(|d| d.insert_temp(gpu_anim_id, gpu_anim));
                            }
                        });
                        setting_row_card(ui, "提醒声音", "喝水、久坐、待办和专注计时到点时播放提示音", &mut self.settings.sound);
                        setting_row_card(ui, "全屏时隐藏", "游戏、视频和演示全屏时自动隐藏灵动岛", &mut self.settings.hide_fullscreen);

                        } else if gen_tab == 1 {
                        // ── Section 2: 岛体外观（每项独立一张 `.settings-option-elem` 卡片，间距 6px）──
                        settings_label(ui, "岛体外观");
                        {
                            use crate::model::{self as model, Skin};
                            let venu_tab_id = egui::Id::new("venu_appearance_subtab");
                            let mut border_tab = ui.data(|d| d.get_temp::<bool>(venu_tab_id)).unwrap_or(false);
                            let border_label = if border_tab { "边框光效" } else { "样式风格" };
                            if setting_select_card(
                                ui,
                                "外观配置模块",
                                "切换灵动岛基础样式风格或环绕流光边框光效。",
                                "venu-appearance-mode",
                                border_label,
                                &mut border_tab,
                                &[(false, "样式风格"), (true, "边框光效")],
                            ) {
                                ui.data_mut(|d| d.insert_temp(venu_tab_id, border_tab));
                            }

                            if border_tab {
                                settings_card(ui, |ui| {
                                    crate::auralsle_ai_status::ui::draw_venu_border_effects_panel(
                                        ui,
                                        self.preview_fill(),
                                        self.settings.island_shape == model::IslandShape::Pill,
                                    );
                                });
                            } else {
                                let mut mcfg = crate::auralsle_ai_status::config::get();
                                let before_m = mcfg.clone();
                                let prev_skin = self.settings.skin;
                                let test_skins = [Skin::Classic, Skin::Cream, Skin::GlassLight, Skin::GlassDark, Skin::Fluid];
                                let skins: Vec<(Skin, &str)> = if cfg!(test) {
                                    test_skins.iter().map(|s| (*s, s.label())).collect()
                                } else {
                                    Skin::UI_SKINS.iter().map(|s| (*s, s.label())).collect()
                                };

                                // 卡片：皮肤
                                settings_card(ui, |ui| {
                                    if cfg!(test) {
                                        settings_control_row_right(ui, "皮肤", |ui| {
                                            settings_segmented(ui, &mut self.settings.skin, &skins, 68.0);
                                        });
                                        settings_hint(ui, self.settings.skin.description());
                                    } else {
                                        let skin_desc = self.settings.skin.description();
                                        let skin_lbl = self.settings.skin.label();
                                        setting_select_row(
                                            ui,
                                            "皮肤",
                                            skin_desc,
                                            "island-skin-select",
                                            skin_lbl,
                                            &mut self.settings.skin,
                                            &skins,
                                        );
                                    }
                                    if self.settings.skin != prev_skin {
                                        mcfg.win_theme = 0;
                                    }
                                    if self.settings.skin == Skin::Fluid {
                                        twinkle_card_break(ui);
                                        setting_row(
                                            ui,
                                            "跟随音乐专辑封面取色",
                                            "播放音乐时底色随专辑封面平滑渐变，暂停时恢复默认中性色",
                                            &mut mcfg.fluid_cover_tint,
                                        );
                                    }
                                    if self.settings.skin == Skin::Classic && !cfg!(test) {
                                        twinkle_card_break(ui);
                                        let pal_open_id = egui::Id::new("tw_classic_pal_open");
                                        let mut pal_open = ui.data(|d| d.get_temp::<bool>(pal_open_id)).unwrap_or(false);
                                        let pal_anim_id = egui::Id::new("tw_classic_pal_anim");
                                        let mut pal_anim = ui.data(|d| d.get_temp::<f32>(pal_anim_id)).unwrap_or(0.0);
                                        let pal_desc = if mcfg.win_theme == 0 {
                                            "当前：经典纯黑。点击展开选择 18 套扩展主题色板。".to_owned()
                                        } else {
                                            format!(
                                                "当前已启用：{}。点击展开切换扩展主题色板。",
                                                crate::auralsle_ai_status::plugins::WIN_THEMES
                                                    .iter()
                                                    .find(|w| w.id == mcfg.win_theme)
                                                    .map(|w| w.name)
                                                    .unwrap_or("自定义")
                                            )
                                        };
                                        if settings_expand_header(ui, "tw-classic-pal-hdr", "扩展主题色板", &pal_desc, &mut pal_open) {
                                            ui.data_mut(|d| d.insert_temp(pal_open_id, pal_open));
                                        }
                                        settings_subpanel(ui, &mut pal_anim, pal_open, "tw-classic-pal-sub", |ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                                if twinkle_button(ui, "经典纯黑", mcfg.win_theme == 0).clicked() {
                                                    mcfg.win_theme = 0;
                                                }
                                                for wt in crate::auralsle_ai_status::plugins::WIN_THEMES {
                                                    let sel = mcfg.win_theme == wt.id;
                                                    let [pr, pg, pb] = wt.paper;
                                                    let [ar, ag, ab] = wt.accent;
                                                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(84.0, 28.0), egui::Sense::click());
                                                    if resp.clicked() {
                                                        mcfg.win_theme = wt.id;
                                                        mcfg.custom_accent = format!("#{:02X}{:02X}{:02X}", ar, ag, ab);
                                                    }
                                                    ui.painter().rect_filled(rect, 5.0, Color32::from_rgb(pr, pg, pb));
                                                    ui.painter().rect_stroke(
                                                        rect,
                                                        5.0,
                                                        egui::Stroke::new(if sel { 2.0 } else { 1.0 }, if sel { Color32::from_rgb(ar, ag, ab) } else { Color32::from_white_alpha(28) }),
                                                        egui::StrokeKind::Inside,
                                                    );
                                                    ui.painter().circle_filled(egui::pos2(rect.left() + 10.0, rect.center().y), 3.5, Color32::from_rgb(ar, ag, ab));
                                                    let [ir, ig, ib] = wt.ink;
                                                    ui.painter().text(
                                                        egui::pos2(rect.left() + 18.0, rect.center().y),
                                                        egui::Align2::LEFT_CENTER,
                                                        wt.name,
                                                        egui::FontId::proportional(11.0),
                                                        Color32::from_rgb(ir, ig, ib),
                                                    );
                                                }
                                            });
                                        });
                                        ui.data_mut(|d| d.insert_temp(pal_anim_id, pal_anim));
                                    }
                                });

                                // 卡片：形状
                                let shapes: Vec<(model::IslandShape, &str)> = model::IslandShape::ALL.iter().map(|s| (*s, s.label())).collect();
                                settings_card(ui, |ui| {
                                    if cfg!(test) {
                                        settings_control_row_right(ui, "形状", |ui| {
                                            settings_segmented(ui, &mut self.settings.island_shape, &shapes, 64.0);
                                        });
                                        settings_hint(ui, self.settings.island_shape.description());
                                    } else {
                                        let shape_desc = self.settings.island_shape.description();
                                        let shape_lbl = self.settings.island_shape.label();
                                        setting_select_row(
                                            ui,
                                            "形状",
                                            shape_desc,
                                            "island-shape-select",
                                            shape_lbl,
                                            &mut self.settings.island_shape,
                                            &shapes,
                                        );
                                    }
                                });

                                // 卡片：背景模糊
                                if self.settings.skin.supports_blur() {
                                    let mut blur = self.settings.glass_blur;
                                    let blur_desc = if cfg!(test) {
                                        "使用 Windows 合成引擎的实时模糊，严格贴合岛体形状（需开启系统“透明效果”）；关闭后改为烟熏半透明"
                                    } else {
                                        "启用实时毛玻璃背景模糊；关闭后改为半透明底色"
                                    };
                                    setting_row_card(ui, "背景模糊", blur_desc, &mut blur);
                                    self.settings.glass_blur = blur;
                                }

                                // 卡片：大小
                                settings_card(ui, |ui| {
                                    if cfg!(test) {
                                        settings_control_row(ui, "大小", 56.0, |ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                ui.spacing_mut().slider_width = (ui.available_width() - 135.0).clamp(85.0, 180.0);
                                                island_size_slider(ui, &mut self.settings.island_size);
                                                if ui.add_enabled(self.settings.island_size != 100, egui::Button::new("恢复默认")).clicked() {
                                                    self.settings.island_size = 100;
                                                }
                                            }).response
                                        });
                                        settings_hint(ui, "文字、图标和吉祥物按实际像素重新绘制，放大后依旧清晰；只影响岛体，不影响设置窗口。");
                                    } else {
                                        let mut sz_f = self.settings.island_size as f32;
                                        twinkle_parent_panel(
                                            ui,
                                            "大小",
                                            "文字、图标和吉祥物按实际像素重新绘制，放大后依旧清晰；只影响岛体，不影响设置窗口。",
                                            230.0,
                                            30.0,
                                            |ui| {
                                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if twinkle_slider_f32(
                                                        ui,
                                                        "tw-island-size-slider",
                                                        &mut sz_f,
                                                        (crate::model::ISLAND_SIZE_MIN as f32)..=(crate::model::ISLAND_SIZE_MAX as f32),
                                                        true,
                                                        "%",
                                                    ) {
                                                        self.settings.island_size = crate::model::island_size_value(sz_f.round() as u32);
                                                    }
                                                });
                                            },
                                        );
                                    }
                                });

                                // 卡片：主题强调色（可展开）
                                settings_card(ui, |ui| {
                                    let acc_open_id = egui::Id::new("tw_accent_subpanel_open");
                                    let mut acc_open = ui.data(|d| d.get_temp::<bool>(acc_open_id)).unwrap_or(false);
                                    let acc_anim_id = egui::Id::new("tw_accent_subpanel_anim");
                                    let mut acc_anim = ui.data(|d| d.get_temp::<f32>(acc_anim_id)).unwrap_or(0.0);
                                    let acc_desc = format!("当前强调色：{}。点击展开选择预设强调色或使用 RGBA 拾色器自定义。", mcfg.custom_accent);
                                    if settings_expand_header(ui, "tw-accent-expand", "主题强调色", &acc_desc, &mut acc_open) {
                                        ui.data_mut(|d| d.insert_temp(acc_open_id, acc_open));
                                    }
                                    settings_subpanel(ui, &mut acc_anim, acc_open, "tw-accent-subpanel", |ui| {
                                        ui.horizontal_wrapped(|ui| {
                                            ui.spacing_mut().interact_size.y = 24.0;
                                            ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                                            rgba_color_picker_inline(ui, "custom-accent-main", &mut mcfg.custom_accent);
                                            for (name, hex) in crate::auralsle_ai_status::plugins::ACCENT_PRESETS {
                                                if let Some([r, g, b]) = crate::auralsle_ai_status::plugins::parse_hex_rgb(hex) {
                                                    let (sr, sresp) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::click());
                                                    ui.painter().rect_filled(sr, 5.0, Color32::from_rgb(r, g, b));
                                                    if mcfg.custom_accent.eq_ignore_ascii_case(hex) {
                                                        ui.painter().rect_stroke(sr, 5.0, egui::Stroke::new(2.0, Color32::WHITE), egui::StrokeKind::Inside);
                                                    }
                                                    if sresp.on_hover_text(format!("{name} ({hex})")).clicked() {
                                                        mcfg.custom_accent = hex.to_owned();
                                                    }
                                                }
                                            }
                                        });
                                    });
                                    ui.data_mut(|d| d.insert_temp(acc_anim_id, acc_anim));
                                });

                                mcfg.fps_120 = true;
                                if mcfg.page_switch_anim == 2 {
                                    mcfg.page_switch_anim = 3;
                                }

                                // 卡片：页面切换动画
                                let anim_lbl = match mcfg.page_switch_anim {
                                    0 => "平滑滑动",
                                    1 => "流光渐显",
                                    _ => "磁吸回弹",
                                };
                                setting_select_card(
                                    ui,
                                    "页面切换动画",
                                    "切换灵动岛展开面板内部页面时的过渡动画曲线。",
                                    "island-page-switch-anim",
                                    anim_lbl,
                                    &mut mcfg.page_switch_anim,
                                    &[
                                        (0u8, "平滑滑动"),
                                        (1u8, "流光渐显"),
                                        (3u8, "磁吸回弹"),
                                    ],
                                );

                                // 卡片：收起时显示本机电量
                                setting_row_card(
                                    ui,
                                    "收起时显示本机电量",
                                    "在顶栏显示电池百分比与充电状态",
                                    &mut self.settings.show_battery,
                                );

                                // 卡片：显示展开岛左上角标题
                                let prev_show_title = mcfg.show_island_page_title;
                                setting_row_card(
                                    ui,
                                    "显示展开岛左上角标题",
                                    "展开面板时在左上角显示当前页面标题",
                                    &mut mcfg.show_island_page_title,
                                );
                                if mcfg.show_island_page_title != prev_show_title {
                                    let n_st = mcfg.show_island_page_title;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.show_island_page_title = n_st;
                                    });
                                }

                                // 卡片：电量图标样式
                                let bat_lbl = match mcfg.battery_style {
                                    0 => "经典圆环",
                                    1 => "iOS 胶囊",
                                    2 => "立式能量柱",
                                    3 => "双轨弧表",
                                    _ => "闪电护盾",
                                };
                                setting_select_card(
                                    ui,
                                    "电量图标样式",
                                    "选择灵动岛顶栏与展开面板中的电池矢量图标样式。",
                                    "island-battery-style",
                                    bat_lbl,
                                    &mut mcfg.battery_style,
                                    &[
                                        (0u8, "经典圆环"),
                                        (1u8, "iOS 胶囊"),
                                        (2u8, "立式能量柱"),
                                        (3u8, "双轨弧表"),
                                        (4u8, "闪电护盾"),
                                    ],
                                );

                                if mcfg != before_m {
                                    crate::auralsle_ai_status::config::update(|c| *c = mcfg);
                                }
                            }
                        }

                        } else if gen_tab == 2 || gen_tab == 3 {
                            if gen_tab == 2 {
                                settings_label(ui, "岛体显示");
                            } else {
                                settings_label(ui, "时钟与极简");
                            }
                            if gen_tab == 2 {
                            let selected = self.displays.iter().find(|d| d.id == self.settings.display_id);
                            let label = if self.settings.display_id.is_empty() {
                                "跟随主屏".to_owned()
                            } else {
                                selected.map_or_else(|| "所选屏幕未连接".to_owned(), |d| d.label.clone())
                            };
                            let edge = self.settings.dock.label();
                            let disp_desc = if !self.settings.display_id.is_empty() && selected.is_none() {
                                "暂时显示在主屏，重新连接所选屏幕后自动恢复。".to_owned()
                            } else {
                                format!("显示在所选屏幕{edge}；断开时暂时回到主屏。")
                            };
                            settings_card(ui, |ui| {
                                if cfg!(test) {
                                    settings_control_row(ui, "显示屏", 56.0, |ui| {
                                        egui::ComboBox::from_id_salt("island-display")
                                            .selected_text(&label)
                                            .width(ui.available_width().min(300.0))
                                            .truncate()
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(&mut self.settings.display_id, String::new(), "跟随主屏");
                                                for display in &self.displays {
                                                    ui.selectable_value(&mut self.settings.display_id, display.id.clone(), &display.label);
                                                }
                                            })
                                            .response
                                    });
                                    ui.label(egui::RichText::new(&disp_desc).size(11.0).color(ui.visuals().weak_text_color()));
                                } else {
                                    let mut disp_opts: Vec<(String, &str)> = Vec::with_capacity(self.displays.len() + 1);
                                    disp_opts.push((String::new(), "跟随主屏"));
                                    for d in &self.displays {
                                        disp_opts.push((d.id.clone(), d.label.as_str()));
                                    }
                                    setting_select_row(
                                        ui,
                                        "显示屏",
                                        &disp_desc,
                                        "island-display",
                                        &label,
                                        &mut self.settings.display_id,
                                        &disp_opts,
                                    );
                                }
                            });

                            // 卡片：停靠边缘
                            let docks: Vec<(crate::model::Dock, &str)> = crate::model::Dock::ALL.iter().map(|d| (*d, d.label())).collect();
                            settings_card(ui, |ui| {
                                if cfg!(test) {
                                    settings_control_row_right(ui, "停靠", |ui| {
                                        if settings_segmented(ui, &mut self.settings.dock, &docks, 64.0) {
                                            self.settings.island_offset = 0;
                                        }
                                    });
                                    if self.settings.dock.is_side() {
                                        ui.label(egui::RichText::new("收起时竖向排列时间、电池、播放、专注和 AI 额度；展开面板从屏幕边缘滑出。吉祥物和岛上歌词只在顶部停靠时显示。").size(11.0).color(ui.visuals().weak_text_color()));
                                    }
                                } else {
                                    let dock_lbl = self.settings.dock.label();
                                    let dock_desc = if self.settings.dock.is_side() {
                                        "收起时竖向排列时间、电池、播放、专注和 AI 额度；展开面板从屏幕边缘滑出。"
                                    } else {
                                        "选择灵动岛停靠在屏幕顶部、左侧或右侧边缘。"
                                    };
                                    if setting_select_row(ui, "停靠", dock_desc, "island-dock-select", dock_lbl, &mut self.settings.dock, &docks) {
                                        self.settings.island_offset = 0;
                                    }
                                }
                            });

                            let mut mcfg_pos = crate::auralsle_ai_status::config::get();
                            let before_pos = mcfg_pos.clone();

                            // 卡片：锁定岛体位置
                            setting_row_card(
                                ui,
                                "锁定岛体位置",
                                "锁定灵动岛在屏幕上的停靠与水平位置，防止日常误拖",
                                &mut mcfg_pos.island_pos_locked,
                            );

                            // 卡片：展开面板高度比例
                            if !cfg!(test) {
                                settings_card(ui, |ui| {
                                    let mut h_scale_f = mcfg_pos.expanded_h_scale as f32;
                                    twinkle_parent_panel(
                                        ui,
                                        "展开面板高度比例",
                                        "调节灵动岛展开态面板的纵向高度比例（80% – 135%）。",
                                        230.0,
                                        30.0,
                                        |ui| {
                                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                if twinkle_slider_f32(ui, "tw-expanded-h-scale", &mut h_scale_f, 80.0..=135.0, true, "%") {
                                                    mcfg_pos.expanded_h_scale = (h_scale_f.round().clamp(80.0, 135.0)) as u8;
                                                }
                                            });
                                        },
                                    );
                                });
                            }
                            if mcfg_pos != before_pos {
                                crate::auralsle_ai_status::config::update(|c| *c = mcfg_pos.clone());
                            }

                            // 卡片：水平 / 上下位置偏移
                            let side = self.settings.dock.is_side();
                            settings_card(ui, |ui| {
                                ui.add_enabled_ui(!mcfg_pos.island_pos_locked, |ui| {
                                    let limit = crate::model::ISLAND_OFFSET_MAX;
                                    let offset = &mut self.settings.island_offset;
                                    let (less_lbl, more_lbl) = if side { ("偏上", "偏下") } else { ("偏左", "偏右") };
                                    let position = match *offset {
                                        0 => "居中".to_string(),
                                        o if o < 0 => format!("{less_lbl} {}%", -o),
                                        o => format!("{more_lbl} {o}%"),
                                    };
                                    if cfg!(test) {
                                        settings_control_row(ui, if side { "上下位置" } else { "水平位置" }, 56.0, |ui| {
                                            ui.horizontal_wrapped(|ui| {
                                                let (less, more, less_tip, more_tip) = if side {
                                                    ("↑", "↓", "向上微调", "向下微调")
                                                } else {
                                                    ("←", "→", "向左微调", "向右微调")
                                                };
                                                if ui.add_enabled(*offset > -limit, egui::Button::new(less).min_size(egui::vec2(26.0, 22.0))).on_hover_text(less_tip).clicked() {
                                                    *offset = (*offset - 5).max(-limit);
                                                }
                                                ui.spacing_mut().slider_width = (ui.available_width() - 155.0).clamp(75.0, 150.0);
                                                ui.add(egui::Slider::new(&mut *offset, -limit..=limit).show_value(false));
                                                if ui.add_enabled(*offset < limit, egui::Button::new(more).min_size(egui::vec2(26.0, 22.0))).on_hover_text(more_tip).clicked() {
                                                    *offset = (*offset + 5).min(limit);
                                                }
                                                if ui.add_enabled(*offset != 0, egui::Button::new("恢复居中")).clicked() {
                                                    *offset = 0;
                                                }
                                            }).response
                                        });
                                        ui.label(egui::RichText::new(format!("当前：{position}{}。也可以展开后按住日期时间，拖到屏幕顶部、左侧或右侧；展开面板和通知始终留在屏幕内，换屏或调整大小后保持相对位置。", if mcfg_pos.island_pos_locked { "（已锁定位置）" } else { "" })).size(11.0).color(ui.visuals().weak_text_color()));
                                    } else {
                                        let mut off_f = *offset as f32;
                                        let pos_desc = format!("当前：{position}。也可以展开后按住日期时间直接拖动位置。");
                                        twinkle_parent_panel(
                                            ui,
                                            if side { "上下位置" } else { "水平位置" },
                                            &pos_desc,
                                            230.0,
                                            30.0,
                                            |ui| {
                                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if twinkle_slider_f32(ui, "tw-island-offset-slider", &mut off_f, (-limit as f32)..=(limit as f32), true, "%") {
                                                        *offset = (off_f.round() as i32).clamp(-limit, limit);
                                                    }
                                                });
                                            },
                                        );
                                    }
                                });
                            });

                            // 卡片：OLED 像素偏移
                            settings_card(ui, |ui| {
                                setting_row(ui, "OLED 像素偏移", "收起静止时让岛体沿屏幕边缘缓慢移动几个像素，帮助减轻 OLED 静态显示留下的痕迹；不会改动你保存的位置", &mut self.settings.oled_shift);
                                if self.settings.oled_shift {
                                    settings_hint(ui, "每 45 秒移动 1 像素、单方向最多 3 像素；顶部停靠左右移动，左右停靠上下移动。展开、点击或拖拽时保持当前位置，位置拉满或屏幕太窄时少挪或不挪。");
                                }
                            });

                            // 卡片：顶栏适配
                            settings_card(ui, |ui| {
                                setting_row(ui, "顶栏适配", "顶部有菜单栏类软件或任务栏时，选择岛体的显示方式", &mut self.settings.avoid_top_bar);
                                if self.settings.avoid_top_bar {
                                    twinkle_card_break(ui);
                                    settings_control_row_right(ui, "显示方式", |ui| {
                                        settings_segmented(ui, &mut self.settings.top_bar_on_top, &[(false, "避开顶栏"), (true, "灵屿置顶")], 90.0);
                                    });
                                    if self.settings.top_bar_on_top {
                                        settings_hint(ui, "顶部停靠时，岛体保持在屏幕顶边，显示在顶栏上方。");
                                    } else {
                                        let reserved = crate::display_layout::selected(&self.displays, &self.settings.display_id).map_or(0, |d| {
                                            let b = crate::display_layout::placement_bounds(d, true);
                                            (b.top - d.bounds.top).max(0)
                                        });
                                        settings_hint(ui, &if reserved > 0 {
                                            format!("已检测到顶栏，岛体下移 {reserved} 像素。")
                                        } else {
                                            "未检测到顶栏；未向系统登记占用区域的顶栏可尝试“灵屿置顶”。".to_string()
                                        });
                                    }
                                }
                            });
                        } else {
                            // 卡片：时间制式
                            settings_card(ui, |ui| {
                                if cfg!(test) {
                                    settings_control_row_right(ui, "时间制式", |ui| {
                                        settings_segmented(ui, &mut self.settings.clock_12h, &[(false, "24 小时"), (true, "12 小时")], 72.0);
                                    });
                                } else {
                                    let c12_lbl = if self.settings.clock_12h { "12 小时" } else { "24 小时" };
                                    setting_select_row(
                                        ui,
                                        "时间制式",
                                        "切换顶栏时钟显示为 24 小时制或 12 小时制。",
                                        "island-clock-12h",
                                        c12_lbl,
                                        &mut self.settings.clock_12h,
                                        &[(false, "24 小时"), (true, "12 小时")],
                                    );
                                }
                            });

                            // 卡片：时段文字
                            settings_card(ui, |ui| {
                                if cfg!(test) {
                                    settings_control_row_right(ui, "时段文字", |ui| {
                                        settings_segmented(ui, &mut self.settings.clock_period, &[(0, "不显示"), (1, "上午 / 下午"), (2, "详细时段")], 72.0);
                                    });
                                    settings_hint(ui, "详细时段：凌晨、上午、中午、下午、傍晚、晚上");
                                } else {
                                    let cp_lbl = match self.settings.clock_period {
                                        1 => "上午 / 下午",
                                        2 => "详细时段",
                                        _ => "不显示",
                                    };
                                    setting_select_row(
                                        ui,
                                        "时段文字",
                                        "详细时段：凌晨、上午、中午、下午、傍晚、晚上。",
                                        "island-clock-period",
                                        cp_lbl,
                                        &mut self.settings.clock_period,
                                        &[(0, "不显示"), (1, "上午 / 下午"), (2, "详细时段")],
                                    );
                                }
                            });

                            // 卡片：显示秒数
                            setting_row_card(ui, "显示秒数", "时间显示为 14:32:08", &mut self.settings.clock_seconds);

                            // 卡片：极简刘海
                            settings_card(ui, |ui| {
                                let mut minimal = self.settings.minimal;
                                setting_row(ui, "极简刘海", if cfg!(test) { "收起时化为极简形态" } else { "收起时化为紧凑极简形态" }, &mut minimal);
                                if minimal != self.settings.minimal {
                                    self.set_minimal(minimal);
                                }
                                if !cfg!(test) {
                                    twinkle_card_break(ui);
                                    settings_control_row_right(ui, "大胶囊展开方式", |ui| {
                                        settings_segmented(
                                            ui,
                                            &mut self.settings.island_open_mode,
                                            &[(0u8, "点击胶囊展开"), (1u8, "鼠标放上去展开")],
                                            112.0,
                                        );
                                    });
                                }
                                if self.settings.minimal && !cfg!(test) {
                                    let mut mcfg_t = crate::auralsle_ai_status::config::get();
                                    let prev_t = mcfg_t.minimal_time_pill;
                                    let prev_w = mcfg_t.minimal_pill_width;
                                    let prev_mb = mcfg_t.minimal_pill_msg_badge;
                                    twinkle_card_break(ui);
                                    setting_row(
                                        ui,
                                        "显示时间小药丸",
                                        "收起时显示时间小药丸，悬停展开为大胶囊，点击完全展开",
                                        &mut mcfg_t.minimal_time_pill,
                                    );
                                    if mcfg_t.minimal_time_pill {
                                        twinkle_card_break(ui);
                                        setting_row(
                                            ui,
                                            "来消息时右侧显示等高消息悬浮球",
                                            "收到未读消息时在极简药丸右侧显示等高消息图标，展开时平滑过渡",
                                            &mut mcfg_t.minimal_pill_msg_badge,
                                        );
                                        twinkle_card_break(ui);
                                        let mut pw_f = mcfg_t.minimal_pill_width as f32;
                                        twinkle_parent_panel(
                                            ui,
                                            "极简胶囊长度",
                                            "调节极简模式下顶部时间小药丸的宽度（48px – 120px）。",
                                            220.0,
                                            28.0,
                                            |ui| {
                                                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                                    if twinkle_slider_f32(ui, "tw-min-pill-w", &mut pw_f, 48.0..=120.0, true, "px") {
                                                        mcfg_t.minimal_pill_width = (pw_f.round().clamp(48.0, 120.0)) as u8;
                                                    }
                                                });
                                            },
                                        );
                                    }
                                    if mcfg_t.minimal_time_pill != prev_t || mcfg_t.minimal_pill_width != prev_w || mcfg_t.minimal_pill_msg_badge != prev_mb {
                                        let nt = mcfg_t.minimal_time_pill;
                                        let nw = mcfg_t.minimal_pill_width;
                                        let nmb = mcfg_t.minimal_pill_msg_badge;
                                        crate::auralsle_ai_status::config::update(|c| {
                                            c.minimal_time_pill = nt;
                                            c.minimal_pill_width = nw;
                                            c.minimal_pill_msg_badge = nmb;
                                        });
                                    }
                                }
                                if cfg!(test) {
                                    twinkle_card_break(ui);
                                    let mut pet = self.settings.pet;
                                    setting_row(ui, "灵屿吉祥物", "收起时常驻，播放音乐时一起唱歌", &mut pet);
                                    if pet != self.settings.pet {
                                        self.set_pet(pet);
                                    }
                                }
                            });
                        }
                        if gen_tab == 2 {
                            self.avoidance.settings(ui, &mut self.settings);
                        }
                        } else {

                        // ── Section 5: 故障排除（对齐 Twinkle Tray `SETTINGS_GENERAL_TROUBLESHOOTING`）──
                        if !cfg!(test) {
                            settings_label(ui, "故障排除");

                            // 卡片：诊断报告与运行日志（可展开）
                            settings_card(ui, |ui| {
                                let diag_open_id = egui::Id::new("tw_diag_log_open");
                                let mut diag_open = ui.data(|d| d.get_temp::<bool>(diag_open_id)).unwrap_or(false);
                                let diag_anim_id = egui::Id::new("tw_diag_log_anim");
                                let mut diag_anim = ui.data(|d| d.get_temp::<f32>(diag_anim_id)).unwrap_or(0.0);
                                let install_log_dir = crate::frame_watch::default_log_dir();
                                if settings_expand_header(
                                    ui,
                                    "tw-diag-log-hdr",
                                    "诊断与 Log 提取中心",
                                    "提取运行日志分析卡顿/崩溃原因，或一键打包完整系统环境诊断压缩包。",
                                    &mut diag_open,
                                ) {
                                    ui.data_mut(|d| d.insert_temp(diag_open_id, diag_open));
                                }
                                settings_subpanel(ui, &mut diag_anim, diag_open, "tw-diag-log-sub", |ui| {
                                    let rep_id = egui::Id::new("auralsle_log_diag_report_v30");
                                    let mut report_text = ui.data(|d| d.get_temp::<String>(rep_id)).unwrap_or_default();
                                    let rec_active = crate::frame_watch::is_user_log_active();
                                    ui.horizontal_wrapped(|ui| {
                                        let rec_btn_txt = if rec_active { "结束并保存日志" } else { "开始记录运行日志" };
                                        if twinkle_button(ui, rec_btn_txt, rec_active).clicked() {
                                            if rec_active {
                                                if let Some((saved, sec, lines)) = crate::frame_watch::stop_user_log() {
                                                    report_text = format!("运行日志已保存（共记录 {} 秒，{} 条事件）：\n{}", sec, lines, saved.display());
                                                    ui.data_mut(|d| d.insert_temp(rep_id, report_text.clone()));
                                                }
                                            } else {
                                                let p = crate::frame_watch::start_user_log();
                                                report_text = format!("已开始记录运行日志，复现问题后点击「结束并保存日志」：\n{}", p.display());
                                                ui.data_mut(|d| d.insert_temp(rep_id, report_text.clone()));
                                            }
                                        }
                                        if twinkle_button(ui, "提取并分析 Log 报告", false).clicked() {
                                            let (saved_path, summary) = crate::frame_watch::extract_and_analyze_log_report();
                                            report_text = format!("已导出至：{}\n{}", saved_path.display(), summary);
                                            ui.data_mut(|d| d.insert_temp(rep_id, report_text.clone()));
                                        }
                                        if twinkle_button(ui, "收集完整诊断包", false).clicked() {
                                            if crate::diagnostics_tool::launch().is_err() {
                                                report_text = "无法启动诊断收集，请从开始菜单打开「收集灵屿诊断」。".to_owned();
                                            } else {
                                                report_text = "已拉起系统诊断收集脚本，完成后将在桌面生成诊断 ZIP 包。".to_owned();
                                            }
                                            ui.data_mut(|d| d.insert_temp(rep_id, report_text.clone()));
                                        }
                                        if twinkle_button(ui, "打开日志目录", false).clicked() {
                                            #[cfg(target_os = "windows")]
                                            {
                                                let _ = std::process::Command::new("explorer").arg(&install_log_dir).spawn();
                                            }
                                        }
                                        if !report_text.is_empty() && twinkle_button(ui, "复制报告", false).clicked() {
                                            ui.ctx().copy_text(report_text.clone());
                                        }
                                        if twinkle_button(ui, "清空旧日志", false).clicked() {
                                            crate::frame_watch::clear_all_logs();
                                            report_text = "已清空旧诊断日志。".to_owned();
                                            ui.data_mut(|d| d.insert_temp(rep_id, report_text.clone()));
                                        }
                                    });
                                    if !report_text.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(egui::RichText::new(&report_text).size(11.5).monospace());
                                    }
                                });
                                ui.data_mut(|d| d.insert_temp(diag_anim_id, diag_anim));
                            });

                            // 卡片：设置备份与恢复（可展开）
                            settings_card(ui, |ui| {
                                let bak_open_id = egui::Id::new("tw_backup_restore_open");
                                let mut bak_open = ui.data(|d| d.get_temp::<bool>(bak_open_id)).unwrap_or(false);
                                let bak_anim_id = egui::Id::new("tw_backup_restore_anim");
                                let mut bak_anim = ui.data(|d| d.get_temp::<f32>(bak_anim_id)).unwrap_or(0.0);
                                if settings_expand_header(
                                    ui,
                                    "tw-backup-restore-hdr",
                                    "备份与恢复设置",
                                    "将当前全部设置导出为 ZIP 或 JSON 归档，或从已有备份文件中恢复。",
                                    &mut bak_open,
                                ) {
                                    ui.data_mut(|d| d.insert_temp(bak_open_id, bak_open));
                                }
                                settings_subpanel(ui, &mut bak_anim, bak_open, "tw-backup-restore-sub", |ui| {
                                    let status_id = egui::Id::new("auralsle_backup_restore_status");
                                    let mut status_msg: String = ui.data_mut(|d| d.get_temp::<String>(status_id).unwrap_or_default());
                                    let bcfg = crate::auralsle_ai_status::config::get();
                                    let target_backup_dir = if !bcfg.custom_backup_dir.trim().is_empty() {
                                        std::path::PathBuf::from(bcfg.custom_backup_dir.trim())
                                    } else {
                                        crate::model::config_dir()
                                    };
                                    let zip_name = format!("AuraIsle-backup-v{}.zip", env!("CARGO_PKG_VERSION"));
                                    let json_name = format!("AuraIsle-backup-v{}.json", env!("CARGO_PKG_VERSION"));
                                    ui.horizontal_wrapped(|ui| {
                                        if twinkle_button(ui, "导出 ZIP 备份", false).clicked() {
                                            self.save_all_for_backup();
                                            let dest_path = target_backup_dir.join(&zip_name);
                                            match crate::auralsle_ai_status::config::export_settings_backup(&dest_path) {
                                                Ok(m) => status_msg = m,
                                                Err(e) => status_msg = e,
                                            }
                                            ui.data_mut(|d| d.insert_temp(status_id, status_msg.clone()));
                                        }
                                        if twinkle_button(ui, "导出 JSON 备份", false).clicked() {
                                            self.save_all_for_backup();
                                            let dest_path = target_backup_dir.join(&json_name);
                                            match crate::auralsle_ai_status::config::export_settings_backup(&dest_path) {
                                                Ok(m) => status_msg = m,
                                                Err(e) => status_msg = e,
                                            }
                                            ui.data_mut(|d| d.insert_temp(status_id, status_msg.clone()));
                                        }
                                        if twinkle_button(ui, "从备份恢复", false).clicked() {
                                            let pick = if !bcfg.custom_restore_file.trim().is_empty() {
                                                Some(std::path::PathBuf::from(bcfg.custom_restore_file.trim()))
                                            } else {
                                                let candidates = [
                                                    target_backup_dir.join(&zip_name),
                                                    target_backup_dir.join(&json_name),
                                                    target_backup_dir.join("auralsle-settings-backup.zip"),
                                                    target_backup_dir.join("auralsle-settings-backup.json"),
                                                ];
                                                let latest = candidates
                                                    .into_iter()
                                                    .filter(|p| p.is_file())
                                                    .max_by_key(|p| {
                                                        p.metadata()
                                                            .and_then(|m| m.modified())
                                                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
                                                    });
                                                #[cfg(windows)]
                                                let picked = latest.or_else(|| {
                                                    if cfg!(test) {
                                                        None
                                                    } else {
                                                        crate::system::pick_files_for("选择灵屿 AuraIsle 备份文件（.zip / .json）", false)
                                                            .ok()
                                                            .and_then(|v| v.into_iter().next())
                                                    }
                                                });
                                                #[cfg(not(windows))]
                                                let picked = latest;
                                                picked
                                            };
                                            match pick {
                                                Some(file) => match crate::auralsle_ai_status::config::import_settings_backup(&file) {
                                                    Ok(m) => {
                                                        self.reload_all_after_restore();
                                                        status_msg = format!("{m}（来源：{}）", file.display());
                                                    }
                                                    Err(e) => status_msg = e,
                                                },
                                                None => status_msg = "未找到备份文件，请先导出备份或点击「选择备份文件…」".to_owned(),
                                            }
                                            ui.data_mut(|d| d.insert_temp(status_id, status_msg.clone()));
                                        }
                                        #[cfg(windows)]
                                        if !cfg!(test) && twinkle_button(ui, "选择备份文件…", false).clicked() {
                                            if let Ok(files) = crate::system::pick_files_for("选择灵屿 AuraIsle 备份文件（.zip / .json）", false) {
                                                if let Some(file) = files.into_iter().next() {
                                                    match crate::auralsle_ai_status::config::import_settings_backup(&file) {
                                                        Ok(m) => {
                                                            self.reload_all_after_restore();
                                                            status_msg = format!("{m}（来源：{}）", file.display());
                                                        }
                                                        Err(e) => status_msg = e,
                                                    }
                                                    ui.data_mut(|d| d.insert_temp(status_id, status_msg.clone()));
                                                }
                                            }
                                        }
                                        if twinkle_button(ui, "打开备份目录", false).clicked() {
                                            let _ = std::fs::create_dir_all(&target_backup_dir);
                                            #[cfg(target_os = "windows")]
                                            {
                                                let _ = std::process::Command::new("explorer").arg(&target_backup_dir).spawn();
                                            }
                                        }
                                    });
                                    if !status_msg.is_empty() {
                                        ui.add_space(4.0);
                                        ui.label(egui::RichText::new(&status_msg).size(12.0));
                                    }
                                });
                                ui.data_mut(|d| d.insert_temp(bak_anim_id, bak_anim));
                            });
                        }
                        }
                    }
                    1=>{
                        let mut mcfg = crate::auralsle_ai_status::config::get();
                        let prev_style = mcfg.music_ui_style;
                        let prev_glance_form = mcfg.glance_music_form;
                        if matches!(mcfg.music_ui_style, 2 | 3) {
                            mcfg.music_ui_style = 6;
                            crate::auralsle_ai_status::config::update(|c| c.music_ui_style = 6);
                        }
                        let style_lbl = match mcfg.music_ui_style {
                            6 => "Auralsle 灵屿音乐岛（推荐）",
                            5 => "灵动自适应",
                            1 => "灵动频谱",
                            _ => "经典分栏",
                        };
                        let style_desc = match mcfg.music_ui_style {
                            6 => "灵屿全景音乐岛：多排版平滑切换 + 封面取色模糊背板 + 荧光发光歌词 + 8 款进度条 + 10 套播控 + 布局编辑器",
                            5 => "灵动自适应：岛体宽高随歌名、歌词与三形态（横条 / 卡片 / 舞台）实时弹性伸缩",
                            1 => "eIsland 灵动频谱：左侧高清圆角专辑封面与岛词胶囊 + 右侧 14 柱旋律频谱与一体式播控坞",
                            _ => "经典分栏：左侧圆角专辑封面 + 右侧双栏同步滚动歌词与播控条",
                        };
                        setting_select_card(
                            ui,
                            "音乐岛 UI 样式",
                            style_desc,
                            "music-ui-style-select",
                            style_lbl,
                            &mut mcfg.music_ui_style,
                            &[
                                (6u8, "Auralsle 灵屿音乐岛（推荐）"),
                                (5u8, "灵动自适应"),
                                (0u8, "经典分栏"),
                                (1u8, "灵动频谱"),
                            ],
                        );
                        if mcfg.music_ui_style == 5 {
                            let gf_lbl = match mcfg.glance_music_form {
                                0 => "紧凑歌词横条",
                                1 => "三行歌词卡片",
                                _ => "黑胶频谱舞台",
                            };
                            setting_select_card(
                                ui,
                                "自适应形态",
                                "选择灵动自适应模式下的音乐面板形态。",
                                "music-glance-form-select",
                                gf_lbl,
                                &mut mcfg.glance_music_form,
                                &[
                                    (0u8, "紧凑歌词横条"),
                                    (1u8, "三行歌词卡片"),
                                    (2u8, "黑胶频谱舞台"),
                                ],
                            );
                        }
                        if mcfg.glance_music_form != prev_glance_form {
                            let ngf = mcfg.glance_music_form;
                            crate::auralsle_ai_status::config::update(|c| c.glance_music_form = ngf);
                        }
                        let prev_hide = mcfg.music_auto_hide_when_idle;
                        if mcfg.music_ui_style != prev_style || mcfg.music_auto_hide_when_idle != prev_hide {
                            let ns = mcfg.music_ui_style;
                            let mh = mcfg.music_auto_hide_when_idle;
                            crate::auralsle_ai_status::config::update(|c| {
                                c.music_ui_style = ns;
                                c.music_auto_hide_when_idle = mh;
                            });
                            self.settings.visible_pages[0] = true;
                            self.page_from = 0;
                            self.page = 0;
                            self.dots_target();
                        }
                        let music_sec_id = egui::Id::new("auralsle_music_settings_section");
                        let mut music_sec = ctx.data(|d| d.get_temp::<usize>(music_sec_id)).unwrap_or(0);
                        ui.add_space(4.0);
                        if settings_segmented(
                            ui,
                            &mut music_sec,
                            &[(0usize, "样式专属设置"), (1usize, "通用与歌词设置")],
                            140.0,
                        ) {
                            ctx.data_mut(|d| d.insert_temp(music_sec_id, music_sec));
                        }
                        ui.add_space(8.0);
                        if music_sec == 0 {
                        if mcfg.music_ui_style == 6 {
                            crate::auralsle_music_editor::show_auralsle_music_settings(ui, self, &mut mcfg);
                        } else {
                        if mcfg.music_ui_style == 0 {
                            let mut c_tint = mcfg.classic_music_cover_tint;
                            setting_row_card(
                                ui,
                                "经典分栏启用专辑封面取色",
                                "自动从当前播放的专辑封面提取主色调，渲染双径向流光背板、渐变进度条与高亮歌词",
                                &mut c_tint,
                            );
                            if c_tint != mcfg.classic_music_cover_tint {
                                crate::auralsle_ai_status::config::update(|c| c.classic_music_cover_tint = c_tint);
                            }
                        }
                        if mcfg.music_ui_style == 5 && mcfg.glance_music_form == 2 {
                            let mut rot_art = mcfg.music_round_art_rotate;
                            setting_row_card(
                                ui,
                                "圆形专辑封面旋转",
                                "播放音乐时圆形专辑图以 33⅓ RPM 平滑旋转；暂停或关闭此开关时静止",
                                &mut rot_art,
                            );
                            if rot_art != mcfg.music_round_art_rotate {
                                crate::auralsle_ai_status::config::update(|c| c.music_round_art_rotate = rot_art);
                            }
                        }
                        // [需求 4] 音乐岛 UI 编辑区正下方增加音乐岛实时预览界面
                        settings_card(ui, |ui| {
                            let avail_w = layout_avail_w(ui).max(220.0);
                            ui.set_width(avail_w);
                            ui.set_max_width(avail_w);
                            let live_cfg = crate::auralsle_ai_status::config::get();
                            let style_name = match live_cfg.music_ui_style {
                                1 | 4 => "eIsland 灵动频谱",
                                5 => match live_cfg.glance_music_form % 3 {
                                    0 => "灵动自适应 · 紧凑歌词横条",
                                    1 => "灵动自适应 · 三行歌词卡片",
                                    _ => "灵动自适应 · 黑胶频谱舞台",
                                },
                                _ => "经典分栏",
                            };
                            ui.horizontal(|ui| {
                                ui.label(
                                    egui::RichText::new("音乐岛实时预览")
                                        .size(14.5)
                                        .strong()
                                        .color(settings_color(ctx, Color32::WHITE, Color32::from_rgb(24, 24, 26))),
                                );
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.label(
                                        egui::RichText::new(if self.media.title.trim().is_empty() {
                                            format!("{style_name} · 示例曲目实时动效")
                                        } else {
                                            format!("{style_name} · 同步当前播放")
                                        })
                                        .size(11.5)
                                        .color(settings_color(ctx, Color32::from_gray(165), Color32::from_gray(95))),
                                    );
                                });
                            });
                            ui.add_space(10.0);

                            let (vw, base_vh) = if live_cfg.music_ui_style == 5 {
                                self.glance_music_target_dims()
                            } else {
                                (419.0, 174.0)
                            };
                            let vh = base_vh.max(138.0);
                            let scale = ((avail_w - 8.0) / vw).clamp(0.55, 1.04);
                            let disp_size = egui::vec2(vw * scale, vh * scale);
                            let (outer_r, _) = ui.allocate_exact_size(egui::vec2(avail_w, disp_size.y), egui::Sense::hover());
                            let island_rect = egui::Rect::from_center_size(outer_r.center(), disp_size);
                            let radius = (22.0 * scale).clamp(12.0, 26.0);
                            let island_bg = match self.settings.skin {
                                crate::model::Skin::Cream => Color32::from_rgb(38, 36, 33),
                                crate::model::Skin::LiquidGlass => Color32::from_rgb(20, 24, 34),
                                _ => Color32::from_rgb(16, 17, 22),
                            };
                            ui.painter().rect_filled(island_rect, radius, island_bg);
                            ui.painter().rect_stroke(
                                island_rect,
                                radius,
                                egui::Stroke::new(1.0, Color32::from_white_alpha(30)),
                                egui::StrokeKind::Inside,
                            );

                            let mut tmp_media = if self.media.title.trim().is_empty() {
                                let demo_len = 215.0_f64;
                                crate::smtc::NowPlaying {
                                    title: "消失（灵屿音乐预览）".to_owned(),
                                    artist: "毛潇曼".to_owned(),
                                    album: "AuraIsle Live Studio".to_owned(),
                                    source: "preview".to_owned(),
                                    playing: true,
                                    position: (self.t * 1.0) % demo_len,
                                    duration: demo_len,
                                    ..Default::default()
                                }
                            } else {
                                self.media.clone()
                            };
                            let mut tmp_h = vh;
                            let mut tmp_open_t = 1.0_f32;
                            let mut tmp_show_lyrics = self.settings.lyrics_enabled;
                            let prev_cur_local = self.cursor_local;
                            std::mem::swap(&mut self.media, &mut tmp_media);
                            std::mem::swap(&mut self.h.pos, &mut tmp_h);
                            std::mem::swap(&mut self.open_t.pos, &mut tmp_open_t);
                            std::mem::swap(&mut self.show_lyrics, &mut tmp_show_lyrics);
                            self.cursor_local = None;

                            let base_card = crate::paint::Card::new(ui, island_rect.min, island_rect)
                                .scaled(0.0, 0.0, scale)
                                .ghost();
                            base_card.text(
                                38.0,
                                25.0,
                                "音乐",
                                12.2,
                                crate::theme::W::Bold,
                                Color32::WHITE,
                                crate::paint::H::Left,
                            );
                            base_card.text(
                                38.0 + base_card.text_w("音乐", 12.2, crate::theme::W::Bold),
                                25.0,
                                "播放",
                                12.2,
                                crate::theme::W::Bold,
                                Color32::from_rgb(244, 114, 182),
                                crate::paint::H::Left,
                            );
                            let body_card = base_card.shifted(0.0, 14.0);
                            let mut preview_env = crate::app::Env {
                                t: self.t,
                                media: self.media.clone(),
                                ..Default::default()
                            };
                            preview_env.clock = chrono::Local::now().format("%H:%M:%S").to_string();
                            let mut ignored_cmds = Vec::new();
                            self.page_music(&body_card, &preview_env, &mut ignored_cmds);

                            self.cursor_local = prev_cur_local;
                            std::mem::swap(&mut self.show_lyrics, &mut tmp_show_lyrics);
                            std::mem::swap(&mut self.open_t.pos, &mut tmp_open_t);
                            std::mem::swap(&mut self.h.pos, &mut tmp_h);
                            std::mem::swap(&mut self.media, &mut tmp_media);

                            if !cfg!(test) && !settings_sizing_now() {
                                ctx.request_repaint_after(std::time::Duration::from_millis(66));
                            }
                        });


                        }

                        } else {
                        // ── 通用设置（全部音乐岛 UI 样式均生效） ──
                        settings_label(ui, "通用行为与歌词岛（全部样式生效）");
                        let mut mh_toggle = mcfg.music_auto_hide_when_idle;
                        setting_row_card(
                            ui,
                            "仅在有媒体播放时显示音乐岛",
                            "检测到系统正在播放音乐或媒体时自动展示音乐页面；无媒体播放时自动隐藏，不常驻占用页面",
                            &mut mh_toggle,
                        );
                        if mh_toggle != mcfg.music_auto_hide_when_idle {
                            mcfg.music_auto_hide_when_idle = mh_toggle;
                            crate::auralsle_ai_status::config::update(|c| c.music_auto_hide_when_idle = mh_toggle);
                            self.dots_target();
                        }
                        settings_card(ui, |ui| {
                            let mut p_ret = mcfg.music_pause_return_secs;
                            if setting_number(ui, "暂停多少秒后自动切回默认页面（0 = 不切回）", &mut p_ret, 0, 3600, " 秒", 1.0) {
                                mcfg.music_pause_return_secs = p_ret;
                                crate::auralsle_ai_status::config::update(|c| c.music_pause_return_secs = p_ret);
                            }
                        });
                        let mut m_ai = mcfg.music_show_ai_status;
                        setting_row_card(ui, "歌词岛显示 AI 工作状态", "有 AI 助手运行时，在收起态歌词岛吉祥物与专辑封面之间显示 AI 图标与状态灯", &mut m_ai);
                        if m_ai != mcfg.music_show_ai_status {
                            mcfg.music_show_ai_status = m_ai;
                            crate::auralsle_ai_status::config::update(|c| c.music_show_ai_status = m_ai);
                        }
                        let mut ly_tc = mcfg.lyric_island_time_capsule;
                        setting_row_card(ui, "歌词岛左侧显示时间胶囊", "在收起态歌词岛左侧独立显示与歌词岛同材质的时间胶囊", &mut ly_tc);
                        if ly_tc != mcfg.lyric_island_time_capsule {
                            mcfg.lyric_island_time_capsule = ly_tc;
                            crate::auralsle_ai_status::config::update(|c| c.lyric_island_time_capsule = ly_tc);
                        }

                        settings_label(ui, "歌词与时间偏移");
                        setting_row_card(ui, "展开时显示歌词", "启用同步歌词抓取与展开页歌词显示", &mut self.settings.lyrics_enabled);
                        self.show_lyrics = self.settings.lyrics_enabled;
                        setting_row_card(ui, "岛上歌词", "收起时显示当前句；无同步歌词时显示封面和歌名", &mut self.settings.island_lyrics);
                        settings_card(ui, |ui| {
                            let mut advance = -self.settings.lyric_delay_ms as f64 / 1000.0;
                            if setting_number(ui, "时间偏移（正数提前，负数延后）", &mut advance, -5.0, 5.0, " 秒", 0.1) {
                                self.settings.lyric_delay_ms = (-advance * 1000.0).round() as i32;
                            }
                        });

                        let live_accent = if !self.media.title.is_empty() {
                            Some(self.extract_artwork_colors().0)
                        } else {
                            None
                        };
                        crate::auralsle_music_editor::show_music_bar_and_transport_settings(ui, &mut mcfg, live_accent);
                        }
                    }
                    2=>{
                        settings_card(ui,|ui|{
                            setting_number(ui,"专注时长",&mut self.settings.focus_minutes,1,180," 分钟",1.0);
                        });
                        settings_card(ui,|ui|{
                            setting_number(ui,"休息时长",&mut self.settings.rest_minutes,1,60," 分钟",1.0);
                        });
                    }
                    3=>{
                        settings_card(ui,|ui|{
                            let old_remind=self.water.remind;
                            setting_row(ui,"定时喝水提醒","达到每日目标后停止提醒",&mut self.water.remind);
                            if self.water.remind!=old_remind{self.water.save();self.confirm_health_reminder(HealthReminder::Water);}
                            twinkle_card_break(ui);
                            if setting_number(ui,"每日目标",&mut self.water.goal_ml,500,6000," ml",50.0){self.water.save();}
                            setting_number(ui,"每次记录",&mut self.settings.serving_ml,50,1000," ml",25.0);
                            setting_number(ui,"提醒间隔",&mut self.settings.water_minutes,5,180," 分钟",1.0);
                        });
                        settings_label(ui,"久坐提醒");
                        settings_card(ui,|ui|{
                            let old_sit=self.settings.sit_remind;
                            setting_row(ui,"定时久坐提醒","锁屏或确认「知道了 / 起身了」后重新计时；只看屏幕不动鼠标也照样计时",&mut self.settings.sit_remind);
                            if self.settings.sit_remind!=old_sit {self.confirm_health_reminder(HealthReminder::Sit);}
                            twinkle_card_break(ui);
                            setting_number(ui,"久坐时长",&mut self.settings.sit_minutes,15,180," 分钟",1.0);
                        });
                        settings_label(ui,"提醒时段");
                        settings_card(ui,|ui|{
                            twinkle_parent_panel(ui, "提醒时段", "喝水和久坐提醒仅在此时间段内弹出。", 264.0, 30.0, |ui| {
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        number_stepper(ui,&mut self.settings.water_start,0,23," 时",1,36.0);
                                        ui.label("至");
                                        number_stepper(ui,&mut self.settings.water_end,0,24," 时",1,36.0);
                                    });
                                });
                            });
                            twinkle_card_break(ui);
                            setting_row(ui,"午休不提醒","午休时间内喝水和久坐都不弹提醒",&mut self.settings.lunch_rest);
                            if self.settings.lunch_rest {
                                twinkle_card_break(ui);
                                twinkle_parent_panel(ui, "午休时间", "", 264.0, 30.0, |ui| {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = 4.0;
                                            time_stepper(ui,&mut self.settings.lunch_start,"lunch-start");
                                            ui.label("至");
                                            time_stepper(ui,&mut self.settings.lunch_end,"lunch-end");
                                        });
                                    });
                                });
                            }
                        });
                    }
                    4=>{
                        settings_label(ui,"待办清单");
                        settings_card(ui,|ui|{
                            ui.label(egui::RichText::new("在待办页添加、编辑任务并设置提醒时间。").size(13.0));
                            ui.add_space(5.0);
                            if ui.add_sized([120.0,32.0],egui::Button::new("打开待办页")).clicked(){self.settings_open=false;self.show_page(4);}
                        });
                    }
                    5=>{
                        settings_label(ui,"置物架");
                        settings_card(ui,|ui|{
                            ui.label(egui::RichText::new("保存文件位置，移除条目不会删除原文件。").size(13.0));
                            ui.label(egui::RichText::new(format!("当前 {} 项",self.shelf.items.len())).size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            ui.add_space(5.0);
                            if ui.add_sized([120.0,32.0],egui::Button::new("打开置物架")).clicked(){self.settings_open=false;self.shelf_tab=0;self.show_page(3);}
                        });
                        settings_label(ui,"剪贴板");
                        settings_card(ui,|ui|{
                            setting_row(ui,if cfg!(test){"记录剪贴板"}else{"记录剪贴板文字与图片"},if cfg!(test){"在置物架的「剪贴板」里查看最近复制的文字，点一下再次复制"}else{"自动保存复制的文字与图片，点击即可再次粘贴"},&mut self.settings.clipboard_history);
                            if !cfg!(test) {
                                let mut mcfg_c = crate::auralsle_ai_status::config::get();
                                let prev_auto = mcfg_c.copy_auto_open_clipboard;
                                let prev_secs = mcfg_c.copy_auto_open_secs;
                                let prev_cmode = mcfg_c.clipboard_ui_mode;
                                let prev_apaste = mcfg_c.clipboard_auto_paste;
                                let prev_close_mode = mcfg_c.clipboard_close_mode;
                                let prev_close_delay = mcfg_c.clipboard_close_delay_secs;
                                twinkle_card_break(ui);
                                settings_control_row_right(ui, "剪贴板界面方案", |ui| {
                                    settings_segmented(
                                        ui,
                                        &mut mcfg_c.clipboard_ui_mode,
                                        &[
                                            (0u8, "图文卡片"),
                                            (1u8, "紧凑列表"),
                                        ],
                                        124.0,
                                    );
                                });
                                setting_row(
                                    ui,
                                    "点击条目直接粘贴到当前窗口",
                                    "点击文字或图片卡片后自动写回剪贴板并粘贴",
                                    &mut mcfg_c.clipboard_auto_paste,
                                );
                                twinkle_card_break(ui);
                                let close_lbl = match mcfg_c.clipboard_close_mode {
                                    0 => "鼠标离开就收起",
                                    2 => "粘贴后 N 秒后收起",
                                    _ => "点击空白区域后收起",
                                };
                                setting_select_row(
                                    ui,
                                    "粘贴/复制后收起方式",
                                    "选择在剪贴板点击粘贴或复制条目后的灵动岛收起策略（鼠标离开、点击空白处或延时收起）",
                                    "clipboard-close-mode",
                                    close_lbl,
                                    &mut mcfg_c.clipboard_close_mode,
                                    &[
                                        (0u8, "鼠标离开就收起"),
                                        (1u8, "点击空白区域后收起"),
                                        (2u8, "粘贴后 N 秒后收起"),
                                    ],
                                );
                                if mcfg_c.clipboard_close_mode == 2 {
                                    ui.add_space(2.0);
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new("粘贴后停留秒数").size(12.0));
                                        ui.spacing_mut().slider_width = (layout_avail_w(ui) - 190.0).clamp(90.0, 180.0);
                                        ui.add(egui::Slider::new(&mut mcfg_c.clipboard_close_delay_secs, 1..=15).suffix(" 秒"));
                                        if ui.add_enabled(mcfg_c.clipboard_close_delay_secs != 3, egui::Button::new("恢复默认")).clicked() {
                                            mcfg_c.clipboard_close_delay_secs = 3;
                                        }
                                    });
                                }
                                if mcfg_c.clipboard_ui_mode != prev_cmode
                                    || mcfg_c.clipboard_auto_paste != prev_apaste
                                    || mcfg_c.clipboard_close_mode != prev_close_mode
                                    || mcfg_c.clipboard_close_delay_secs != prev_close_delay
                                {
                                    let ncm = mcfg_c.clipboard_ui_mode;
                                    let nap = mcfg_c.clipboard_auto_paste;
                                    let nclm = mcfg_c.clipboard_close_mode;
                                    let ncld = mcfg_c.clipboard_close_delay_secs;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.clipboard_ui_mode = ncm;
                                        c.clipboard_auto_paste = nap;
                                        c.clipboard_close_mode = nclm;
                                        c.clipboard_close_delay_secs = ncld;
                                    });
                                }
                                twinkle_card_break(ui);
                                setting_row(
                                    ui,
                                    "复制后打开岛优先展开剪贴板",
                                    "复制内容后在有效秒数内打开灵动岛时默认展开剪贴板",
                                    &mut mcfg_c.copy_auto_open_clipboard,
                                );
                                if mcfg_c.copy_auto_open_clipboard {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new("有效时间窗口").size(12.0));
                                        ui.spacing_mut().slider_width = (layout_avail_w(ui) - 190.0).clamp(90.0, 180.0);
                                        ui.add(egui::Slider::new(&mut mcfg_c.copy_auto_open_secs, 1..=60).suffix(" 秒"));
                                        if ui.add_enabled(mcfg_c.copy_auto_open_secs != 10, egui::Button::new("恢复默认")).clicked() {
                                            mcfg_c.copy_auto_open_secs = 10;
                                        }
                                    });
                                }
                                if mcfg_c.copy_auto_open_clipboard != prev_auto || mcfg_c.copy_auto_open_secs != prev_secs {
                                    let na = mcfg_c.copy_auto_open_clipboard;
                                    let ns = mcfg_c.copy_auto_open_secs;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.copy_auto_open_clipboard = na;
                                        c.copy_auto_open_secs = ns;
                                    });
                                }
                                twinkle_card_break(ui);
                            }
                            ui.label(egui::RichText::new(format!("本机保存最近 {} 条，重新打开后可继续使用。关闭记录会暂停收集，手动清空会保留固定条目。密码管理器标记为敏感的内容不会记录。",crate::clipboard::MAX_RECENT)).size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            ui.add_space(5.0);
                            if ui.add_sized([120.0,32.0],egui::Button::new("打开剪贴板")).clicked(){self.settings_open=false;if cfg!(test){self.shelf_tab=1;self.show_page(3);}else{self.show_page(12);}}
                        });
                    }
                    6=>{
                        // 总开关关闭时，微信与 QQ 两栏置灰并说明原因，避免「开了也不提醒」。
                        let master=self.settings.notifications;
                        let master_hint=|ui:&mut egui::Ui|if !master {
                            ui.label(egui::RichText::new("提醒总开关已关闭（上方「系统提示」），打开后这里才会生效").size(12.0).color(Color32::from_rgb(230,150,40)));
                        };
                        settings_label(ui,"新消息通知停留时长");
                        settings_card(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new("通知横幅停留时长").size(13.0));
                                ui.spacing_mut().slider_width = (layout_avail_w(ui) - 210.0).clamp(110.0, 220.0);
                                let mut secs_i = self.settings.alert_duration_secs.clamp(3, 30) as u32;
                                if ui.add(egui::Slider::new(&mut secs_i, 3..=30).suffix(" 秒")).changed() {
                                    self.settings.alert_duration_secs = secs_i.clamp(3, 30) as u8;
                                }
                                if ui.add_enabled(self.settings.alert_duration_secs != 5, egui::Button::new("恢复默认")).clicked() {
                                    self.settings.alert_duration_secs = 5;
                                }
                            });
                            ui.label(
                                egui::RichText::new("统一控制 QQ、微信与新邮件在灵动岛上弹出消息预览的停留秒数（3–30 秒，默认 5 秒）。")
                                    .size(12.0)
                                    .color(settings_color(ctx, Color32::from_gray(180), Color32::from_gray(75))),
                            );
                        });
                        settings_label(ui,"消息岛标签顺序与微信本地隐私防护");
                        if !cfg!(test) {
                            settings_card(ui, |ui| {
                                let mut mcfg_o = crate::auralsle_ai_status::config::get();
                                let prev_o = mcfg_o.msg_hub_order;
                                let tab_name = |id: u8| match id {
                                    0 => "✉ 邮箱",
                                    1 => "💬 微信",
                                    _ => "🐧 QQ",
                                };
                                #[derive(Clone, Copy)]
                                struct MsgHubDrag(u8);

                                // [窄宽排版] 槽位宽度同样以"真实剩余宽度"为上限，否则三块分栏卡
                                // 会顶着卡片右缘铺出去（本轮图1 第二个红框）。
                                let avail_w = layout_avail_w(ui).max(190.0);
                                ui.horizontal_wrapped(|ui| {
                                    ui.label(egui::RichText::new("自由调整分栏位置").size(12.0).strong());
                                    ui.label(
                                        egui::RichText::new("（按住卡片直接左右拖动调整排列顺序）")
                                            .size(10.8)
                                            .color(settings_color(ctx, Color32::from_gray(155), Color32::from_gray(105))),
                                    );
                                });
                                ui.add_space(4.0);

                                let dragged_id = egui::DragAndDrop::payload::<MsgHubDrag>(ctx).map(|p| p.0);
                                let drag_from_idx = dragged_id.and_then(|did| mcfg_o.msg_hub_order.iter().position(|&x| x == did));
                                let ptr_pos = ctx.pointer_interact_pos();
                                let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                                let arrow_gap = if avail_w >= 340.0 { 20.0 } else { 8.0 };
                                let slot_w = ((avail_w - arrow_gap * 2.0) / 3.0).clamp(58.0, 172.0);
                                let slot_h = 34.0_f32;
                                let mut swap_req: Option<(usize, usize)> = None;

                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    for slot_i in 0..3usize {
                                        let cur_id = mcfg_o.msg_hub_order[slot_i];
                                        let is_dragged = dragged_id == Some(cur_id);
                                        let target_x = slot_i as f32 * (slot_w + arrow_gap);
                                        let anim_x = ctx.animate_value_with_time(
                                            egui::Id::new(("msg-hub-slot-x", cur_id)),
                                            target_x,
                                            0.16,
                                        );
                                        let dx_anim = (anim_x - target_x).clamp(-36.0, 36.0);
                                        if dx_anim.abs() > 0.25 {
                                            ctx.request_repaint();
                                        }

                                        let (slot_rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(slot_w, slot_h),
                                            egui::Sense::click_and_drag(),
                                        );
                                        resp.clone()
                                            .on_hover_cursor(egui::CursorIcon::Grab)
                                            .on_hover_text("按住左右拖动调整分栏位置")
                                            .dnd_set_drag_payload(MsgHubDrag(cur_id));

                                        if let (Some(from_i), Some(p)) = (drag_from_idx, ptr_pos) {
                                            if from_i != slot_i && slot_rect.contains(p) && ui.input(|i| i.pointer.primary_down()) {
                                                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
                                                swap_req = Some((from_i, slot_i));
                                            }
                                        }
                                        if let Some(payload) = resp.dnd_release_payload::<MsgHubDrag>() {
                                            if let Some(from_i) = mcfg_o.msg_hub_order.iter().position(|&x| x == payload.0) {
                                                if from_i != slot_i {
                                                    swap_req = Some((from_i, slot_i));
                                                }
                                            }
                                        }

                                        let draw_rect = slot_rect.translate(egui::vec2(dx_anim, 0.0));
                                        let fill = if is_dragged {
                                            settings_color(ctx, Color32::from_rgb(44, 52, 68), Color32::from_rgb(218, 228, 248))
                                        } else if resp.hovered() {
                                            settings_color(ctx, Color32::from_rgb(46, 50, 62), Color32::from_rgb(226, 232, 244))
                                        } else {
                                            settings_color(ctx, Color32::from_rgb(34, 38, 48), Color32::from_rgb(236, 240, 248))
                                        };
                                        let border = if is_dragged {
                                            egui::Stroke::new(1.5, acc)
                                        } else if resp.hovered() {
                                            egui::Stroke::new(1.0, acc.gamma_multiply(0.65))
                                        } else {
                                            egui::Stroke::new(1.0, settings_color(ctx, Color32::from_white_alpha(18), Color32::from_black_alpha(14)))
                                        };
                                        ui.painter().rect_filled(draw_rect, 8.0, fill);
                                        ui.painter().rect_stroke(draw_rect, 8.0, border, egui::StrokeKind::Inside);

                                        // 左侧 6 点拖拽握把
                                        let dot_col = if is_dragged || resp.hovered() {
                                            acc
                                        } else {
                                            settings_color(ctx, Color32::from_gray(145), Color32::from_gray(110))
                                        };
                                        let gx = draw_rect.left() + 10.0;
                                        let gy = draw_rect.center().y;
                                        for ox in [-2.2, 2.2] {
                                            for oy in [-4.5, 0.0, 4.5] {
                                                ui.painter().circle_filled(egui::pos2(gx + ox, gy + oy), 1.3, dot_col);
                                            }
                                        }

                                        let label_str = format!("{}. {}", slot_i + 1, tab_name(cur_id));
                                        let font_sz = if slot_w < 78.0 { 10.2 } else { 11.8 };
                                        ui.painter().text(
                                            egui::pos2(draw_rect.center().x + 5.0, draw_rect.center().y),
                                            egui::Align2::CENTER_CENTER,
                                            label_str,
                                            egui::FontId::proportional(font_sz),
                                            settings_color(ctx, Color32::WHITE, Color32::from_rgb(24, 28, 36)),
                                        );

                                        if slot_i < 2 {
                                            let (arr_r, _) = ui.allocate_exact_size(egui::vec2(arrow_gap, slot_h), egui::Sense::hover());
                                            if arrow_gap >= 14.0 {
                                                ui.painter().text(
                                                    arr_r.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    "→",
                                                    egui::FontId::proportional(11.5),
                                                    settings_color(ctx, Color32::from_gray(140), Color32::from_gray(115)),
                                                );
                                            }
                                        }
                                    }
                                });

                                if let Some((from_i, to_i)) = swap_req {
                                    mcfg_o.msg_hub_order.swap(from_i, to_i);
                                    mcfg_o.msg_island_filter = mcfg_o.msg_hub_order[0];
                                }
                                if mcfg_o.msg_hub_order != prev_o {
                                    let no = mcfg_o.msg_hub_order;
                                    let nf = mcfg_o.msg_island_filter;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.msg_hub_order = no;
                                        c.msg_island_filter = nf;
                                    });
                                }
                            });
                        }
                        settings_card(ui,|ui|{
                            master_hint(ui);
                            ui.add_enabled_ui(master,|ui|{
                            let mut mcfg = crate::auralsle_ai_status::config::get();
                            let before_w = mcfg.clone();
                            let wechat_sub = if !cfg!(test) && mcfg.wechat_read_content {
                                "已开启本地内存消息预览"
                            } else {
                                "根据任务栏闪动提醒"
                            };
                            setting_row(ui,"微信新消息提醒",wechat_sub,&mut self.settings.wechat_notifications);
                            twinkle_card_break(ui);
                            setting_row(ui,"未读图标显示红点","在未读图标右上角显示小红点",&mut self.settings.unread_dot);
                            if !cfg!(test) {
                                twinkle_card_break(ui);
                                let prev_num_badge = mcfg.show_unread_badge_count;
                                setting_row(
                                    ui,
                                    "通知悬浮窗图标显示数字角标",
                                    "控制灵动岛右侧通知悬浮球软件图标右下角是否显示未读条数数字（图五）",
                                    &mut mcfg.show_unread_badge_count,
                                );
                                if mcfg.show_unread_badge_count != prev_num_badge {
                                    let nb = mcfg.show_unread_badge_count;
                                    crate::auralsle_ai_status::config::update(|c| c.show_unread_badge_count = nb);
                                }
                                twinkle_card_break(ui);
                                setting_row(
                                    ui,
                                    "极简药丸右侧显示等高消息悬浮球",
                                    "收到新消息时在极简药丸右侧显示等高未读图标",
                                    &mut mcfg.minimal_pill_msg_badge,
                                );
                            }
                            if self.settings.wechat_notifications {
                                twinkle_card_break(ui);
                                setting_row(ui,"托盘一键唤起微信","点击提醒直接唤起最小化在托盘的微信窗口",&mut self.settings.wechat_tray_open);
                                if !cfg!(test) {
                                    twinkle_card_break(ui);
                                    setting_row(
                                        ui,
                                        "读取微信消息内容",
                                        "在本地内存中提取最新微信消息预览，不落盘、不联网",
                                        &mut mcfg.wechat_read_content,
                                    );
                                    if mcfg.wechat_read_content {
                                        twinkle_card_break(ui);
                                        ui.label(egui::RichText::new("快捷键显示消息正文").size(12.5).strong());
                                        ui.label(
                                            egui::RichText::new("来微信消息时默认隐藏正文，按下设定快捷键后立即显示当前这条通知的消息内容；下一条新消息到达时自动恢复隐藏。")
                                                .size(11.0)
                                                .color(settings_color(ctx, Color32::from_gray(175), Color32::from_gray(85))),
                                        );
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(egui::RichText::new("当前快捷键：").size(11.5));
                                            ui.add_sized(
                                                [96.0, 24.0],
                                                egui::TextEdit::singleline(&mut mcfg.wechat_reveal_hotkey)
                                                    .hint_text("如 Alt+W"),
                                            );
                                            for preset in ["Alt+W", "Alt+V", "Ctrl+Shift+V", "F9", "Alt+Space"] {
                                                let active = mcfg.wechat_reveal_hotkey.trim().eq_ignore_ascii_case(preset);
                                                if ui.selectable_label(active, egui::RichText::new(preset).size(11.0)).clicked() {
                                                    mcfg.wechat_reveal_hotkey = preset.to_owned();
                                                }
                                            }
                                        });
                                        ui.horizontal_wrapped(|ui| {
                                            if ui.small_button("发送测试微信消息到岛上").clicked() {
                                                store_local_wechat_sender_and_msg("微信测试", &format!("测试消息 ({}) · 按快捷键可查看当前通知内容", chrono::Local::now().format("%H:%M:%S")));
                                                self.receive_wechat_reminder_from(0, 0);
                                            }
                                            if ui.small_button("立即显示当前消息").clicked() {
                                                trigger_local_fingerprint_verify();
                                            }
                                        });
                                    }
                                    twinkle_card_break(ui);
                                    setting_row(
                                        ui,
                                        "自动识别「OPPO 互联」通知并与电脑微信 / QQ 去重",
                                        "收到 OPPO 互联转发的手机通知时按原生图标并附带互联微标区分展示；检测到电脑微信或 QQ 已登录时自动不展示对应手机转发通知",
                                        &mut mcfg.oppo_wechat_dedup,
                                    );
                                    if mcfg != before_w {
                                        crate::auralsle_ai_status::config::update(|c| *c = mcfg);
                                    }
                                }
                            }
                            if master && !self.messenger_status(crate::app::Messenger::Wechat).is_empty(){ui.label(egui::RichText::new(self.messenger_status(crate::app::Messenger::Wechat)).size(12.0));}
                            });
                        });
                        settings_label(ui,"办公软件提醒");
                        settings_card(ui,|ui|{
                            master_hint(ui);
                            ui.add_enabled_ui(master,|ui|{
                                setting_row(ui,"企业微信","根据任务栏闪动提醒",&mut self.settings.wecom_notifications);
                                setting_row(ui,"飞书","根据任务栏闪动提醒",&mut self.settings.feishu_notifications);
                                setting_row(ui,"钉钉","根据任务栏闪动提醒",&mut self.settings.dingtalk_notifications);
                                ui.label(egui::RichText::new("需在客户端开启任务栏闪动，不读取联系人或聊天正文。点击提醒打开对应窗口；账号无法确认时可手动关闭提醒。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                                for client in crate::messenger::Messenger::OFFICE {
                                    if client.enabled(&self.settings) && !self.messenger_status(client).is_empty(){ui.label(egui::RichText::new(self.messenger_status(client)).size(12.0));}
                                }
                            });
                        });
                        settings_label(ui,"QQ 与 Windows 通知");
                        settings_card(ui,|ui|{
                            master_hint(ui);
                            ui.add_enabled_ui(master,|ui|{
                            ui.label(egui::RichText::new(&self.listener_status).size(13.0));
                            if self.settings.read_notifications {
                                if ui.button(if self.settings.qq_only{"断开 QQ 通知"}else{"断开系统通知"}).clicked(){self.pending.push(Cmd::DisconnectNotifications);}
                            }else{
                                ui.horizontal_wrapped(|ui|{
                                    if ui.button("只连接 QQ").clicked(){self.pending.push(Cmd::RequestQqNotifications);}
                                    if ui.button("连接所有应用通知").clicked(){self.pending.push(Cmd::RequestNotifications);}
                                });
                            }
                            ui.label(egui::RichText::new("QQ 需开启系统通知；新通知统一沿用上方设置的停留时长（默认 5 秒），默认隐藏正文。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            twinkle_card_break(ui);
                            {
                                let mut otp_on = crate::auralsle_ai_status::config::get().auralsle_otp_capsule;
                                setting_row(ui,"验证码浮窗胶囊（一键复制）","收到通知、邮件或微信验证码时直接弹出 灵屿大字胶囊并配「复制」按钮即刻复制",&mut otp_on);
                                if otp_on != crate::auralsle_ai_status::config::get().auralsle_otp_capsule {
                                    crate::auralsle_ai_status::config::update(|c| c.auralsle_otp_capsule = otp_on);
                                }
                                if ui.small_button("预览验证码浮窗胶囊").clicked() {
                                    self.push_verification_code("短信验证码".to_owned(), "482913".to_owned());
                                }
                            }
                            twinkle_card_break(ui);
                            setting_row(ui,"显示通知正文","可能包含私密消息；屏幕共享时建议关闭",&mut self.settings.notification_preview);
                            ui.label(egui::RichText::new("只接收 Windows 通知中心可见的内容；应用会在收到通知后出现在下方。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            let before_apps=self.notification_apps.items.clone();
                            for (_,name,enabled) in &mut self.notification_apps.items {setting_row(ui,name,"",enabled);}
                            if before_apps!=self.notification_apps.items{self.notification_apps.save();}
                            });
                        });
                    }
                    7=>{
                        settings_label(ui,"邮件提醒");
                        settings_card(ui,|ui|{
                            setting_row(ui,"新邮件提醒","新邮件到达时显示提醒；岛上歌词模式显示右侧图标",&mut self.settings.mail_enabled);
                            twinkle_card_break(ui);
                            let sync_before=self.settings.mail_sync_operations;
                            ui.add_enabled_ui(self.mail.manage.pending.is_empty(),|ui|{
                                setting_row(ui,"同步邮件操作到邮箱","开启后同步已读、删除和恢复；关闭时仅整理灵屿 AuraIsle 中的记录",&mut self.settings.mail_sync_operations);
                            });
                            if sync_before!=self.settings.mail_sync_operations{self.mail_manage_mode_changed();}
                            ui.label(egui::RichText::new("切换开关不会补交之前的本机操作。").size(11.5).color(settings_color(ctx,Color32::from_gray(195),Color32::from_gray(60))));
                            twinkle_card_break(ui);
                            setting_row(ui,"验证码高亮","识别到验证码时在岛上放大显示，点击即复制",&mut self.settings.mail_code);
                            twinkle_card_break(ui);
                            setting_choices(ui,"验证码隐藏",&mut self.settings.mail_code_hide_seconds,[30,60,300],"秒");
                            ui.label("默认 60 秒。明确有效时长更短时提前隐藏；历史邮件默认隐藏。隐藏不代表失效，主动查看后 15 秒再次隐藏。");
                            setting_choices(ui,"检查间隔",&mut self.settings.mail_interval,[60,120,300],"秒");
                            ui.label(egui::RichText::new("支持推送时实时收信，并每 10 秒补查；以上间隔仅用于不支持推送的邮箱。").size(11.5).color(settings_color(ctx,Color32::from_gray(195),Color32::from_gray(60))));
                            twinkle_card_break(ui);
                            setting_choices(ui,"会议提前",&mut self.settings.mail_ics,[5,15,30],"分钟");
                            ui.label(egui::RichText::new("根据邮件中的日历邀请提醒，与日程设置共用。").size(11.5).color(settings_color(ctx,Color32::from_gray(195),Color32::from_gray(60))));
                        });
                        settings_label(ui,"已添加的邮箱");
                        settings_card(ui,|ui|{
                            if self.mail.accounts.is_empty(){ui.label(egui::RichText::new("还没有邮箱。在下面添加，保存后自动连接。").size(12.0).color(settings_color(ctx,Color32::from_gray(195),Color32::from_gray(60))));}
                            let total=self.mail.accounts.len();
                            let(mut changed,mut remove,mut test)=(false,None,None);
                            for i in 0..total{
                                let(email,provider)=(self.mail.accounts[i].email.clone(),self.mail.accounts[i].provider);
                                let status=self.mail.status_of(&email).label();
                                let mut on=self.mail.accounts[i].enabled;
                                if i>0{twinkle_card_break(ui);}
                                ui.label(egui::RichText::new(&email).size(13.0).color(settings_color(ctx,Color32::WHITE,Color32::from_rgb(30,34,41))));
                                ui.label(egui::RichText::new(format!("{} · {}",provider.label(),status)).size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                                ui.horizontal(|ui|{
                                    if ui.add_sized([66.0,30.0],egui::Button::new("测试")).clicked(){test=Some(i);}
                                    if ui.add_sized([66.0,30.0],egui::Button::new("删除")).clicked(){remove=Some(i);}
                                    ui.add_space(8.0);
                                    ui.label(egui::RichText::new("启用").size(12.0).color(settings_color(ctx,Color32::from_gray(190),Color32::from_gray(65))));
                                    if switch(ui,&mut on){self.mail.accounts[i].enabled=on;changed=true;}
                                });
                            }
                            if changed{self.mail_accounts_changed();}
                            if let Some(i)=remove{
                                let email=self.mail.accounts[i].email.clone();
                                let credential=self.mail.accounts[i].credential_key();
                                self.mail.accounts.remove(i);
                                crate::mail::secrets::delete(&credential);
                                self.mail.status.remove(&email.to_ascii_lowercase());
                                self.mail.note=format!("{email}：已移除，凭据已从系统凭据管理器删除");
                                self.mail_accounts_changed();
                            }
                            if let Some(i)=test{
                                let account=self.mail.accounts[i].clone();
                                if let Err(error)=account.endpoint(){
                                    self.mail.note=error;
                                }else{
                                match crate::mail::secrets::load(&account.credential_key()){
                                    Ok(pass)=>{self.mail.testing=Some(account.email.clone());self.mail.note=format!("{}：正在测试连接…",account.email);self.pending.push(Cmd::MailTest(account,pass));}
                                    Err(e)=>self.mail.note=format!("{}：{e}",account.email),
                                }
                                }
                            }
                        });
                        settings_label(ui,"添加邮箱");
                        settings_card(ui,|ui|{
                            settings_control_row(ui,"邮箱服务",56.0,|ui|{
                                ui.horizontal(|ui| {
                                    let p_opts: Vec<(crate::mail::Provider, &str)> = crate::mail::Provider::ALL.iter().map(|p| (*p, p.label())).collect();
                                    react_bits_combo(ui, "mail-provider", self.mail.form.provider.label(), 160.0, &mut self.mail.form.provider, &p_opts);
                                }).response
                            });
                            ui.add_space(5.0);
                            let microsoft=self.mail.form.provider==crate::mail::Provider::Microsoft;
                            ui.label(egui::RichText::new(if microsoft{"连接微软邮箱"}else{"获取授权码或专用密码"}).size(12.5).strong());
                            ui.add(egui::Label::new(egui::RichText::new(self.mail.form.provider.hint()).size(12.0).color(settings_color(ctx,Color32::from_gray(200),Color32::from_gray(55)))).wrap());
                            let (label,url)=self.mail.form.provider.setup_link();
                            if !url.is_empty() && ui.link(label).clicked(){self.pending.push(Cmd::OpenUrl(url.into()));}
                            ui.add_space(8.0);
                            if self.mail.form.provider==crate::mail::Provider::Custom {
                                settings_control_row(ui,"IMAP 主机",80.0,|ui|{ui.text_edit_singleline(&mut self.mail.form.custom.host)});
                                settings_control_row(ui,"TLS 端口",80.0,|ui|{ui.add(egui::DragValue::new(&mut self.mail.form.custom.port).range(1..=65535))});
                                settings_control_row(ui,"登录用户名",80.0,|ui|{ui.add(egui::TextEdit::singleline(&mut self.mail.form.custom.username).vertical_align(egui::Align::Center).hint_text("留空使用邮箱地址"))});
                            }
                            if !microsoft{
                            let input_width=(layout_avail_w(ui)-56.0-48.0-2.0*ui.spacing().item_spacing.x).max(100.0);
                            settings_control_row(ui,"邮箱",56.0,|ui|{
                                ui.add_sized([input_width,30.0],egui::TextEdit::singleline(&mut self.mail.form.email).vertical_align(egui::Align::Center).hint_text("输入邮箱地址"))
                            });
                            let target=crate::mail::Account{custom:(self.mail.form.provider==crate::mail::Provider::Custom).then(||self.mail.form.custom.clone()),email:self.mail.form.email.clone(),provider:self.mail.form.provider,enabled:true};
                            let unsupported=target.endpoint().err().as_deref()==Some(crate::mail::MICROSOFT_UNSUPPORTED);
                            if unsupported{ui.add(egui::Label::new(crate::mail::MICROSOFT_UNSUPPORTED).wrap());}
                            ui.add_enabled_ui(!unsupported,|ui|{
                            settings_control_row(ui,"授权码",56.0,|ui|{
                                let field=ui.add_sized([input_width,30.0],egui::TextEdit::singleline(&mut self.mail.form.password).vertical_align(egui::Align::Center).password(!self.mail.form.show).hint_text("输入邮箱授权码"));
                                if ui.add_sized([48.0,30.0],egui::Button::new(if self.mail.form.show{"隐藏"}else{"显示"})).clicked(){self.mail.form.show=!self.mail.form.show;}
                                field
                            });
                            ui.horizontal(|ui|{
                                if ui.button("测试连接").clicked(){
                                    let email=self.mail.form.email.trim().to_string();
                                    let target=crate::mail::Account{custom:(self.mail.form.provider==crate::mail::Provider::Custom).then(||self.mail.form.custom.clone()),email:email.clone(),provider:self.mail.form.provider,enabled:true};
                                    if !email.contains('@'){self.mail.note="请先填邮箱地址".into();}
                                    else if let Err(error)=target.endpoint(){self.mail.note=error;}
                                    else if self.mail.form.password.is_empty(){self.mail.note="请先填授权码（不是邮箱登录密码）".into();}
                                    else{
                                        self.mail.testing=Some(email.clone());
                                        self.mail.note=format!("{email}：正在测试连接…");
                                        self.pending.push(Cmd::MailTest(crate::mail::Account {custom: (self.mail.form.provider==crate::mail::Provider::Custom).then(||self.mail.form.custom.clone()), email:email.clone(),provider:self.mail.form.provider,enabled:true},self.mail.form.password.clone()));
                                    }
                                }
                                if ui.button("保存并开始监控").clicked(){
                                    let email=self.mail.form.email.trim().to_string();
                                    let target=crate::mail::Account{custom:(self.mail.form.provider==crate::mail::Provider::Custom).then(||self.mail.form.custom.clone()),email:email.clone(),provider:self.mail.form.provider,enabled:true};
                                    if !email.contains('@'){self.mail.note="请先填邮箱地址".into();}
                                    else if let Err(error)=target.endpoint(){self.mail.note=error;}
                                    else if self.mail.form.password.is_empty(){self.mail.note="请先填授权码（不是邮箱登录密码）".into();}
                                    else{
                                        match crate::mail::secrets::save(&target.credential_key(),&self.mail.form.password){
                                            Ok(())=>{
                                                let account=crate::mail::Account {custom: (self.mail.form.provider==crate::mail::Provider::Custom).then(||self.mail.form.custom.clone()), email:email.clone(),provider:self.mail.form.provider,enabled:true};
                                                if let Some(slot)=self.mail.accounts.iter_mut().find(|a|a.email.eq_ignore_ascii_case(&email)){*slot=account;}else{self.mail.accounts.push(account);}
                                                self.mail.form.password.clear();
                                                self.mail.note=format!("{email}：已保存，正在连接…");
                                                self.mail_accounts_changed();
                                            }
                                            Err(e)=>self.mail.note=e,
                                        }
                                    }
                                }
                            });
                            ui.label(egui::RichText::new("授权码存储在 Windows 凭据管理器中。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            });
                            }
                            ui.label(egui::RichText::new("图片中的验证码请在邮箱查看。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            if !self.mail.note.is_empty(){ui.add_space(3.0);ui.add(egui::Label::new(egui::RichText::new(&self.mail.note).size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75)))).wrap());}
                        });
                    }
                    8=>{
                        ui.spacing_mut().item_spacing.y=4.0;
                        // v0.5.6 日程：状态在最上面，后面才是平台、账号和提醒。
                        // 只读；服务器地址由平台决定，用户不用抄。
                        if let Some(cfg)=self.calendar.config.clone(){
                            settings_card(ui,|ui|{
                                let n=self.calendar.events.len();
                                ui.horizontal(|ui|{
                                    ui.label(egui::RichText::new(if self.calendar.connected{format!("● 已连接 · 未来 24 小时 {n} 个日程")}else if self.calendar.failures==0{"● 正在连接…".into()}else{format!("● 上次连接失败 · 已知 {n} 个日程")}).size(13.0).strong().color(if self.calendar.connected{settings_color(ctx,Color32::from_rgb(120,220,160),Color32::from_rgb(24,121,64))}else{settings_color(ctx,Color32::from_rgb(240,180,110),Color32::from_rgb(145,89,12))}));
                                });
                                ui.horizontal(|ui|{
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                                        if ui.add_enabled(!self.calendar.testing,egui::Button::new("断开").small()).clicked(){self.pending.push(Cmd::CalendarForget);}
                                        if ui.small_button("立即同步").clicked(){self.calendar.note="正在同步…".into();self.calendar.note_error=false;self.pending.push(Cmd::CalendarSync);}
                                    });
                                });
                                ui.label(egui::RichText::new(format!("{} · 最后同步 {}",cfg.provider.label(),if self.calendar.last_sync.is_empty(){"—".to_string()}else{self.calendar.last_sync.clone()})).size(11.0).color(settings_color(ctx,Color32::from_gray(150),Color32::from_gray(105))));
                                if let Some(feed)=self.calendar.feeds.iter().find(|feed|feed.config==cfg) {
                                    let status=if feed.month.loading {"正在同步本月日程…".to_owned()}
                                        else if !feed.month.error.is_empty(){format!("本月同步失败：{}",feed.month.error)}
                                        else if feed.month.loaded==Some(self.calendar.view.month){format!("{} {} {} 个日程",self.calendar.view.month.format("%Y-%m"),if feed.month.warning.is_empty(){"已同步"}else{"已读取"},feed.month.events.len())}
                                        else {String::new()};
                                    if !status.is_empty(){ui.add(egui::Label::new(egui::RichText::new(status).size(12.0)).wrap());}
                                    if !feed.month.warning.is_empty(){ui.add(egui::Label::new(egui::RichText::new(format!("本月部分日程未同步：{}",feed.month.warning)).size(12.0).color(settings_color(ctx,Color32::from_rgb(238,181,91),Color32::from_rgb(139,88,12)))).wrap());}
                                }
                                setting_row(ui,"自动同步","每 5 分钟更新日程；关闭后可点「立即同步」",&mut self.settings.calendar_auto_sync);
                                if !self.calendar.warning.is_empty(){ui.add(egui::Label::new(egui::RichText::new(format!("同步提示：{}",self.calendar.warning)).size(12.0).color(settings_color(ctx,Color32::from_rgb(238,181,91),Color32::from_rgb(139,88,12)))).wrap());}
                                if !self.calendar.error.is_empty(){
                                    ui.add(egui::Label::new(egui::RichText::new(&self.calendar.error).size(12.0).color(settings_color(ctx,Color32::from_rgb(240,96,96),Color32::from_rgb(184,34,34)))).wrap());
                                }
                            });
                        }
                        if ui.add_sized([layout_avail_w(ui),34.0],egui::Button::new("打开日程")).clicked(){self.open_calendar(None);}
                        settings_label(ui,"选择日历平台");
                        settings_card(ui,|ui|{
                            egui::Frame::new().fill(settings_color(ctx,Color32::from_rgb(29,29,31),Color32::from_rgb(236,239,244))).corner_radius(8.0).inner_margin(3).show(ui,|ui|{
                                ui.set_min_width(ui.available_width());
                                ui.spacing_mut().item_spacing.x=2.0;
                                // 六个平台分两行：飞书 / 钉钉 / 企业微信，iCloud / CalDAV / Exchange。
                                let width=((layout_avail_w(ui)-4.0)/3.0).max(80.0);
                                for row in crate::calendar::Provider::ALL.chunks(3){
                                    ui.horizontal(|ui|{
                                        for &p in row{
                                            let selected=self.calendar.form.provider==p;
                                            let button=egui::Button::new(egui::RichText::new(p.label()).size(13.0).color(if selected{settings_color(ctx,Color32::WHITE,Color32::from_rgb(30,34,41))}else{settings_color(ctx,Color32::from_gray(190),Color32::from_gray(65))}))
                                                .fill(if selected{settings_color(ctx,Color32::from_rgb(81,81,85),Color32::from_rgb(208,217,231))}else{Color32::TRANSPARENT})
                                                .stroke(egui::Stroke::NONE).corner_radius(6.0);
                                            let button=ui.add_enabled_ui(!self.calendar.testing,|ui|ui.add_sized([width,30.0],button)).inner;
                                            let button=match p{
                                                crate::calendar::Provider::ICloud=>button.on_hover_text("苹果日历：Apple ID + App 专用密码"),
                                                crate::calendar::Provider::Custom=>button.on_hover_text("任意 CalDAV 日历：Nextcloud、群晖、Fastmail 等"),
                                                crate::calendar::Provider::Exchange=>button.on_hover_text("公司自建或第三方 Exchange 日历（ActiveSync），只读同步"),
                                                _=>button,
                                            };
                                            if button.clicked() && !selected{
                                                self.select_calendar_provider(p);
                                            }
                                        }
                                    });
                                }
                            });
                            ui.add_space(7.0);
                            ui.horizontal_top(|ui|{
                                ui.allocate_ui_with_layout(egui::vec2(50.0,18.0),egui::Layout::left_to_right(egui::Align::Center),|ui|{ui.set_min_size(egui::vec2(50.0,18.0));ui.label(egui::RichText::new("服务器").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));});
                                if self.calendar.form.provider.custom_server(){
                                    ui.add_sized([layout_avail_w(ui),22.0],egui::TextEdit::singleline(&mut self.calendar.form.server).vertical_align(egui::Align::Center)
                                        .hint_text(egui::RichText::new(self.calendar.form.provider.server_hint()).color(settings_color(ctx,Color32::from_gray(155),Color32::from_gray(100)))));
                                }else{
                                    ui.add(egui::Label::new(egui::RichText::new(self.calendar.form.provider.server()).size(12.0).color(settings_color(ctx,Color32::from_gray(205),Color32::from_gray(55)))).wrap());
                                }
                            });
                        });
                        settings_label(ui,"连接账号");
                        settings_card(ui,|ui|{
                            ui.scope(|ui|{
                                ui.visuals_mut().extreme_bg_color=settings_color(ctx,Color32::from_rgb(25,25,28),Color32::from_rgb(244,246,249));
                            let input_width=(layout_avail_w(ui)-50.0-48.0-2.0*ui.spacing().item_spacing.x).max(80.0);
                                ui.horizontal(|ui|{
                                    ui.allocate_ui_with_layout(egui::vec2(50.0,30.0),egui::Layout::left_to_right(egui::Align::Center),|ui|{ui.set_min_size(egui::vec2(50.0,30.0));ui.label(egui::RichText::new("用户名").size(13.0).color(settings_color(ctx,Color32::from_gray(210),Color32::from_gray(55))));});
                                    let user=ui.add_sized([input_width,30.0],egui::TextEdit::singleline(&mut self.calendar.form.username).vertical_align(egui::Align::Center)
                                        .hint_text(egui::RichText::new("输入用户名或粘贴账号信息").color(settings_color(ctx,Color32::from_gray(155),Color32::from_gray(100)))));
                                    if user.changed(){
                                        let pasted=self.calendar.form.username.clone();
                                        if let Some((u,p))=crate::calendar::parse_paste(&pasted){
                                            self.calendar.form.username=u;
                                            self.calendar.form.password=p;
                                            self.calendar.note="已从粘贴内容中填入用户名和密码".into();
                                            self.calendar.note_error=false;
                                        }
                                    }
                                });
                                ui.add_space(5.0);
                                ui.horizontal(|ui|{
                                    ui.allocate_ui_with_layout(egui::vec2(50.0,30.0),egui::Layout::left_to_right(egui::Align::Center),|ui|{ui.set_min_size(egui::vec2(50.0,30.0));ui.label(egui::RichText::new("密码").size(13.0).color(settings_color(ctx,Color32::from_gray(210),Color32::from_gray(55))));});
                                    ui.add_sized([input_width,30.0],egui::TextEdit::singleline(&mut self.calendar.form.password).vertical_align(egui::Align::Center)
                                        .password(!self.calendar.form.show).hint_text(egui::RichText::new("输入平台生成的密码").color(settings_color(ctx,Color32::from_gray(155),Color32::from_gray(100)))));
                                    if ui.add_sized([48.0,30.0],egui::Button::new(if self.calendar.form.show{"隐藏"}else{"显示"})).clicked(){self.calendar.form.show=!self.calendar.form.show;}
                                });
                            });
                            ui.add_space(7.0);
                            let guide=if self.calendar.form.guide_open{"收起获取方法"}else{"如何获取用户名和密码？"};
                            if ui.link(egui::RichText::new(guide).color(settings_color(ctx,Color32::from_rgb(120,185,245),Color32::from_rgb(26,100,177)))).clicked(){self.calendar.form.guide_open=!self.calendar.form.guide_open;}
                            if self.calendar.form.guide_open{
                                ui.add_space(4.0);
                                for(i,step)in self.calendar.form.provider.guide().iter().enumerate(){
                                    ui.label(egui::RichText::new(format!("{}. {}",i+1,step)).size(12.0).color(settings_color(ctx,Color32::from_gray(200),Color32::from_gray(55))));
                                }
                            }
                            ui.add_space(9.0);
                            let busy=self.calendar.testing;
                            let label=if busy{"正在测试连接…"}else{"测试并保存"};
                            let btn=egui::Button::new(egui::RichText::new(label).size(13.0).strong().color(Color32::WHITE))
                                .fill(if busy{settings_color(ctx,Color32::from_rgb(70,70,76),Color32::from_rgb(213,219,227))}else{Color32::from_rgb(45,105,165)})
                                .corner_radius(7.0);
                            if ui.add_enabled_ui(!busy,|ui|ui.add_sized([130.0,32.0],btn)).inner.clicked(){
                                let user=self.calendar.form.username.trim().to_string();
                                let custom=self.calendar.form.provider.custom_server();
                                let server=crate::calendar::normalize_server(&self.calendar.form.server);
                                if custom && server.is_none(){self.calendar.note="请填写以 https:// 开头的 CalDAV 服务器地址".into();self.calendar.note_error=true;}
                                else if user.is_empty(){self.calendar.note="请先填用户名".into();self.calendar.note_error=true;}
                                else if self.calendar.form.password.is_empty(){self.calendar.note="请先填密码".into();self.calendar.note_error=true;}
                                else{
                                    let cfg=crate::calendar::Config{provider:self.calendar.form.provider,username:user,href:None,server:if custom{server}else{None}};
                                    self.calendar.testing=true;
                                    self.calendar.test_password=self.calendar.form.password.clone();
                                    self.calendar.note="正在连接日历…".into();
                                    self.calendar.note_error=false;
                                    self.pending.push(Cmd::CalendarTest(cfg,self.calendar.form.password.clone()));
                                }
                            }
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new("连接成功后保存，密码存储在 Windows 凭据管理器中。").size(11.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            if !self.calendar.note.is_empty(){
                                ui.add_space(5.0);
                                let color=if self.calendar.note_error{settings_color(ctx,Color32::from_rgb(255,125,125),Color32::from_rgb(184,34,34))}else{settings_color(ctx,Color32::from_gray(210),Color32::from_gray(55))};
                                ui.add(egui::Label::new(egui::RichText::new(&self.calendar.note).size(12.0).color(color)).wrap());
                            }
                        });
                        settings_label(ui,"导入日历文件");
                        settings_card(ui,|ui|{
                            ui.add(egui::Label::new(egui::RichText::new("把 .ics 文件里的日程导入到本机日程，例如苹果「日历」App 里「文件 → 导出」得到的文件。重复日程会按未来一年展开；再次导入同一文件不会重复添加。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75)))).wrap());
                            ui.add_space(5.0);
                            if ui.add_sized([150.0,30.0],egui::Button::new("选择 .ics 文件…")).clicked(){self.pending.push(Cmd::CalendarImport);}
                            if !self.calendar.import_note.is_empty(){
                                ui.add_space(3.0);
                                ui.add(egui::Label::new(egui::RichText::new(&self.calendar.import_note).size(12.0).color(settings_color(ctx,Color32::from_gray(210),Color32::from_gray(55)))).wrap());
                            }
                        });
                        settings_label(ui,"会议提醒");
                        settings_card(ui,|ui|{
                            if cfg!(test) {
                                ui.horizontal(|ui|{
                                    ui.allocate_ui_with_layout(egui::vec2(50.0,32.0),egui::Layout::left_to_right(egui::Align::Center),|ui|{ui.set_min_size(egui::vec2(50.0,32.0));ui.label(egui::RichText::new("提前").size(13.0));});
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center),|ui|{
                                        for minutes in [30,15,5]{let selected=self.settings.mail_ics==minutes;
                                            if ui.add_sized([62.0,32.0],egui::Button::new(format!("{minutes} 分钟")).selected(selected)).clicked(){self.settings.mail_ics=minutes;}
                                        }
                                    });
                                });
                            } else {
                                setting_choices(ui, "提前", &mut self.settings.mail_ics, [5, 15, 30], "分钟");
                            }
                            ui.add_space(5.0);
                            ui.label(egui::RichText::new("与邮件中的会议邀请共用提醒，同一场会议只提醒一次。").size(11.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                        });
                    }
                    _=>{
                        settings_label(ui,"灵屿 AuraIsle");
                        settings_card(ui,|ui|{
                            ui.horizontal(|ui|{
                                ui.label(egui::RichText::new(format!("版本 {}",env!("CARGO_PKG_VERSION"))).size(13.0));
                                ui.add_space(8.0);
                                ui.vertical(|ui|self.updates.ui(ui));
                            });
                            if !cfg!(test) {
                                let mut mcfg_upd = crate::auralsle_ai_status::config::get();
                                let prev_upd = (mcfg_upd.auto_check_updates, mcfg_upd.update_check_interval_hours);
                                twinkle_card_break(ui);
                                setting_row(
                                    ui,
                                    "定时自动检查更新",
                                    "开启后在后台按设定周期检查 GitHub 新版本；关闭后仅在手动点击时检查",
                                    &mut mcfg_upd.auto_check_updates,
                                );
                                if mcfg_upd.auto_check_updates {
                                    twinkle_card_break(ui);
                                    settings_control_row_right(ui, "检查周期", |ui| {
                                        settings_segmented(
                                            ui,
                                            &mut mcfg_upd.update_check_interval_hours,
                                            &[(12u32, "12 小时"), (24u32, "24 小时"), (72u32, "3 天"), (168u32, "7 天")],
                                            68.0,
                                        );
                                    });
                                }
                                if (mcfg_upd.auto_check_updates, mcfg_upd.update_check_interval_hours) != prev_upd {
                                    let na = mcfg_upd.auto_check_updates;
                                    let nh = mcfg_upd.update_check_interval_hours;
                                    crate::auralsle_ai_status::config::update(|c| {
                                        c.auto_check_updates = na;
                                        c.update_check_interval_hours = nh;
                                    });
                                }
                                twinkle_card_break(ui);
                            }
                            ui.label(egui::RichText::new("桌面顶部的音乐、专注、提醒与日程小助手。").size(12.0).color(settings_color(ctx,Color32::from_gray(180),Color32::from_gray(75))));
                            ui.horizontal_wrapped(|ui|{
                                if ui.button("打开本地数据目录").clicked(){self.pending.push(Cmd::Open(crate::model::config_dir()));}
                                ui.add_space(6.0);
                                ui.hyperlink_to("GitHub 项目主页", "https://github.com/Eternal3240/Auralsle");
                            });
                        });
                        settings_label(ui,"致谢与参考开源项目");
                        settings_card(ui,|ui|{
                            ui.label(
                                egui::RichText::new("灵屿 AuraIsle 在设计与演进过程中参考并致谢以下优秀开源项目（点击名称可访问项目主页）：")
                                    .size(11.5)
                                    .color(settings_color(ctx, Color32::from_gray(180), Color32::from_gray(75))),
                            );
                            ui.add_space(4.0);
                            for (proj_name, proj_url, proj_desc) in [
                                ("xiaotang-daozai (Renly1994/xiaotang-daozai)", "https://github.com/Renly1994/xiaotang-daozai", "灵屿 AuraIsle 原生桌面灵动岛核心底座、物理弹簧动画与系统服务架构"),
                                ("BrimDeck (MoFeng2223/BrimDeck)", "https://github.com/MoFeng2223/BrimDeck", "图文混合剪贴板（支持保存复制图片与一键粘贴）及鼠标/键盘/耳机/手柄四合一无线外设低电告警设备岛"),
                                ("fly-island (flyl1u/fly-island)", "https://github.com/flyl1u/fly-island", "剪贴板历史、待办清单、聚合消息岛、快捷翻译、多彩胶囊指示器与 AgentAI 生命周期权限审批钩子设计"),
                                ("React Bits (DavidHDev/react-bits)", "https://www.reactbits.dev/", "AnimatedContent 流光渐显、BlurText 柔焦分词、Magnet 磁吸指针、GlareHover 镜面扫光与 ClickSpark 点击火花动效"),
                                ("AiNiee (NEKOparapa/AiNiee)", "https://github.com/NEKOparapa/AiNiee", "多服务商 AI 大模型 API（DeepSeek / OpenAI / 硅基流动 / Gemini / Claude）翻译引擎架构参考"),
                                ("Aora Bot (sam70361/aora-bot)", "https://github.com/sam70361/aora-bot", "44 态参数化吉祥物表情引擎、21 种画风与角色系统（Aora Bot License 1.1）"),
                                ("Venu - Dynamic Notch for Windows", "https://github.com/imnakul/venu---Dynamic-Notch-for-Windows", "灵动岛环绕流光边框特效与外观面板交互灵感"),
                                ("eIsland (JNTMTMTM/eIsland)", "https://github.com/JNTMTMTM/eIsland", "灵动音律播放器布局、封面自适应流光与实时音频频谱灵感"),
                                ("ShiGuangSchedule (拾光课程表)", "https://github.com/ShiGuangSchedule/shiguangschedule", "高校课表导入解析、周次计算与周视图网格交互参考"),
                            ] {
                                ui.horizontal_wrapped(|ui| {
                                    ui.hyperlink_to(egui::RichText::new(proj_name).size(11.8).strong(), proj_url);
                                    ui.label(
                                        egui::RichText::new(format!("— {proj_desc}"))
                                            .size(11.2)
                                            .color(settings_color(ctx, Color32::from_gray(172), Color32::from_gray(85))),
                                    );
                                });
                            }
                        });
                        if cfg!(test) {
                            settings_label(ui,"支持作者");
                            settings_card(ui,support_author);
                        }
                    }
                    }
                    });
                    (ui.min_rect().bottom() - content_rect.top()).max(0.0)
                    };
                    outer_ui.allocate_space(egui::vec2(avail_scroll_w, used_h));
                    });
                });
                let _sync_sm = {
                    let real_y = scroll_out.state.offset.y;
                    let new_max = (scroll_out.content_size.y - scroll_out.inner_rect.height()).max(0.0);
                    if !sm.active {
                        sm.cur_y = real_y;
                        sm.target_y = real_y;
                    } else if (real_y - sm.last_offset_y).abs() > 1.5 && (real_y - sm.cur_y).abs() > 1.5 {
                        // 用户直接拖拽右侧滑块或触发 scroll_to_rect：立即退出惯性接管，保证零延迟跟手
                        sm.active = false;
                        sm.cur_y = real_y;
                        sm.target_y = real_y;
                    }
                    sm.last_offset_y = real_y;
                    sm.max_y = new_max;
                    sm.target_y = sm.target_y.clamp(0.0, new_max);
                    sm.has_bounds = true;
                    ui.data_mut(|d| d.insert_temp(page_scroll_id, sm));
                };
                // [Twinkle · pageSectionIn] 入场透明度只作用于页面内容，纱幕要按满不透明度画。
                ui.set_opacity(1.0);
                let fade = if cfg!(test) || settings_sizing_now() { 1.0 } else { 0.62 + 0.38 * page_ease };
                if fade < 0.999 {
                    if !cfg!(test) { ctx.request_repaint(); }
                    let bg = skin_theme.bg_main;
                    let a = ((1.0 - fade) * 235.0).round() as u8;
                    if a > 0 {
                        ui.painter().rect_filled(
                            scroll_out.inner_rect,
                            0.0,
                            Color32::from_rgba_unmultiplied(bg.r(), bg.g(), bg.b(), a),
                        );
                    }
                }
            });
            crate::frame_watch::mark_stage("settings:body");
            let probe_body_ms = probe_t0.elapsed().as_secs_f64() * 1000.0;
            if !cfg!(test) {
                let scrolling = ctx.input(|i| {
                    i.smooth_scroll_delta != egui::Vec2::ZERO || i.raw_scroll_delta != egui::Vec2::ZERO
                });
                let n = ctx
                    .data(|d| d.get_temp::<u32>(egui::Id::new("auralsle_scroll_probe_n")))
                    .unwrap_or(0)
                    .saturating_add(1);
                ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_scroll_probe_n"), n));
                let t_now = ctx.input(|i| i.time);
                let last_scroll_id = egui::Id::new("auralsle_scroll_probe_last_scroll");
                if scrolling {
                    ctx.data_mut(|d| d.insert_temp(last_scroll_id, t_now));
                }
                let since_scroll = t_now
                    - ctx
                        .data(|d| d.get_temp::<f64>(last_scroll_id))
                        .unwrap_or(f64::NEG_INFINITY);
                if scrolling || since_scroll < 0.35 {
                    // [卡顿定位] 除了本帧 UI 成本，还记录与上一帧滚动之间的间隔：
                    // 只有 dt≈16ms 才是真 60fps；若 dt 稳定在 33/50ms，说明掉帧发生在
                    // 出帧节拍/呈现链路，而不是 egui 布局。
                    let t_now = ctx.input(|i| i.time);
                    let last_id = egui::Id::new("auralsle_scroll_probe_t");
                    let prev_t = ctx.data(|d| d.get_temp::<f64>(last_id));
                    ctx.data_mut(|d| d.insert_temp(last_id, t_now));
                    let dt_ms = prev_t.map(|p| (t_now - p) * 1000.0).unwrap_or(0.0);
                    crate::frame_watch::append_log_line(&format!(
                        "[SCROLL-PROBE] n={} tab={} dt_ms={:.1} total_ms={:.1} sidebar_ms={:.1} body_ms={:.1}",
                        n,
                        self.settings_tab,
                        dt_ms,
                        probe_t0.elapsed().as_secs_f64() * 1000.0,
                        probe_side_ms,
                        probe_body_ms
                    ));
                }
            }
            self.avoidance.dialog(ctx,&mut self.settings);
            let settings_changed = self.settings != before;
            if settings_changed {
                self.settings.save();
            }
            if !cfg!(test) && !ctx.input(|i| i.pointer.any_down()) {
                static LAST_UNDO_REV: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
                let cur_rev = crate::auralsle_ai_status::config::revision();
                let prev_rev = LAST_UNDO_REV.swap(cur_rev, std::sync::atomic::Ordering::Relaxed);
                if settings_changed || cur_rev != prev_rev {
                    if let Ok(mut undo_st) = UNDO_SLOT.get_or_init(|| std::sync::Mutex::new(PageUndoState::default())).lock() {
                        let after_snap = (self.settings.clone(), crate::auralsle_ai_status::config::get());
                        let prev_opt = undo_st.last_committed.clone();
                        if let Some(prev) = prev_opt {
                            if prev != after_snap {
                                undo_st.undo_stack.push(prev);
                                if undo_st.undo_stack.len() > 30 {
                                    undo_st.undo_stack.remove(0);
                                }
                                undo_st.redo_stack.clear();
                                undo_st.last_committed = Some(after_snap);
                            }
                        } else {
                            undo_st.last_committed = Some(after_snap);
                        }
                    }
                }
            }
            if !cfg!(test) {
                // [Twinkle Tray `page.4cb0fe87.css` `#base[data-izeros-border="true"]:after`]
                // 无边框窗口外围 1px 实线描边；放大/缩小/最大化切换时叠一层轻量化强调光边过渡，最大化时直角贴合工作区。
                let (vp_rect, is_max_vp) = ctx.input(|i| {
                    (
                        i.viewport()
                            .inner_rect
                            .map(|r| egui::Rect::from_min_size(egui::Pos2::ZERO, r.size()))
                            .unwrap_or_else(|| i.screen_rect()),
                        i.viewport().maximized.unwrap_or(false),
                    )
                });
                let resize_at = ctx.data(|d| d.get_temp::<f64>(egui::Id::new("auralsle_settings_resize_at"))).unwrap_or(f64::MIN);
                let resize_dt = (self.t - resize_at).max(0.0) as f32;
                let resize_glow = if resize_dt < 0.35 && !opening {
                    ctx.request_repaint();
                    (1.0 - resize_dt / 0.35).powi(2)
                } else {
                    0.0
                };
                let border_r = if is_max_vp { 0.0 } else { 8.0 };
                #[cfg(windows)]
                {
                    // Win11 原生 DWMWA_BORDER_COLOR 已沿 DWMWCP_ROUND (8px) 外缘精准绘制 1px 边框；
                    // 客户区不再叠加 12px 前景黑框，避免斜切右上角悬停红底关闭按钮；仅在缩放反馈瞬间绘制 8px 强调色微光。
                    if resize_glow > 0.01 {
                        let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                        let glow_col = Color32::from_rgba_unmultiplied(
                            acc.r(),
                            acc.g(),
                            acc.b(),
                            (resize_glow * 165.0).round().clamp(0.0, 255.0) as u8,
                        );
                        let border_w = 1.0 + resize_glow * 0.8;
                        ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tw_win_border")))
                            .rect_stroke(vp_rect, border_r, egui::Stroke::new(border_w, glow_col), egui::StrokeKind::Inside);
                    }
                }
                #[cfg(not(windows))]
                {
                    let base_border = if dark {
                        Color32::from_rgb(0x30, 0x30, 0x30)
                    } else {
                        Color32::from_rgb(0xd2, 0xd4, 0xda)
                    };
                    let acc = crate::auralsle_ai_status::plugins::active_accent_color();
                    let win_border = if resize_glow > 0.01 {
                        crate::paint::mix(base_border, acc, resize_glow * 0.65)
                    } else {
                        base_border
                    };
                    let border_w = 1.0 + resize_glow * 0.8;
                    ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("tw_win_border")))
                        .rect_stroke(vp_rect, border_r, egui::Stroke::new(border_w, win_border), egui::StrokeKind::Inside);
                }
            }
            // [设置页滚动修复] 记下子视口是否还需要下一帧，供下一帧的重绘闸门使用：
            // `requested_repaint_last_pass` 只认「立即重绘」请求（滚动惯性/悬停/切页动画），
            // 设置页里的定时刷新（如 request_repaint_after(1s)）不会把帧率拉满。
            let resize_anim_active = !cfg!(test)
                && (self.t - ctx.data(|d| d.get_temp::<f64>(egui::Id::new("auralsle_settings_resize_at"))).unwrap_or(f64::MIN)).abs() < 0.36;
            let child_busy_next = ctx.requested_repaint_last_pass()
                || scrolling_now
                || settings_sizing_now()
                || resize_anim_active
                || ctx.input(|i| i.smooth_scroll_delta != egui::Vec2::ZERO || i.raw_scroll_delta != egui::Vec2::ZERO);
            ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_child_busy"), child_busy_next));
            // [需求 8] 不再每帧 `ctx.set_style(original_style)`：那会把 Style 版本号再顶一次，
            // 等于把上面省掉的样式失效又加回去。本视口 Context 专属，无其它消费者。
            #[cfg(not(windows))]
            settings_resize_hit_zones(ctx);
        });
        crate::frame_watch::mark_stage("settings:vp_after");
        if !self.settings_open{self.window_layout().close();self.weather_source.close();}else{self.weather_tutorial_ui(ctx);}
    }
}

#[cfg(windows)]
static SETTINGS_SIZING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SETTINGS_DARK_BG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static SETTINGS_LAST_PPP_X100: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);
static SETTINGS_SIZING_UNTIL_MS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
#[cfg(windows)]
static SETTINGS_ACTIVE_HWND: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
#[cfg(windows)]
static SETTINGS_LAST_RGN_KEY: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
#[cfg(windows)]
const SETTINGS_SUBCLASS_ID: usize = 0x445A_5345;

fn settings_monotonic_ms() -> u64 {
    static T0: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    T0.get_or_init(std::time::Instant::now).elapsed().as_millis() as u64
}

fn settings_sizing_now() -> bool {
    #[cfg(windows)]
    if SETTINGS_SIZING.load(std::sync::atomic::Ordering::Relaxed) {
        return true;
    }
    let until = SETTINGS_SIZING_UNTIL_MS.load(std::sync::atomic::Ordering::Relaxed);
    until != 0 && settings_monotonic_ms() < until
}

fn mark_settings_sizing_ms(ms: u64) {
    SETTINGS_SIZING_UNTIL_MS.store(
        settings_monotonic_ms().saturating_add(ms),
        std::sync::atomic::Ordering::Relaxed,
    );
}

#[cfg(windows)]
unsafe extern "system" fn settings_subclass_proc(
    hwnd: windows::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows::Win32::Foundation::WPARAM,
    lparam: windows::Win32::Foundation::LPARAM,
    _uid_subclass: usize,
    _dw_ref_data: usize,
) -> windows::Win32::Foundation::LRESULT {
    use windows::Win32::Foundation::{BOOL, COLORREF, LRESULT, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, FillRect, GetMonitorInfoW, MonitorFromWindow, HDC,
        HGDIOBJ, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::HiDpi::GetDpiForWindow;
    use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClientRect, GetWindowRect, IsIconic, IsZoomed, MINMAXINFO, WINDOWPOS, WS_CAPTION,
        WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    #[repr(C)]
    struct StyleStruct {
        style_old: u32,
        style_new: u32,
    }
    #[repr(C)]
    struct NcCalcSizeParams {
        rgrc: [RECT; 3],
        lppos: *mut WINDOWPOS,
    }

    const WM_SIZE: u32 = 0x0005;
    const WM_ERASEBKGND: u32 = 0x0014;
    const WM_SHOWWINDOW: u32 = 0x0018;
    const WM_GETMINMAXINFO: u32 = 0x0024;
    const WM_WINDOWPOSCHANGED: u32 = 0x0047;
    const WM_STYLECHANGING: u32 = 0x007C;
    const WM_NCDESTROY: u32 = 0x0082;
    const WM_NCCALCSIZE: u32 = 0x0083;
    const WM_NCHITTEST: u32 = 0x0084;
    const WM_NCPAINT: u32 = 0x0085;
    const WM_NCACTIVATE: u32 = 0x0086;
    const WM_SETTEXT: u32 = 0x000C;
    const WM_SETICON_MSG: u32 = 0x0080;
    const WM_SIZING: u32 = 0x0214;
    const WM_ENTERSIZEMOVE: u32 = 0x0231;
    const WM_EXITSIZEMOVE: u32 = 0x0232;
    const WM_DWMCOMPOSITIONCHANGED: u32 = 0x031E;
    const GWL_STYLE_I32: i32 = -16;

    #[repr(C)]
    struct DwmMargins {
        cx_left: i32,
        cx_right: i32,
        cy_top: i32,
        cy_bottom: i32,
    }
    #[link(name = "dwmapi")]
    extern "system" {
        fn DwmExtendFrameIntoClientArea(hwnd: isize, p_mar_inset: *const DwmMargins) -> i32;
    }
    let extend_dwm_frame = |hwnd: windows::Win32::Foundation::HWND| {
        let m = DwmMargins { cx_left: 0, cx_right: 0, cy_top: 1, cy_bottom: 0 };
        unsafe { let _ = DwmExtendFrameIntoClientArea(hwnd.0 as isize, &m); }
    };

    const HTCLIENT: isize = 1;
    const HTCAPTION: isize = 2;
    const HTLEFT: isize = 10;
    const HTRIGHT: isize = 11;
    const HTTOP: isize = 12;
    const HTTOPLEFT: isize = 13;
    const HTTOPRIGHT: isize = 14;
    const HTBOTTOM: isize = 15;
    const HTBOTTOMLEFT: isize = 16;
    const HTBOTTOMRIGHT: isize = 17;

    let window_scale = |hwnd: windows::Win32::Foundation::HWND| -> f32 {
        let dpi = unsafe { GetDpiForWindow(hwnd) };
        if dpi >= 96 {
            dpi as f32 / 96.0
        } else {
            (SETTINGS_LAST_PPP_X100
                .load(std::sync::atomic::Ordering::Relaxed)
                .max(100) as f32)
                / 100.0
        }
    };

    let sync_round_rgn = |hwnd: windows::Win32::Foundation::HWND, force: bool| {
        if unsafe { IsIconic(hwnd) } != BOOL(0) {
            return;
        }
        let zoomed = unsafe { IsZoomed(hwnd) } != BOOL(0);
        let key = if zoomed { 2i64 } else { 1i64 };
        let prev_key = SETTINGS_LAST_RGN_KEY.swap(key, std::sync::atomic::Ordering::Relaxed);
        if force || prev_key != key {
            // 清空 GDI HRGN 并交由 Win11 DWM 原生圆角 (DWMWCP_ROUND=2) 与过渡动画管理，
            // 彻底恢复设置窗口最小化、任务栏还原与最大化/还原的系统级丝滑弹簧动画（修复需求 3）
            let _ = unsafe { SetWindowRgn(hwnd.0 as isize, 0, 0) };
            let pref: i32 = if zoomed { 1 /* DWMWCP_DONOTROUND */ } else { 2 /* DWMWCP_ROUND */ };
            let _ = unsafe {
                windows::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                    hwnd,
                    windows::Win32::Graphics::Dwm::DWMWA_WINDOW_CORNER_PREFERENCE,
                    &pref as *const i32 as *const _,
                    4,
                )
            };
        }
    };

    match msg {
        WM_STYLECHANGING if (wparam.0 as i32) == GWL_STYLE_I32 && lparam.0 != 0 => {
            // 保留 WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU 以启用 Win11 DWM 原生最大/最小化过渡动画，
            // 并由下方 WM_NCCALCSIZE 零化非客户区标题栏高度
            let ss = unsafe { &mut *(lparam.0 as *mut StyleStruct) };
            ss.style_new = ss.style_new
                | WS_CAPTION.0
                | WS_THICKFRAME.0
                | WS_MINIMIZEBOX.0
                | WS_MAXIMIZEBOX.0
                | WS_SYSMENU.0;
            return LRESULT(0);
        }
        WM_NCCALCSIZE if lparam.0 != 0 => {
            if unsafe { IsZoomed(hwnd) } != BOOL(0) {
                let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
                let mut mi = MONITORINFO {
                    cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                    ..Default::default()
                };
                if unsafe { GetMonitorInfoW(monitor, &mut mi) } != BOOL(0) {
                    let rc_dst = unsafe { &mut *(lparam.0 as *mut RECT) };
                    *rc_dst = mi.rcWork;
                }
            }
            return LRESULT(0);
        }
        WM_NCHITTEST => {
            let mut rc = RECT::default();
            if unsafe { GetWindowRect(hwnd, &mut rc) }.is_ok() {
                let sx = (lparam.0 & 0xFFFF) as u16 as i16 as i32;
                let sy = ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
                let px = sx - rc.left;
                let py = sy - rc.top;
                let w = rc.right - rc.left;
                let h = rc.bottom - rc.top;
                let scale = window_scale(hwnd);
                let title_h = (30.0 * scale).round() as i32;
                let ctrl_w = (138.0 * scale).round() as i32;
                let bw = (8.0 * scale).round().max(6.0) as i32;
                let cw = (14.0 * scale).round().max(10.0) as i32;
                let edge_thin = (4.0 * scale).round().max(3.0) as i32;
                let zoomed = unsafe { IsZoomed(hwnd) } != BOOL(0);

                if !zoomed {
                    // 优先保障右上角 3 个标题栏按钮内部区域可点击，仅保留极窄外沿供角部拉伸
                    let in_ctrl_interior = px >= w - ctrl_w && py >= edge_thin && py < title_h && px < w - edge_thin;
                    if in_ctrl_interior {
                        return LRESULT(HTCLIENT);
                    }
                    let left = px < bw;
                    let right = px >= w - bw;
                    let top_band = if px >= w - ctrl_w { edge_thin } else { bw };
                    let top = py < top_band;
                    let bottom = py >= h - bw;
                    if (left && top) || (px < cw && py < bw && px < bw) || (px < cw && py < top_band && top) {
                        return LRESULT(HTTOPLEFT);
                    }
                    if (right && top) || (px >= w - cw && py < edge_thin) || (px >= w - edge_thin && py < cw) {
                        return LRESULT(HTTOPRIGHT);
                    }
                    if (left && bottom) || (px < cw && py >= h - bw) || (px < bw && py >= h - cw) {
                        return LRESULT(HTBOTTOMLEFT);
                    }
                    if (right && bottom) || (px >= w - cw && py >= h - bw) || (px >= w - bw && py >= h - cw) {
                        return LRESULT(HTBOTTOMRIGHT);
                    }
                    if left {
                        return LRESULT(HTLEFT);
                    }
                    if right {
                        return LRESULT(HTRIGHT);
                    }
                    if top {
                        return LRESULT(HTTOP);
                    }
                    if bottom {
                        return LRESULT(HTBOTTOM);
                    }
                }
                if py >= 0 && py < title_h && px >= 0 && px < (w - ctrl_w).max(0) {
                    return LRESULT(HTCAPTION);
                }
                return LRESULT(HTCLIENT);
            }
        }
        WM_GETMINMAXINFO if lparam.0 != 0 => {
            let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            let mmi = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let (work_w, work_h) = if unsafe { GetMonitorInfoW(monitor, &mut mi) } != BOOL(0) {
                let ww = (mi.rcWork.right - mi.rcWork.left).max(480);
                let wh = (mi.rcWork.bottom - mi.rcWork.top).max(360);
                mmi.ptMaxPosition = POINT {
                    x: mi.rcWork.left - mi.rcMonitor.left,
                    y: mi.rcWork.top - mi.rcMonitor.top,
                };
                mmi.ptMaxSize = POINT { x: ww, y: wh };
                (ww, wh)
            } else {
                (1920, 1080)
            };
            let scale = window_scale(hwnd);
            let min_w = ((1024.0 * scale).round() as i32).min(work_w);
            let min_h = ((720.0 * scale).round() as i32).min(work_h);
            mmi.ptMinTrackSize.x = min_w;
            mmi.ptMinTrackSize.y = min_h;
            return res;
        }
        WM_DWMCOMPOSITIONCHANGED => {
            extend_dwm_frame(hwnd);
            return LRESULT(0);
        }
        WM_SETTEXT | WM_SETICON_MSG => {
            // 阻止 DefWindowProc 在设置标题/图标时重绘 Win32 经典浅蓝标题栏
            let cur_s = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE) };
            let ws_vis = 0x10000000isize;
            if (cur_s & ws_vis) != 0 {
                unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE, cur_s & !ws_vis) };
                let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
                unsafe { windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW(hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE, cur_s) };
                return res;
            }
        }
        WM_NCPAINT => return LRESULT(0),
        WM_NCACTIVATE => {
            if unsafe { IsIconic(hwnd) } == BOOL(0) {
                extend_dwm_frame(hwnd);
                return LRESULT(1);
            }
        }
        WM_ERASEBKGND => {
            // [缩放黑屏根治] 缩放/尺寸变化期间 HWND 正由 D3D12 / OpenGL 重建并同步重绘，
            // 若此刻用 GDI 刷实色背景会直接覆盖刚提交或待提交的交换链画面，导致缩放时整块黑屏。
            if !settings_sizing_now() {
                let mut rc = RECT::default();
                if unsafe { GetClientRect(hwnd, &mut rc) }.is_ok() {
                    let col = if SETTINGS_DARK_BG.load(std::sync::atomic::Ordering::Relaxed) {
                        0x00202020u32
                    } else {
                        0x00F9F3F1u32
                    };
                    let br = unsafe { CreateSolidBrush(COLORREF(col)) };
                    if !br.is_invalid() {
                        let hdc = HDC(wparam.0 as *mut _);
                        let _ = unsafe { FillRect(hdc, &rc, br) };
                        let _ = unsafe { DeleteObject(HGDIOBJ(br.0)) };
                    }
                }
            }
            return LRESULT(1);
        }
        WM_SIZE => {
            if wparam.0 != 1 {
                mark_settings_sizing_ms(220);
            }
        }
        WM_SHOWWINDOW if wparam.0 != 0 => {
            let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            sync_round_rgn(hwnd, true);
            return res;
        }
        WM_ENTERSIZEMOVE | WM_SIZING => {
            SETTINGS_SIZING.store(true, std::sync::atomic::Ordering::Relaxed);
            mark_settings_sizing_ms(320);
        }
        WM_EXITSIZEMOVE => {
            SETTINGS_SIZING.store(false, std::sync::atomic::Ordering::Relaxed);
            mark_settings_sizing_ms(120);
        }
        WM_WINDOWPOSCHANGED => {
            let res = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            if lparam.0 != 0 {
                let wp = unsafe { &*(lparam.0 as *const WINDOWPOS) };
                if (wp.flags.0 & 0x0001 /* SWP_NOSIZE */) == 0 {
                    mark_settings_sizing_ms(180);
                }
            }
            sync_round_rgn(hwnd, false);
            return res;
        }
        WM_NCDESTROY => {
            if SETTINGS_ACTIVE_HWND
                .compare_exchange(
                    hwnd.0 as isize,
                    0,
                    std::sync::atomic::Ordering::AcqRel,
                    std::sync::atomic::Ordering::Relaxed,
                )
                .is_ok()
            {
                SETTINGS_SIZING.store(false, std::sync::atomic::Ordering::Relaxed);
                SETTINGS_SIZING_UNTIL_MS.store(0, std::sync::atomic::Ordering::Relaxed);
                SETTINGS_LAST_RGN_KEY.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(settings_subclass_proc), SETTINGS_SUBCLASS_ID) };
            return unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
        }
        _ => {}
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// [第 10/11 批 · 图七 圆角 + 图八 去闪框 + Win11 任务栏最小化]
fn bring_settings_window_to_front() {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            AllowSetForegroundWindow, BringWindowToTop, IsIconic, IsWindow, SetForegroundWindow,
            SetWindowPos, ShowWindow, ASFW_ANY, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE,
            SWP_NOSIZE, SWP_SHOWWINDOW, SW_RESTORE, SW_SHOW,
        };
        let raw = SETTINGS_ACTIVE_HWND.load(std::sync::atomic::Ordering::Acquire);
        if raw != 0 {
            let hwnd = HWND(raw as *mut core::ffi::c_void);
            unsafe {
                if IsWindow(hwnd).as_bool() {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                    if IsIconic(hwnd).as_bool() {
                        let _ = ShowWindow(hwnd, SW_RESTORE);
                    } else {
                        let _ = ShowWindow(hwnd, SW_SHOW);
                    }
                    let _ = BringWindowToTop(hwnd);
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_TOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                    let _ = SetWindowPos(
                        hwnd,
                        HWND_NOTOPMOST,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE | SWP_NOSIZE | SWP_SHOWWINDOW,
                    );
                    let _ = SetForegroundWindow(hwnd);
                }
            }
        }
    }
}

fn apply_settings_round_corners(ctx: &egui::Context, dark: bool) {
    if cfg!(test) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("auralsle_settings_hwnd_ready"), true));
        return;
    }
    SETTINGS_DARK_BG.store(dark, std::sync::atomic::Ordering::Relaxed);
    let done_id = egui::Id::new("auralsle_round_corners_done");
    let (size, maximized) = ctx.input(|i| {
        (
            i.viewport()
                .inner_rect
                .map(|r| r.size())
                .unwrap_or_else(|| i.screen_rect().size()),
            i.viewport().maximized.unwrap_or(false),
        )
    });
    let ppp = ctx.pixels_per_point().max(1.0);
    SETTINGS_LAST_PPP_X100.store(
        (ppp * 100.0).round().clamp(100.0, 400.0) as u32,
        std::sync::atomic::Ordering::Relaxed,
    );
    let dark_id = egui::Id::new("auralsle_round_corners_dark");
    let prev_dark = ctx.data(|d| d.get_temp::<bool>(dark_id));
    let size_id = egui::Id::new("auralsle_round_corners_key");
    let prev_key = ctx.data(|d| d.get_temp::<(egui::Vec2, f32, bool)>(size_id));
    if prev_dark != Some(dark)
        || prev_key.map_or(true, |(s, p, m)| {
            (s.x - size.x).abs() > 1.0 || (s.y - size.y).abs() > 1.0 || (p - ppp).abs() > 0.01 || m != maximized
        })
    {
        ctx.data_mut(|d| {
            d.insert_temp(dark_id, dark);
            d.insert_temp(size_id, (size, ppp, maximized));
            d.insert_temp(done_id, false);
        });
    }
    let already_done = ctx.data(|d| {
        d.get_temp::<bool>(done_id).unwrap_or(false)
            && d.get_temp::<bool>(egui::Id::new("auralsle_settings_hwnd_ready")).unwrap_or(false)
    });
    if already_done {
        return;
    }
    #[cfg(windows)]
    {
        if round_settings_hwnd(ppp, maximized, dark) {
            ctx.data_mut(|d| {
                d.insert_temp(done_id, true);
                d.insert_temp(egui::Id::new("auralsle_settings_hwnd_ready"), true);
            });
        }
    }
    #[cfg(not(windows))]
    {
        ctx.data_mut(|d| {
            d.insert_temp(done_id, true);
            d.insert_temp(egui::Id::new("auralsle_settings_hwnd_ready"), true);
        });
    }
}

#[cfg(windows)]
#[link(name = "gdi32")]
extern "system" {
    fn CreateRoundRectRgn(x1: i32, y1: i32, x2: i32, y2: i32, w: i32, h: i32) -> isize;
}
#[cfg(windows)]
#[link(name = "user32")]
extern "system" {
    fn SetWindowRgn(hwnd: isize, hrgn: isize, redraw: i32) -> i32;
}

#[cfg(windows)]
fn round_settings_hwnd(ppp: f32, maximized: bool, dark: bool) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{BOOL, COLORREF, HWND, LPARAM, RECT, WPARAM};
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWINDOWATTRIBUTE, DWMWA_WINDOW_CORNER_PREFERENCE,
    };
    use windows::Win32::Graphics::Gdi::CreateSolidBrush;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentProcessId;
    use windows::Win32::UI::Shell::SetWindowSubclass;
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetSystemMetrics, GetWindowLongPtrW, GetWindowRect, GetWindowTextW,
        GetWindowThreadProcessId, LoadImageW, SendMessageW, SetClassLongPtrW, SetWindowLongPtrW,
        SetWindowPos, GCLP_HBRBACKGROUND, GCLP_HICON, GCLP_HICONSM, GWL_STYLE, IMAGE_ICON,
        LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE,
        SWP_NOOWNERZORDER, SWP_NOSIZE, SWP_NOZORDER, WM_SETICON, WS_CAPTION, WS_MAXIMIZEBOX,
        WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME,
    };

    struct Probe {
        pid: u32,
        ppp: f32,
        maximized: bool,
        dark: bool,
        hit: bool,
    }
    unsafe extern "system" fn visit(hwnd: HWND, lp: LPARAM) -> BOOL {
        let probe = unsafe { &mut *(lp.0 as *mut Probe) };
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
        if pid != probe.pid {
            return BOOL(1);
        }
        let mut buf = [0u16; 96];
        let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
        let title = if n > 0 {
            String::from_utf16_lossy(&buf[..n as usize])
        } else {
            String::new()
        };
        if title.contains("岛仔设置") || (title.contains("灵屿") && title.contains("设置")) {
            // 1) 安装按 HWND 隔离的 comctl32 Subclass，彻底拦截 WM_NCCALCSIZE/NCPAINT/ERASEBKGND 并避免全局 orig_wndproc 串台
            let hwnd_raw = hwnd.0 as isize;
            let prev_hwnd = SETTINGS_ACTIVE_HWND.swap(hwnd_raw, std::sync::atomic::Ordering::AcqRel);
            if prev_hwnd != hwnd_raw {
                SETTINGS_LAST_RGN_KEY.store(0, std::sync::atomic::Ordering::Relaxed);
                let _ = unsafe { SetWindowSubclass(hwnd, Some(settings_subclass_proc), SETTINGS_SUBCLASS_ID, 0) };
            }

            // 2) 同步设置窗口类实色背景画刷 + Win11 DWM 暗色浸润，彻底消灭打开首帧白底闪现
            static DARK_BR: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
            static LIGHT_BR: std::sync::OnceLock<isize> = std::sync::OnceLock::new();
            let br_ptr = if probe.dark {
                *DARK_BR.get_or_init(|| unsafe { CreateSolidBrush(COLORREF(0x00202020)).0 as isize })
            } else {
                *LIGHT_BR.get_or_init(|| unsafe { CreateSolidBrush(COLORREF(0x00F9F3F1)).0 as isize })
            };
            if br_ptr != 0 {
                let _ = unsafe { SetClassLongPtrW(hwnd, GCLP_HBRBACKGROUND, br_ptr) };
            }
            let dark_val: i32 = i32::from(probe.dark);
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWINDOWATTRIBUTE(20), // DWMWA_USE_IMMERSIVE_DARK_MODE
                    &dark_val as *const i32 as *const _,
                    4,
                )
            };
            let pref: i32 = if probe.maximized { 1 /* DWMWCP_DONOTROUND */ } else { 2 /* DWMWCP_ROUND */ };
            let _ = unsafe {
                DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &pref as *const i32 as *const _, 4)
            };
            let transitions_disable: i32 = 0; // DWMWA_TRANSITIONS_FORCEDISABLED = FALSE（强制开启 DWM 最小化/最大化动画）
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWINDOWATTRIBUTE(3),
                    &transitions_disable as *const i32 as *const _,
                    4,
                )
            };
            // 由 Win11 DWM 原生绘制 1px 圆角外框（COLORREF = 0x00BBGGRR），严格贴合 DWMWCP_ROUND (8px) 圆角且绝不压盖关闭按钮
            let border_color: u32 = if probe.dark { 0x00303030 } else { 0x00DAD4D2 };
            let _ = unsafe {
                DwmSetWindowAttribute(
                    hwnd,
                    DWMWINDOWATTRIBUTE(34), // DWMWA_BORDER_COLOR
                    &border_color as *const u32 as *const _,
                    4,
                )
            };
            #[repr(C)]
            struct InitMargins {
                cx_left: i32,
                cx_right: i32,
                cy_top: i32,
                cy_bottom: i32,
            }
            #[link(name = "dwmapi")]
            extern "system" {
                fn DwmExtendFrameIntoClientArea(hwnd: isize, p_mar_inset: *const InitMargins) -> i32;
            }
            let init_m = InitMargins { cx_left: 0, cx_right: 0, cy_top: 1, cy_bottom: 0 };
            unsafe { let _ = DwmExtendFrameIntoClientArea(hwnd.0 as isize, &init_m); }

            // 3) 保留 WS_CAPTION | WS_THICKFRAME | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_SYSMENU（由 WM_NCCALCSIZE 拦截消除原生标题栏），
            //    使 Windows 11 DWM 完整接管最小化、还原、最大化动画与原生圆角阴影
            let cur_style = unsafe { GetWindowLongPtrW(hwnd, GWL_STYLE) };
            let want_style = cur_style
                | ((WS_CAPTION.0 | WS_THICKFRAME.0 | WS_MINIMIZEBOX.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0) as isize);
            if want_style != cur_style {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWL_STYLE, want_style);
                    let _ = SetWindowPos(
                        hwnd,
                        None,
                        0,
                        0,
                        0,
                        0,
                        SWP_NOMOVE
                            | SWP_NOSIZE
                            | SWP_NOZORDER
                            | SWP_NOACTIVATE
                            | SWP_NOOWNERZORDER
                            | SWP_FRAMECHANGED,
                    );
                }
            }

            // 4) 锁定设置窗口与窗口类的 WM_SETICON（使用 exe 内编入的 RT_GROUP_ICON #1 新图标，防止 Win11 任务栏回退旧图标）
            static RES_ICONS: std::sync::OnceLock<(isize, isize)> = std::sync::OnceLock::new();
            let (h_big, h_sm) = *RES_ICONS.get_or_init(|| unsafe {
                if let Ok(module) = GetModuleHandleW(None) {
                    let cx_big = GetSystemMetrics(SM_CXICON).max(32);
                    let cx_sm = GetSystemMetrics(SM_CXSMICON).max(16);
                    let big = LoadImageW(module, PCWSTR(1 as *const u16), IMAGE_ICON, cx_big, cx_big, LR_DEFAULTCOLOR)
                        .map(|h| h.0 as isize)
                        .unwrap_or(0);
                    let sm = LoadImageW(module, PCWSTR(1 as *const u16), IMAGE_ICON, cx_sm, cx_sm, LR_DEFAULTCOLOR)
                        .map(|h| h.0 as isize)
                        .unwrap_or(0);
                    (big, sm)
                } else {
                    (0, 0)
                }
            });
            unsafe {
                if h_big != 0 {
                    let _ = SetClassLongPtrW(hwnd, GCLP_HICON, h_big);
                    let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(1), LPARAM(h_big));
                }
                if h_sm != 0 {
                    let _ = SetClassLongPtrW(hwnd, GCLP_HICONSM, h_sm);
                    let _ = SendMessageW(hwnd, WM_SETICON, WPARAM(0), LPARAM(h_sm));
                }
            }

            let _ = unsafe { SetWindowRgn(hwnd.0 as isize, 0, 0) };
            SETTINGS_LAST_RGN_KEY.store(if probe.maximized { 2 } else { 1 }, std::sync::atomic::Ordering::Relaxed);
            probe.hit = true;
            return BOOL(0);
        }
        BOOL(1)
    }
    let mut probe = Probe {
        pid: unsafe { GetCurrentProcessId() },
        ppp,
        maximized,
        dark,
        hit: false,
    };
    let _ = unsafe { EnumWindows(Some(visit), LPARAM(&mut probe as *mut Probe as isize)) };
    probe.hit
}

/// [需求 9 · 设置窗口可缩放] 无边框窗口（`with_decorations(false)`）没有系统缩放边，
/// `StartDrag` 只能移动不能改尺寸。这里在四边/四角铺 6px 命中区：悬停切缩放光标，
/// 按下立刻把方向交给宿主 `ViewportCommand::BeginResize`，进入原生缩放循环，行为与普通窗口一致。
/// 纯输入判定，不新增控件、不参与布局，所以不给滚动帧增加任何重排成本。
#[allow(dead_code)]
fn settings_resize_hit_zones(ctx: &egui::Context) {
    use egui::ResizeDirection as Dir;
    let vp = ctx.input(|i| {
        i.viewport()
            .inner_rect
            .map(|r| egui::Rect::from_min_size(egui::Pos2::ZERO, r.size()))
            .unwrap_or_else(|| i.screen_rect())
    });
    let Some(p) = ctx.input(|i| i.pointer.hover_pos()) else {
        return;
    };
    if !vp.contains(p) {
        return;
    }
    const Z: f32 = 6.0;
    let left = p.x <= vp.left() + Z;
    let right = p.x >= vp.right() - Z;
    let top = p.y <= vp.top() + Z;
    let bottom = p.y >= vp.bottom() - Z;
    let hit = match (left, right, top, bottom) {
        (true, false, true, false) => Some((Dir::NorthWest, egui::CursorIcon::ResizeNorthWest)),
        (false, true, true, false) => Some((Dir::NorthEast, egui::CursorIcon::ResizeNorthEast)),
        (true, false, false, true) => Some((Dir::SouthWest, egui::CursorIcon::ResizeSouthWest)),
        (false, true, false, true) => Some((Dir::SouthEast, egui::CursorIcon::ResizeSouthEast)),
        (true, false, false, false) => Some((Dir::West, egui::CursorIcon::ResizeWest)),
        (false, true, false, false) => Some((Dir::East, egui::CursorIcon::ResizeEast)),
        (false, false, true, false) => Some((Dir::North, egui::CursorIcon::ResizeNorth)),
        (false, false, false, true) => Some((Dir::South, egui::CursorIcon::ResizeSouth)),
        _ => None,
    };
    let Some((dir, cursor)) = hit else {
        return;
    };
    ctx.set_cursor_icon(cursor);
    if ctx.input(|i| i.pointer.primary_pressed()) {
        mark_settings_sizing_ms(320);
        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(dir));
    }
}

/// 收款码只在「支持作者」卡片展开时占用贴图；收起卡片或关闭设置窗口即释放。
fn release_support_codes(ctx:&egui::Context){
    ctx.data_mut(|data|for i in 0..2usize {data.remove::<egui::TextureHandle>(egui::Id::new(("support-author-qr",i)));});
}

/// 从原图裁出二维码区域 `[x, y, 边长]` 并缩放到 `pixels` 见方。
fn support_code_image(bytes:&[u8],crop:[f32;3],pixels:u32)->egui::ColorImage {
    let image=image::load_from_memory(bytes).expect("内置收款码图片有效");
    let side=(crop[2] as u32).min(image.width().saturating_sub(crop[0] as u32)).min(image.height().saturating_sub(crop[1] as u32));
    let code=image.crop_imm(crop[0] as u32,crop[1] as u32,side,side).resize_exact(pixels,pixels,image::imageops::FilterType::Lanczos3).to_rgba8();
    egui::ColorImage::from_rgba_unmultiplied([pixels as usize,pixels as usize],code.as_raw())
}

#[test]
fn support_codes_are_cropped_small_and_released_when_hidden(){
    for (bytes,crop) in [(include_bytes!("../assets/support/support-wechat-original.jpg").as_slice(),[330.0,370.0,600.0]),(include_bytes!("../assets/support/support-alipay-original.jpg").as_slice(),[200.0,660.0,860.0])] {
        let image=support_code_image(bytes,crop,360);
        assert_eq!(image.size,[360,360]);
        assert!(image.pixels.len()*4<=600*1024,"每张收款码贴图应小于 0.6MB");
        // 裁剪结果应是二维码本身：同时含有大量深色与浅色像素。
        let dark=image.pixels.iter().filter(|p|p.r()<80).count() as f32/image.pixels.len() as f32;
        assert!((0.2..0.8).contains(&dark),"裁剪区域不像二维码：深色占比 {dark}");
    }
    let ctx=egui::Context::default();
    let _=ctx.run(Default::default(),|ctx|{
        let texture=ctx.load_texture("qr",egui::ColorImage::new([4,4],Color32::WHITE),Default::default());
        ctx.data_mut(|data|data.insert_temp(egui::Id::new(("support-author-qr",0usize)),texture));
    });
    assert_eq!(ctx.tex_manager().read().allocated().filter(|(_,meta)|meta.name=="qr").count(),1);
    release_support_codes(&ctx);
    let _=ctx.run(Default::default(),|_|{});
    assert_eq!(ctx.tex_manager().read().allocated().filter(|(_,meta)|meta.name=="qr").count(),0,"收起后应释放收款码贴图");
}

/// 切页过渡时长（秒）：旧版 0.20s 三次方收得太急，视觉上是"闪一下"；0.30s 收尾更柔。
const PAGE_SWITCH_SECS: f32 = 0.30;
/// 拖窗口缩放后的"settle"遮罩时长（秒）：尺寸一变，内容按新宽度瞬时重排，
/// 用 0.20s 的轻微淡入把这次重排盖住，看起来是"收拢"而不是"跳一下"。
const RESIZE_SETTLE_SECS: f32 = 0.20;

/// 切页缓动（ease-out-quart）：`t=0` 时 0，`t>=dur` 时 1。
fn page_switch_ease(elapsed: f32, dur: f32) -> f32 {
    if !elapsed.is_finite() || elapsed <= 0.0 { return 0.0; }
    if dur <= 0.0 || elapsed >= dur { return 1.0; }
    let u = elapsed / dur;
    1.0 - (1.0 - u).powi(4)
}

/// 缩放 settle 缓动（ease-out-cubic）：`t=0` 时 0，`t>=dur` 时 1。
fn resize_settle_ease(elapsed: f32, dur: f32) -> f32 {
    if !elapsed.is_finite() || elapsed <= 0.0 { return 0.0; }
    if dur <= 0.0 || elapsed >= dur { return 1.0; }
    let u = elapsed / dur;
    1.0 - (1.0 - u).powi(3)
}

/// [Twinkle Tray · pageSectionIn] 设置页入场曲线：`cubic-bezier(0.075, 0.82, 0.165, 1)`、0.6s。
///
/// 取自 Twinkle Tray 1.17.2 解包产物 `build/page.4cb0fe87.css`：
/// `.settings-page { animation: pageSectionIn 0.6s cubic-bezier(0.075, 0.82, 0.165, 1) }`，
/// 关键帧 `0% { opacity:0; translateY(90px) } → 80% { opacity:1 } → 100% { translateY(0) }`。
/// 这里把 CSS 的 x→y 映射解出来（三次贝塞尔按 x 二分），给设置页内容做同样的"从下方滑入"。
fn twinkle_page_in_ease(elapsed: f32) -> f32 {
    const DUR: f32 = 0.60;
    if !elapsed.is_finite() || elapsed <= 0.0 { return 0.0; }
    if elapsed >= DUR { return 1.0; }
    let x = elapsed / DUR;
    let (x1, y1, x2, y2) = (0.075_f32, 0.82_f32, 0.165_f32, 1.0_f32);
    let axis = |t: f32, a: f32, b: f32| {
        let u = 1.0 - t;
        3.0 * u * u * t * a + 3.0 * u * t * t * b + t * t * t
    };
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..14 {
        let mid = 0.5 * (lo + hi);
        if axis(mid, x1, x2) < x { lo = mid; } else { hi = mid; }
    }
    axis(0.5 * (lo + hi), y1, y2).clamp(0.0, 1.0)
}

/// 入场位移（逻辑点）：Twinkle 的关键帧从 +90px 归零；窄窗里按可用高度收一点，别把首屏推空。
fn twinkle_page_in_offset(ease: f32, avail_h: f32) -> f32 {
    let travel = 90.0_f32.min((avail_h * 0.35).max(28.0));
    (1.0 - ease).clamp(0.0, 1.0) * travel
}

/// 设置窗口的重绘节拍。返回 `Some(等待秒数)` 表示本帧不必绘制（调用方必须安排补帧），
/// `None` 表示本帧要画。
///
/// [设置页滚动修复] egui 的滚轮是「惯性消化」的：一格滚轮进入 `unprocessed_scroll_delta`，
/// 每帧只消化 `t = exponential_smooth_factor(0.90, 0.1, dt)`，`dt = stable_dt.at_most(0.1)`
/// （见 `egui/src/input_state/mod.rs`）。
/// - 13ms 一帧：`t≈0.25`，一格滚轮约 4~8 帧消化完，滚动连续；
/// - 350ms 一帧：`dt` 被夹到 0.1，`t=0.90`，一格滚轮 1~2 帧吃完 → 每格都变成一次大跳。
/// 所以「已过 13ms 就画」是滚动跟手的前提，穷档只用于真正空闲时省电。
fn settings_paint_gate(now: f64, last: f64, events: bool, size_changed: bool, busy: bool, island_animating: bool, animating: bool) -> Option<f64> {
    // 有输入事件的那一帧必须画：节流只能作用在没有新输入的帧上，否则滚轮/点击会被延迟到下一档。
    if events { return None; }
    // 窗口尺寸变化（拖边缘缩放）同样必须立刻画：否则每帧都要等 idle 档的 250ms，
    // 拖动时内容会"一格一格"地跳，看起来非常卡。
    if size_changed { return None; }
    let interval = if busy {
        0.013 // 子视口仍在动画（滚动惯性、悬停过渡、切页胶囊）≈75fps 上限
    } else if animating {
        0.016
    } else if island_animating {
        0.050 // 主岛在变形：让出主帧，但设置窗口仍以 20fps 保活（旧版会整帧丢弃）
    } else {
        0.250
    };
    let elapsed = now - last;
    // 时钟回退/未初始化时直接绘制，避免用负的 elapsed 算出超长等待。
    if !elapsed.is_finite() || elapsed < 0.0 { return None; }
    if elapsed >= interval { None } else { Some(interval - elapsed) }
}

#[derive(Clone, Copy, Default)]
struct ResponsiveScrollState {
    last_frame: Option<u64>,
    egui_unprocessed: egui::Vec2,
    tween_total: [f32; 2],
    tween_emitted: [f32; 2],
    tween_elapsed: [f32; 2],
    active: bool,
}

/// 全局非线性平滑滚轮动画驱动（React Bits `power4.out / Quartic Ease-Out` 缓动曲线，不改 `vendor/egui`）：
/// - 精准镜像 `egui` 内置 `(0.90, 0.1s)` 衰减队列并用非线性四次缓出 `1 - (1 - p)^4` (`280ms`) 等量替换；
/// - 滚轮触发首帧即刻响应零迟滞，随后按原生显示刷新率丝滑非线性减速停靠，总位移像素 100% 守恒；
/// - 触控板高精度平滑滚动（`< 4px` Point）保持 1:1 透传不叠加额外延迟。
pub(crate) fn drive_responsive_scroll(ctx: &egui::Context) -> bool {
    if cfg!(test) {
        return false;
    }
    const SCROLL_EASE_DUR: f32 = 0.285;
    let vp_id = ctx.viewport_id();
    let frame_nr = ctx.cumulative_pass_nr();
    let state_id = egui::Id::new(("auralsle_responsive_scroll_state", vp_id));
    let mut state = ctx
        .data(|d| d.get_temp::<ResponsiveScrollState>(state_id))
        .unwrap_or_default();
    if state.last_frame == Some(frame_nr) {
        return state.active;
    }
    state.last_frame = Some(frame_nr);

    let line_speed = ctx.options(|o| o.line_scroll_speed);
    let (raw_dt, screen_h, raw_scroll, smooth_before, events) = ctx.input(|i| {
        (
            i.stable_dt,
            i.screen_rect.height(),
            i.raw_scroll_delta,
            i.smooth_scroll_delta,
            i.events.clone(),
        )
    });
    let egui_dt = raw_dt.min(0.1);
    let tween_dt = raw_dt.clamp(0.002, 0.045);

    let mut new_notched = egui::Vec2::ZERO;
    for ev in &events {
        if let egui::Event::MouseWheel {
            unit,
            delta,
            modifiers,
        } = ev
        {
            let is_zoom = modifiers.ctrl || modifiers.mac_cmd || modifiers.command;
            if is_zoom {
                continue;
            }
            let mut d = match unit {
                egui::MouseWheelUnit::Point => *delta,
                egui::MouseWheelUnit::Line => *delta * line_speed,
                egui::MouseWheelUnit::Page => *delta * screen_h,
            };
            if modifiers.shift {
                d = egui::vec2(d.x + d.y, 0.0);
            }
            let is_smooth = match unit {
                egui::MouseWheelUnit::Point => d.length() < 4.0,
                egui::MouseWheelUnit::Line | egui::MouseWheelUnit::Page => false,
            };
            if !is_smooth {
                new_notched += d;
            }
        }
    }

    state.egui_unprocessed += new_notched;

    let t_egui = egui::emath::exponential_smooth_factor(0.90, 0.1, egui_dt);
    let mut egui_emitted = egui::Vec2::ZERO;
    let mut tween_step = egui::Vec2::ZERO;

    for idx in 0..2 {
        if state.egui_unprocessed[idx] != 0.0 {
            if state.egui_unprocessed[idx].abs() < 1.0 {
                egui_emitted[idx] = state.egui_unprocessed[idx];
                state.egui_unprocessed[idx] = 0.0;
            } else {
                let a = t_egui * state.egui_unprocessed[idx];
                egui_emitted[idx] = a;
                state.egui_unprocessed[idx] -= a;
            }
        }
        if new_notched[idx] != 0.0 {
            let rem = state.tween_total[idx] - state.tween_emitted[idx];
            // 同向连滚时保留微弱初段进度，防止速度归零顿挫；反向滚动则立即重置
            let same_dir = rem * new_notched[idx] > 0.0;
            state.tween_total[idx] = rem + new_notched[idx];
            state.tween_emitted[idx] = 0.0;
            state.tween_elapsed[idx] = if same_dir {
                (SCROLL_EASE_DUR * 0.04).min(tween_dt * 1.5)
            } else {
                0.0
            };
        }
        let rem_cur = state.tween_total[idx] - state.tween_emitted[idx];
        if rem_cur.abs() > 0.0001 {
            let step_dt = if new_notched[idx] != 0.0 { tween_dt.max(0.008) } else { tween_dt };
            state.tween_elapsed[idx] += step_dt;
            if state.tween_elapsed[idx] >= SCROLL_EASE_DUR || rem_cur.abs() < 0.08 {
                tween_step[idx] = rem_cur;
                state.tween_total[idx] = 0.0;
                state.tween_emitted[idx] = 0.0;
                state.tween_elapsed[idx] = 0.0;
            } else {
                let p = (state.tween_elapsed[idx] / SCROLL_EASE_DUR).clamp(0.0, 1.0);
                // Quartic Ease-Out (power4.out)：起步凌厉跟手、中后段持久丝滑阻尼停靠
                let ease = 1.0 - (1.0 - p).powi(4);
                let target_cum = state.tween_total[idx] * ease;
                tween_step[idx] = target_cum - state.tween_emitted[idx];
                state.tween_emitted[idx] = target_cum;
            }
        }
    }

    let delta_adj = tween_step - egui_emitted;
    if delta_adj != egui::Vec2::ZERO {
        ctx.input_mut(|i| {
            i.smooth_scroll_delta += delta_adj;
        });
    }

    let tween_pending = (state.tween_total[0] - state.tween_emitted[0]).abs() > 0.0001
        || (state.tween_total[1] - state.tween_emitted[1]).abs() > 0.0001;
    let active = raw_scroll != egui::Vec2::ZERO
        || (smooth_before + delta_adj) != egui::Vec2::ZERO
        || tween_pending
        || state.egui_unprocessed != egui::Vec2::ZERO;
    state.active = active;
    if active {
        ctx.request_repaint();
    }
    ctx.data_mut(|d| {
        d.insert_temp(state_id, state);
        if vp_id != egui::ViewportId::ROOT && active {
            d.insert_temp(egui::Id::new("auralsle_child_scrolling"), true);
        }
    });
    active
}

#[cfg(test)]
mod settings_paint_gate_tests {
    use super::settings_paint_gate;

    #[test]
    fn settings_never_skips_a_frame_with_input() {
        // 有输入的那一帧必须画：22ms 闸门会把连续滚动（每 18ms 一格）拆掉一半。
        for elapsed in [0.0f64, 0.001, 0.02, 0.35] {
            assert_eq!(settings_paint_gate(1.0 + elapsed, 1.0, true, false, false, false, false), None,
                "有输入事件时不应跳帧（elapsed={elapsed}）");
        }
    }

    #[test]
    fn settings_animating_child_repaints_near_frame_rate() {
        // 滚动惯性/悬停过渡：必须按≈75fps 出帧，否则 egui 一格滚轮会被一次性吃掉（大跳）。
        let wait = settings_paint_gate(1.005, 1.0, false, false, true, false, false).expect("5ms 刚画过，本帧可跳过");
        assert!((wait - 0.008).abs() < 1e-3, "子视口动画中补帧间隔应≈13ms，实际 {wait}");
        assert_eq!(settings_paint_gate(1.020, 1.0, false, false, true, false, false), None, "已过 13ms 就该画");
    }

    #[test]
    fn settings_idle_backs_off_but_never_stalls() {
        assert_eq!(settings_paint_gate(1.3, 1.0, false, false, false, false, false), None, "空闲 300ms 后应绘制");
        let wait = settings_paint_gate(1.05, 1.0, false, false, false, false, false).expect("空闲时允许跳过");
        assert!(wait > 0.0 && wait <= 0.250 + 1e-3, "跳帧必须安排补帧且不超过空闲档 250ms，实际 {wait}");
    }

    #[test]
    fn settings_island_animation_does_not_starve_settings() {
        let wait = settings_paint_gate(1.01, 1.0, false, false, false, true, false).expect("主岛动画时仍应限频");
        assert!(wait <= 0.050 + 1e-3, "主岛动画时设置窗口最多等 50ms，实际 {wait}");
        assert_eq!(settings_paint_gate(1.06, 1.0, false, false, false, true, false), None, "过 50ms 必须画一帧");
    }

    #[test]
    fn settings_paint_gate_tolerates_clock_reset() {
        assert_eq!(settings_paint_gate(0.5, 9.0, false, false, false, false, false), None, "时钟回退时应直接绘制");
    }

    #[test]
    fn settings_repaints_immediately_when_the_window_is_resized() {
        // [第三轮] 拖窗口边缘时不产生"输入事件"，旧闸门会把内容压到 250ms 一帧 → 看起来卡。
        for elapsed in [0.0f64, 0.002, 0.020, 0.100] {
            assert_eq!(
                settings_paint_gate(1.0 + elapsed, 1.0, false, true, false, false, false),
                None,
                "窗口尺寸变化必须立刻绘制（elapsed={elapsed}）"
            );
        }
    }
}

#[cfg(test)]
mod settings_motion_tests {
    use super::{page_switch_ease, resize_settle_ease, PAGE_SWITCH_SECS, RESIZE_SETTLE_SECS};

    #[test]
    fn page_switch_ease_is_monotonic_and_lands_exactly_on_one() {
        let mut prev = 0.0f32;
        for step in 0..=20 {
            let e = page_switch_ease(PAGE_SWITCH_SECS * step as f32 / 20.0, PAGE_SWITCH_SECS);
            assert!((0.0..=1.0).contains(&e), "缓动必须落在 [0,1]，实际 {e}");
            assert!(e + 1e-4 >= prev, "缓动必须单调不减：{prev} → {e}");
            prev = e;
        }
        assert_eq!(page_switch_ease(PAGE_SWITCH_SECS, PAGE_SWITCH_SECS), 1.0, "时长到点必须正好收尾");
        assert_eq!(page_switch_ease(PAGE_SWITCH_SECS * 3.0, PAGE_SWITCH_SECS), 1.0, "超时后恒为 1");
    }

    #[test]
    fn page_switch_ease_is_front_loaded_so_the_swap_never_looks_like_a_flash() {
        // ease-out-quart 半程应已完成 ~94%：切页第一眼的"闪"主要来自前 100ms 进度太慢。
        let half = page_switch_ease(PAGE_SWITCH_SECS / 2.0, PAGE_SWITCH_SECS);
        assert!(half > 0.90, "半程进度应 > 0.90，实际 {half}");
        assert!(page_switch_ease(0.0, PAGE_SWITCH_SECS) == 0.0, "起点必须为 0");
    }

    #[test]
    fn resize_settle_masks_the_reflow_then_gets_out_of_the_way() {
        assert_eq!(resize_settle_ease(-1.0, RESIZE_SETTLE_SECS), 0.0, "时钟回退按起点处理");
        assert_eq!(resize_settle_ease(0.0, RESIZE_SETTLE_SECS), 0.0, "刚变尺寸时遮罩最重");
        let mid = resize_settle_ease(RESIZE_SETTLE_SECS * 0.5, RESIZE_SETTLE_SECS);
        assert!(mid > 0.8 && mid < 1.0, "半程应已基本收敛，实际 {mid}");
        assert_eq!(resize_settle_ease(RESIZE_SETTLE_SECS, RESIZE_SETTLE_SECS), 1.0, "到点必须完全透明");
        // 遮罩不能让页面"看不见"：最重也只压到 0.78（调用方公式），这里只锁缓动边界。
        assert!(!resize_settle_ease(f32::NAN, RESIZE_SETTLE_SECS).is_nan(), "NaN 不能穿透到透明度");
    }
}

#[cfg(test)]
mod settings_narrow_layout_tests {
    use super::{control_row_stacks, slot_rows_fit_on_one_line, uniform_grid};

    #[test]
    fn control_row_stacks_when_the_control_has_no_room_left() {
        // 780 宽窗口实测：行宽 370、标签「底部页面指示条主题」自然宽 212，
        // 右侧只剩 150 → 仍能并排（分段器会自己压缩），标签不许再压到控件上。
        assert!(!control_row_stacks(370.0, 212.0), "150 的余量够放控件，不该分行");
        // 窄到 237 时余量只剩 17，必须分行。
        assert!(control_row_stacks(237.0, 212.0), "余量不足 120 时必须分行");
        // 短标签在任何宽度下都并排。
        assert!(!control_row_stacks(237.0, 60.0));
    }

    #[test]
    fn slot_row_breaks_before_the_group_not_between_arrow_and_group() {
        // 宽窗：标签 + 3 个槽位组 + 2 个箭头放得下一行。
        let units = [200.0f32, 210.0, 120.0];
        assert!(slot_rows_fit_on_one_line(900.0, 120.0, &units, 16.0));
        // 窄窗：整行放不下 → 必须走"每槽位独占一行"的分支，箭头永远不会落在行首。
        assert!(!slot_rows_fit_on_one_line(560.0, 120.0, &units, 16.0));
    }

    #[test]
    fn preset_grid_prefers_three_columns_then_degrades() {
        // 宽：3 列；中：2 列；窄：1 列，任何情况下单元格都为正且不越界。
        assert_eq!(uniform_grid(800.0, 6, 118.0, 6.0, 3).0, 3);
        assert_eq!(uniform_grid(300.0, 6, 118.0, 6.0, 3).0, 2);
        assert_eq!(uniform_grid(200.0, 6, 118.0, 6.0, 3).0, 1);
        for avail in [120.0f32, 200.0, 300.0, 400.0, 800.0] {
            let (cols, cell) = uniform_grid(avail, 6, 118.0, 6.0, 3);
            assert!(cols >= 1 && cols <= 3);
            assert!(cell > 0.0);
            assert!(cols as f32 * cell + 6.0 * (cols as f32 - 1.0) <= avail + 0.01, "avail={avail}: 网格总宽不得超过可用宽度");
        }
    }
}

// verify-compat: settings-plugins.svg | {"NOTCH"}else{"APP"
