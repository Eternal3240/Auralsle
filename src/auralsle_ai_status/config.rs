//! AI 工作状态 mod 的配置。
//!
//! 独立落盘到 `%APPDATA%\Auralsle\ai-status-mod.json`，不写入上游 `settings.txt`，
//! 因此上游升级、改设置结构都不会覆盖本 mod 的配置。
//! 上游只依赖 `serde_json`（没有 `serde` derive），所以这里手工读写 `Value`。

use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

pub const CONFIG_FILE: &str = "ai-status-mod.json";
/// 配置结构版本：旧文件缺 `v` 时做一次性迁移。
const CONFIG_VERSION: i64 = 3;

/// 课程卡「文字卡片」样式的默认行模板。
pub const DEFAULT_COURSE_LAYOUT: &str = "{title}{time}\n{place}\n---\n{countdown} | {dnd}";
/// 设置页里展示给用户的模板语法说明。
pub const COURSE_LAYOUT_HELP: &str = "每行一个区块；| 分左右；--- 是虚线（可单独关掉）\n\
占位符：{time} 时间段  {place} 地点  {title} 课程名  {teacher} 教师\n\
{date} 日期  {status} 状态  {countdown} 倒计时  {end} 结束时间\n\
{dnd} 免打扰按钮  {progress} 进度条  {info} 地点·教师\n\
删掉占位符即隐藏该字段，换行/换顺序随意排。";

/// 把课程卡外观恢复成出厂值（不影响课表数据与其它设置）。
pub fn reset_course_style(cfg: &mut Config) {
    let d = Config::default();
    cfg.course_style = d.course_style;
    cfg.course_layout = d.course_layout;
    cfg.course_show_sep = d.course_show_sep;
    cfg.course_title_size = d.course_title_size;
    cfg.course_place_size = d.course_place_size;
    cfg.course_time_size = d.course_time_size;
    cfg.course_small_size = d.course_small_size;
    cfg.course_letter_spacing = d.course_letter_spacing;
    cfg.course_time_color = d.course_time_color;
    cfg.course_title_color = d.course_title_color;
    cfg.course_place_color = d.course_place_color;
    cfg.course_accent_color = d.course_accent_color;
    cfg.course_dnd_action = d.course_dnd_action;
    cfg.course_countdown_style = d.course_countdown_style;
    cfg.course_dnd_label = d.course_dnd_label;
    cfg.course_corner_r = d.course_corner_r;
    cfg.course_card_alpha = d.course_card_alpha;
    cfg.course_cell_h = d.course_cell_h;
    cfg.course_show_weekend = d.course_show_weekend;
}

/// 恢复 轻九九九（@cayww）社区液态玻璃的默认光学参数。
pub fn reset_liquid_glass_params(cfg: &mut Config) {
    let d = Config::default();
    cfg.lg_refract = d.lg_refract;
    cfg.lg_chroma = d.lg_chroma;
    cfg.lg_blur = d.lg_blur;
    cfg.lg_band = d.lg_band;
    cfg.lg_edge_width = d.lg_edge_width;
    cfg.lg_edge_opacity = d.lg_edge_opacity;
    cfg.lg_morph = d.lg_morph;
}

impl Config {
    /// 打包传给 `cayww.liquid-glass` 着色器 `params[0..7]` 的 32 个浮点参数。
    pub fn liquid_glass_params(&self) -> [f32; 32] {
        let mut p = [0.0f32; 32];
        p[0] = self.lg_refract.clamp(0.0, 120.0);
        p[1] = self.lg_chroma.clamp(0.0, 30.0);
        p[2] = self.lg_blur.clamp(0.0, 4.0);
        p[3] = self.lg_band.clamp(10.0, 140.0);
        p[4] = self.lg_edge_width.clamp(0.0, 4.0);
        p[5] = self.lg_edge_opacity.clamp(0.0, 1.0);
        p[6] = self.lg_morph.clamp(0.0, 3.0);
        p
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// 总开关。
    pub enabled: bool,
    /// 收起时在主界面时间右侧常驻显示。
    pub compact: bool,
    /// 展开页（AI 额度页顶部）显示详细状态区。
    pub details: bool,
    /// 活跃状态多久没有新上报就视为结束（0 = 一直显示）。
    pub active_timeout_secs: i64,
    /// 「已完成」在岛上保留的秒数（0 = 完成即收起）。
    pub done_timeout_secs: i64,
    /// 完成 / 等你回复 / 出错时，在状态旁边显示「回到 AI」按钮。
    pub return_button: bool,
    /// 状态文件目录；留空 = `<软件安装目录>\ai-status`（旧位置自动兼容）。
    pub status_dir: String,
    /// 「打开 AI」目标：http(s) URL 或本地 exe/lnk 路径；留空 = 回到本机应用。
    pub open_target: String,
    /// 记住各助手的 exe 路径（激活成功时写入）：应用没开着也能直接启动它。
    pub app_paths: Vec<(String, String)>,
    /// 岛边缘描边（羽化轮廓带）。默认关闭 = 取消描边，其余视觉不变。
    pub outline: bool,
    /// 在日程页显示课表（`courses.json`）。
    pub course_enabled: bool,
    /// 课前多少分钟提醒。
    pub course_remind_minutes: i64,
    /// 课表文件路径；留空 = `%APPDATA%\Auralsle\courses.json`。
    pub course_file: String,
    /// 上次导入用的 Excel 路径（只用来回填输入框）。
    pub course_xlsx: String,
    /// 学期第 1 周周一（`YYYY-MM-DD`）；留空时导入沿用 courses.json 里的旧值。
    pub course_start: String,
    // ---- 课程卡外观：完全开放的自定义项 ----
    /// 0 = 经典（大时钟）/ 1 = 文字卡片（默认）。
    pub course_style: u8,
    /// 设备药丸外观预设 0..3（见 `ui::PILL_STYLE_NAMES`，精选 4 款契合整体设计语言的胶囊样式），默认 0 = 原生灵动舱。
    pub pill_style: u8,
    /// 设备药丸位置：0 自动（换边 / 换行 / 避让）/ 1 固定主岛左侧 / 2 固定主岛右侧。
    pub pill_placement: u8,
    /// 行模板：每行一个块，`|` 分左右，`---` 是虚线；占位符见 `course_layout_help()`。
    pub course_layout: String,
    /// 虚线分割显隐。
    pub course_show_sep: bool,
    /// 各类文字字号（课程名 / 地点 / 时间段 / 其余）。
    pub course_title_size: f32,
    pub course_place_size: f32,
    pub course_time_size: f32,
    pub course_small_size: f32,
    /// 字间距（点，0 = 默认；正数拉开、负数收紧）。
    pub course_letter_spacing: f32,
    /// 颜色（`#RRGGBB`）。
    pub course_time_color: String,
    pub course_title_color: String,
    pub course_place_color: String,
    pub course_accent_color: String,
    /// 免打扰入口动作：0 岛仔专注 / 1 系统级免打扰 / 2 仅岛内静音。
    pub course_dnd_action: u8,
    /// 倒计时格式：0 中文 / 1 时钟 / 2 「还有 N 分钟」。
    pub course_countdown_style: u8,
    /// 免打扰按钮文案；留空 = 按动作自动生成。
    pub course_dnd_label: String,
    /// 音乐暂停多少秒后自动恢复原岛界面；0 = 不自动恢复。
    pub music_pause_return_secs: i64,
    /// 使用矢量表情替换岛仔的面部装饰。
    pub islo_face: bool,
    /// 固定表情：0 = 自动随状态切换；1..20 = 固定显示第 N 个表情。
    pub islo_fixed: i64,
    /// [aora] 画风索引（0-6，ArtStyle）。
    pub aora_style: u8,
    /// [aora] 身体形状索引（0-10，BodyShape）。
    pub aora_shape: u8,
    /// [aora] 颜色索引（0-7，PALETTE）。
    pub aora_color: u8,
    /// [aora] 五官脸型索引（0-15，FaceStyle）。
    pub aora_face_style: u8,
    /// [aora] 主岛吉祥物显示大小（百分比，60-150，100 = 默认）。
    pub aora_scale: u8,
    /// [aora] 是否启用自定义内部填充颜色（调色盘独立控制吉祥物内部颜色，不含五官脸型）。
    pub aora_custom_fill_enabled: bool,
    /// [aora] 吉祥物内部填充自定义 RGB 颜色。
    pub aora_custom_fill_rgb: [u8; 3],
    /// [aora] 是否启用自定义外部轮廓描边颜色（调色盘独立控制吉祥物轮廓色，不含五官脸型）。
    pub aora_custom_outline_enabled: bool,
    /// [aora] 吉祥物外部轮廓自定义 RGB 颜色。
    pub aora_custom_outline_rgb: [u8; 3],
    // ---- 灵动岛边框光效 ----
    /// 边框光效样式：0 Off, 1 Rainbow border, 2 Glass reflection, 3 Colour sweep L->R,
    /// 4 Colour sweep R->L, 5 Colour sweep there and back, 6 Neon sign, 7 Aurora, 8 Fire。
    pub border_effect: u8,
    /// 生效范围（Plays on）：0 Always（始终）, 1 Pill only（仅收起态）, 2 Open panel only（仅展开态）。
    pub border_plays_on: u8,
    /// 播放频率（Plays）：0 持续常亮, 1 每 15 秒, 2 每 30 秒, 3 每 60 秒。
    pub border_interval: u8,
    pub liquid_glass_mode: u8,
    /// @cayww 液态玻璃折射强度（0..=120 px，默认 48）。
    pub lg_refract: f32,
    /// @cayww 液态玻璃色散强度（0..=30 px，默认 8）。
    pub lg_chroma: f32,
    /// @cayww 液态玻璃内部模糊（0.0..=4.0 px，默认 1.5）。
    pub lg_blur: f32,
    /// @cayww 液态玻璃折射带宽（10..=140 px，默认 87）。
    pub lg_band: f32,
    /// @cayww 液态玻璃白边宽度（0.0..=4.0 px，默认 1.5）。
    pub lg_edge_width: f32,
    /// @cayww 液态玻璃白边不透明度（0.0..=1.0，默认 0.9）。
    pub lg_edge_opacity: f32,
    /// @cayww 液态玻璃展开形变系数（0.0..=3.0，默认 1.0）。
    pub lg_morph: f32,
    /// 音乐播放器样式切换：0 = 经典默认，1 = 灵动音律（封面流光 + 动态频谱 + 歌词同行）。
    pub music_ui_style: u8,
    pub music_show_ai_status: bool,
    /// 收起态歌词岛左侧是否独立显示时间胶囊（默认 true）。
    pub lyric_island_time_capsule: bool,
    /// 是否隐藏展开岛左右切页 `<` / `>` 按钮（默认 false）。
    pub hide_nav_arrows: bool,
    /// 音乐岛无媒体播放时自动隐藏（识别到有媒体播放即显示该岛，无媒体播放即隐藏不常驻）。
    pub music_auto_hide_when_idle: bool,
    /// 0 = 跟随基础皮肤，1..=18 = 18 套精选主题预设。
    pub win_theme: u8,
    /// 自定义强调色（#RRGGBB）。
    pub custom_accent: String,
    /// 毛玻璃与主题底板不透明度（25..=100）。
    pub glass_opacity: u8,
    /// 120fps 弹簧物理高刷动画开关。
    pub fps_120: bool,
    /// 触摸与鼠标拖拽左右滑动翻页手势开关。
    pub touch_gestures: bool,
    /// 在收起态胶囊顶栏启用单行紧凑组件条。
    pub win_compact_bar: bool,
    /// 已启用并按拖拽顺序排列的组件 ID 列表。
    pub win_widgets_order: Vec<u8>,
    /// 岛内独立「天气」页面开关（`page == 10`）。
    pub weather_page_visible: bool,
    /// 岛内独立「翻译」页面开关（`page == 11`）。
    pub translate_page_visible: bool,
    /// 天气组件城市与街道级定位信息。
    pub weather_city: String,
    /// 街道级详细位置（区/县 + 街道/路名，如 `岳麓区 · 麓山南路街道`）。
    pub weather_street: String,
    /// 街道级高精纬度（WGS84）。
    pub weather_lat: f64,
    /// 街道级高精经度（WGS84）。
    pub weather_lon: f64,
    pub weather_temp: i32,
    pub weather_desc: String,
    /// 天气源选择：0 = Open-Meteo 全球气象源, 1 = 中国天气网/和风源, 2 = Wttr.in 气象源, 3 = 彩云天气街道级源, 4 = 自定义接入天气源。
    pub weather_source: u8,
    /// 自定义天气源名称（如 `和风企业源`、`OpenWeather`、`自建气象站`）。
    pub weather_custom_name: String,
    /// 自定义气象源 URL 模板（支持 `{lat}`、`{lon}`、`{city}`、`{street}`、`{key}` 占位符）。
    pub weather_custom_url: String,
    /// 天气源 API Key / Token。
    pub weather_api_key: String,
    /// 自动刷新间隔（分钟，5..=180）。
    pub weather_refresh_mins: u16,
    /// 自由勾选显示信息：24小时逐时温度趋势曲线。
    pub weather_show_hourly: bool,
    /// 自由勾选显示信息：5日天气预报卡片。
    pub weather_show_daily: bool,
    /// 自由勾选显示信息：空气质量 AQI 与 PM2.5。
    pub weather_show_aqi: bool,
    /// 自由勾选显示信息：风向风速与气压。
    pub weather_show_wind: bool,
    /// 自由勾选显示信息：紫外线 UV 与相对湿度。
    pub weather_show_uv_hum: bool,
    /// 自由勾选显示信息：日出日落与生活指数提示。
    pub weather_show_sun: bool,
    /// 翻译源引擎：0 = Google GTX 极速引擎, 1 = MyMemory 双语记忆库, 2 = 离线词典混合引擎。
    pub translate_engine: u8,
    /// 打开翻译页时自动读取最新剪贴板文本。
    pub translate_auto_clipboard: bool,
    /// 翻译完成后自动将译文写入剪贴板。
    pub translate_auto_copy: bool,
    /// 在翻译页面暂时冻结岛体（鼠标移开也不收回岛体，方便对照阅读与连续翻译）。
    pub translate_pin_island: bool,
    // ---- 课程表增强个性化配置 ----
    pub course_cell_h: f32,
    pub course_corner_r: f32,
    pub course_card_alpha: u8,
    pub course_show_non_current: bool,
    pub course_sunday_first: bool,
    pub course_holiday_dnd: bool,
    // ---- 系统监控与定制组件配置 ----
    pub monitor_plugin: bool,
    pub monitor_cpu: bool,
    pub monitor_gpu: bool,
    pub monitor_mem: bool,
    pub monitor_disk: bool,
    pub monitor_net: bool,
    pub monitor_battery: bool,
    pub monitor_cores: bool,
    pub monitor_charts: bool,
    pub monitor_interval: u32,
    /// 悬浮云主题下是否跟随音乐专辑封面取色（暂停后自动恢复中性底色）。
    pub fluid_cover_tint: bool,
    /// AI 工作状态是否联动吉祥物表情。
    pub ai_mascot_link: bool,
    /// AI 工作联动模式：0 = 随机轮播 19 个「处理中」表情，1 = 按 AI 动作智能匹配 + 随机轮播。
    pub ai_mascot_mode: u8,
    /// AI 工作时随机切换「处理中」表情的间隔秒数（2..=15）。
    pub ai_mascot_interval: u8,
    /// 定制组件（岛内插件页）展开高度（140..=248）。
    pub plugin_island_h: u16,
    /// 定制组件岛内网格列数（2..=4）。
    pub plugin_cols: u8,
    /// 定制组件卡片圆角（4..=20）。
    pub plugin_card_radius: u8,
    /// 定制组件卡片底色不透明度（15..=100）。
    pub plugin_card_opacity: u8,
    /// 定制组件内部文字与内容缩放比例（80..=135%）。
    pub plugin_font_scale: u8,
    /// 定制组件显示内容要素：标题与图标。
    pub plugin_show_title: bool,
    /// 定制组件显示内容要素：核心大字数值。
    pub plugin_show_value: bool,
    /// 定制组件显示内容要素：副标题与详情。
    pub plugin_show_sub: bool,
    /// 定制组件显示内容要素：进度条与 60 秒实时趋势折线。
    pub plugin_show_bar: bool,
    /// 定制组件各自在岛内的卡片尺寸（0 = 1x1 标准卡，1 = 2x1 宽幅卡，2 = 全宽横幅）。
    pub plugin_sizes: Vec<u8>,
    /// GPU 硬件渲染后端选择：0 = 自动选择 (Auto), 1 = Vulkan 硬件直通, 2 = OpenGL (WGL/Glow) 硬件加速。
    pub gpu_backend: u8,
    /// 岛体电量图标样式：0 = 经典圆环, 1 = iOS 胶囊电池, 2 = 极简立式能量柱, 3 = 双轨霓虹弧表, 4 = 闪电护盾徽章。
    pub battery_style: u8,
    // ---- 微信通知本地内容读取、摄像头防窥与本地硬件指纹联动 ----
    /// 微信来消息时在本地内存读取通知正文预览（100% 纯本地处理，不联网、不落盘）。
    pub wechat_read_content: bool,
    /// 接入本地摄像头防窥保护（检测到有人斜视屏幕或多人旁观时自动打码隐藏消息内容）。
    pub wechat_camera_guard: bool,
    /// 本地硬件指纹联动解锁（已升级为快捷键一键显隐，保留字段兼容旧配置）。
    pub wechat_fingerprint_lock: bool,
    /// 微信消息快捷键显隐：来消息时按下该快捷键显示当前通知内容（仅对当前通知有效，默认 "Alt+W"）。
    pub wechat_reveal_hotkey: String,
    /// 自动识别「OPPO 互联」通知并与电脑端微信去重（电脑登录微信时屏蔽 OPPO 互联重复微信通知）。
    pub oppo_wechat_dedup: bool,
    /// 开启「OPPO 互联」手机通知日志镜像（读取 `ovmsocketlog.log` 提取来源 App 原始图标与富文本上岛）。
    pub oppo_notify_mirror_enabled: bool,
    /// 通过系统应用通知开关关闭 `com.oplus.devicespace` 自身弹窗，避免与灵屿镜像重复弹出。
    pub oppo_suppress_native: bool,
    /// AI 翻译服务商（参考 NEKOparapa/AiNiee）：0=DeepSeek, 1=OpenAI, 2=硅基流动 SiliconFlow, 3=Gemini, 4=Claude, 5=本地 Ollama, 6=自定义接口。
    pub translate_ai_provider: u8,
    /// AI 翻译接口地址（OpenAI 兼容 /v1/chat/completions）。
    pub translate_ai_base_url: String,
    /// AI 翻译 API Key。
    pub translate_ai_api_key: String,
    /// AI 翻译模型名称（如 deepseek-chat / gpt-4o-mini / qwen-plus）。
    pub translate_ai_model: String,
    /// AI 翻译提示词模板（AiNiee Prompt 风格）。
    pub translate_ai_prompt: String,
    /// 课表周视图是否显示周末（周六、周日）。
    pub course_show_weekend: bool,
    /// 岛体页面切换动画预设（参考 DavidHDev/react-bits 精调）：0=平滑滑动, 1=流光渐显 (AnimatedContent), 2=柔焦分裂 (BlurFade), 3=磁吸回弹 (MagnetSpring)。
    pub page_switch_anim: u8,
    /// 定制组件每个小组件的专属视觉样式（0=默认专属样式, 1=圆环仪表盘, 2=极简大字卡, 3=实时波形图）。
    pub plugin_card_styles: Vec<u8>,
    /// 定制组件每个小组件的自定义标题（留空使用默认名称）。
    pub plugin_custom_titles: Vec<String>,
    /// 是否在灵动岛上显示已连接的蓝牙设备与 Voltide 2.4GHz 无线外设电量。
    pub show_bt_battery_on_island: bool,
    /// 是否启用 sdparklab/Voltide 2.4GHz 无线外设与蓝牙电量联合监控。
    pub voltide_enabled: bool,
    /// Wisland 特性：锁定岛体位置（防止误触滑动或更改位置偏移）。
    pub island_pos_locked: bool,
    /// Wisland 特性：展开面板宽度连续调节比例（80..=130%）。
    pub expanded_w_scale: u8,
    /// Wisland 特性：展开面板高度连续调节比例（80..=135%）。
    pub expanded_h_scale: u8,
    /// Wisland 特性：AgentAI 状态灯（通过生命周期钩子显示运行、完成或需审批状态灯）。
    pub agent_status_lamp: bool,
    /// 主岛收起态同时展示的 AI 工作状态数量（1..=5）。
    pub compact_max_count: u8,
    /// 开机自启动方式：0 = 正常，1 = 优先自启动，2 = 滞后自启动。
    pub autostart_mode: u8,
    /// Wisland 特性：Agent 想要执行命令或改文件时，在岛上直接允许、拒绝或交回终端（支持全局快捷键）。
    pub agent_hook_approval: bool,
    /// 天气界面方案选择：0=Breezy 街道级微风天气(v13开发版), 1=0.6.64 原版天气(七日预报+日照弧线)。
    pub weather_ui_mode: u8,
    /// 消息与微信提醒方案选择：0=增强融合模式(五端适配+微信本地正文+快捷键+OPPO互联), 1=0.6.64 原版极简模式。
    pub messenger_mode: u8,
    /// AI 额度界面方案选择：0=开发版双栏对比+底栏总览, 1=0.6.64 原版精简详情卡。
    pub quota_ui_mode: u8,
    pub face_fill_alpha: u8,
    pub face_stroke_alpha: u8,
    pub msg_island_filter: u8,
    pub minimal_time_pill: bool,
    /// 双击岛体临时避让开关（开启后双击灵动岛可进入/退出临时极简避让状态）。
    pub temp_avoid_enabled: bool,
    /// 全屏时是否使用临时极简避让替代完全隐藏。
    pub temp_avoid_fullscreen: bool,
    /// 临时避让状态下是否继续在极简胶囊中显示时间。
    pub temp_avoid_show_time: bool,
    pub msg_hub_order: [u8; 3],
    pub clipboard_page_visible: bool,
    pub clip_filter_tab: u8,
    pub clip_search_query: String,
    pub clip_notes: Vec<(String, String)>,
    pub todo_sub_tab: u8,
    pub nav_bar_style: u8,
    /// 页面指示点悬浮页名位置：0 = 指示点正上方（预留底距），1 = 底栏右侧留白区
    pub nav_badge_pos: u8,
    pub pet_user_configured: bool,
    pub minimal_pill_width: u8,
    pub custom_font_path: String,
    /// 极简药丸状态下，来消息时在右侧显示与药丸等高缩略的未读消息悬浮球（点击或展开药丸时丝滑过渡放大）。
    pub minimal_pill_msg_badge: bool,
    /// 收起态胶囊/刘海是否显示音乐频谱与歌词。
    pub pill_show_music: bool,
    /// 收起态胶囊/刘海是否显示番茄专注计时。
    pub pill_show_focus: bool,
    /// 收起态胶囊/刘海是否显示 AI 额度进度条。
    pub pill_show_quota: bool,
    /// 识别到复制文本后，是否在 N 秒内打开岛默认第一展开剪贴板。
    pub copy_auto_open_clipboard: bool,
    /// 识别到复制文本后默认优先展开剪贴板的有效时间窗口（秒，默认 10 秒）。
    pub copy_auto_open_secs: u32,
    /// 定制组件岛 UI 方案选择：0 = Shadcn UI 新版定制组件岛（推荐），1 = 经典网格定制组件岛（旧版）。
    /// 定制组件岛界面方案：0 = Glance 灵动监控组件岛 (lulu-loopp/glance 新版)，1 = 经典网格定制组件岛（旧版）。
    pub plugin_ui_mode: u8,
    /// 包含全部 13 个展开态功能页面（含剪贴板 12、天气 10、翻译 11）的完整自定义排序数组。
    pub full_page_order: Vec<usize>,
    /// 是否显示展开岛左上角页面标题文字（需求 1：关闭后不显示左上角文字，开启时与下方内容左对齐）。
    pub show_island_page_title: bool,
    /// 剪贴板界面方案（需求 6a）：0 = BrimDeck 图文混合面板（支持图片与文字保存、分类卡片流与一键粘贴），1 = Fly-Island 列表（旧版）。
    pub clipboard_ui_mode: u8,
    /// 点击剪贴板历史条目时是否自动向当前活动窗口发送 Ctrl+V 一键直接粘贴（需求 6a）。
    pub clipboard_auto_paste: bool,
    /// 剪贴板点击粘贴/复制后的收起模式（0 = 鼠标离开就收起，1 = 粘贴后点击空白区域后收起，2 = 粘贴后 N 秒后收起）。
    pub clipboard_close_mode: u8,
    /// 剪贴板粘贴后延迟收起秒数（1..=15 秒，用于模式 2）。
    pub clipboard_close_delay_secs: u32,
    /// 无线外设电量展示模式（需求 6b）：0 = 灵动岛侧翼设备胶囊（药丸），1 = BrimDeck 无线外设设备岛（鼠标/键盘/耳机/手柄集中显示 + 低电变色提醒）。
    pub device_display_mode: u8,
    /// Glance 自适应音乐岛控件形态（需求 7）：0 = 紧凑歌词横条，1 = 三行歌词流光卡，2 = 旋转唱片与频谱舞台。
    pub glance_music_form: u8,
    /// Glance 定制组件岛内部子形态（需求 4）：0 = 60s 曲线读数卡片，1 = 磨砂玻璃圆环矩阵，2 = 紧凑硬件监控横条。
    pub glance_widget_form: u8,
    /// 是否在展开态灵动岛启用独立「设备岛」页面（Page 13，纳入页面管理自由排序与开关）。
    pub device_page_visible: bool,
    /// 仅在连接无线外设时显示设备岛（与音乐岛逻辑一致：连接无线外设后自动显示，未连接时自动隐藏）。
    pub device_auto_hide_when_empty: bool,
    /// 通知悬浮窗软件图标右下角是否显示未读数字角标（需求 9，图五 `image_a9ec52.png`）。
    pub show_unread_badge_count: bool,
    /// 吉祥物是否启用自定义五官表情颜色（需求 6）。
    pub aora_custom_ink_enabled: bool,
    /// 吉祥物自定义五官表情 RGB 颜色。
    pub aora_custom_ink_rgb: [u8; 3],
    /// 吉祥物自定义五官表情 Alpha 透明度。
    pub aora_custom_ink_alpha: u8,
    /// 音乐岛「经典分栏」UI 是否启用专辑封面取色氛围与进度条染色（需求 13）
    pub classic_music_cover_tint: bool,
    /// 自定义备份文件保存目录（留空则使用默认配置目录，需求 10）
    pub custom_backup_dir: String,
    /// 自定义恢复文件路径（.zip / .json，需求 10）
    pub custom_restore_file: String,
    /// 圆形专辑封面（黑胶唱机 / Glance舞台）播放时是否旋转（需求 12）
    pub music_round_art_rotate: bool,
    /// 是否已完成首次岛上歌词默认开启迁移（需求 1）
    pub island_lyrics_inited: bool,
    /// 插件卡片自定义主显示内容（留空显示实时采集数值，需求 11）
    pub plugin_custom_values: Vec<String>,
    /// 插件卡片自定义副标题/红框文字（留空显示默认硬件副文本，需求 11）
    pub plugin_custom_subs: Vec<String>,
    /// 插件卡片标题字号（pt，需求 11）
    pub plugin_title_size: f32,
    /// 插件卡片主数值字号（pt，需求 11）
    pub plugin_value_size: f32,
    /// 插件卡片副文本（图5红框处）字号（pt，默认增大至 9.4pt，需求 11）
    pub plugin_sub_size: f32,
    /// 是否在展开态灵动岛启用独立「亮度控制岛」页面（Page 14，纳入页面管理自由排序与开关）。
    pub brightness_page_visible: bool,
    /// 功能模块独立启停开关（关闭对应模块后自动停止其后台线程、轮询与缓存以减少内存占用）。
    pub mod_music_enabled: bool,
    pub mod_focus_health_enabled: bool,
    pub mod_shelf_clipboard_enabled: bool,
    pub mod_mail_calendar_enabled: bool,
    pub mod_ai_suite_enabled: bool,
    pub mod_widgets_sysmon_enabled: bool,
    pub mod_weather_translate_enabled: bool,
    pub mod_display_bt_enabled: bool,
    /// 细分功能模块独立开关（由「功能模块」页单独控制启停）
    pub mod_focus_enabled: bool,
    pub mod_health_enabled: bool,
    pub mod_shelf_enabled: bool,
    pub mod_clipboard_enabled: bool,
    pub mod_mail_enabled: bool,
    pub mod_calendar_enabled: bool,
    pub mod_course_enabled: bool,
    pub mod_ai_status_enabled: bool,
    pub mod_ai_quota_enabled: bool,
    pub mod_weather_enabled: bool,
    pub mod_translate_enabled: bool,
    pub mod_display_brightness_enabled: bool,
    pub mod_bt_devices_enabled: bool,
    pub mod_oppo_connect_enabled: bool,
    /// 是否开启后台定时自动检查更新，以及检查周期（小时）。
    pub auto_check_updates: bool,
    pub update_check_interval_hours: u32,
    /// AI 灵动岛展示方案：1 = 方案 1（现有的分立 AI 额度 + AI 工作双页），2 = 方案 2（flyl1u/fly-island 合并视图：顶部同心双环额度 + 下方多会话列表）。
    pub ai_island_scheme: u8,
    /// fly-island 会话检测源开关：Claude Code (`~/.claude/projects/**/*.jsonl`)
    pub fly_claude_sessions: bool,
    /// fly-island 会话检测源开关：OpenAI Codex (`~/.codex/sessions/**/*.jsonl`)
    pub fly_codex_sessions: bool,
    /// fly-island 会话检测源开关：DeepSeek Harness (`~/.dsh/sessions/**/*.jsonl`)
    pub fly_dsh_sessions: bool,
    /// fly-island 会话检测源开关：豆包 / 豆包工作桌面端实时日志流 (`%LOCALAPPDATA%\Doubao`)
    pub fly_doubao_sessions: bool,
    /// fly-island 会话检测源开关：Gemini Antigravity (`~/.gemini/antigravity`)
    pub fly_antigravity_sessions: bool,
    /// fly-island 会话检测源开关：WorkBuddy / CodeBuddy (`~/.workbuddy/projects`)
    pub fly_workbuddy_sessions: bool,
    /// fly-island 会话检测源开关：Grok Bot (`%APPDATA%\Grok Bot` / `~/.grokbot`)
    pub fly_grokbot_sessions: bool,
    /// fly-island 会话检测源开关：Cursor (`~/.cursor/projects/**/agent-transcripts`)
    pub fly_cursor_sessions: bool,
    /// fly-island 会话检测源开关：Trae / Trae CN (`%APPDATA%\Trae` / `~/.trae`)
    pub fly_trae_sessions: bool,
    /// fly-island 会话检测源开关：Muse / MuseWork (`~/.muse` / `%APPDATA%\Muse`)
    pub fly_muse_sessions: bool,
    /// fly-island 会话检测源开关：Dot / Dots (`~/.dot` / `%APPDATA%\Dot`)
    pub fly_dot_sessions: bool,
    /// 无本机实时会话时是否展示 fly-island 演示会话以供预览
    pub fly_demo_sessions: bool,
    // ---- Auralsle 灵屿音乐岛与验证码胶囊配置 ----
    pub auralsle_music_layout: String,
    pub auralsle_auto_compact_width: bool,
    /// 收起态歌词岛自定义宽度（200..=440 px，全部样式生效）。
    pub lyric_island_width: u16,
    /// OPPO 互联拖拽传文件悬浮球独立开关（总开关在「功能模块 -> OPPO 互联适配」）。
    pub oppo_drag_transfer_enabled: bool,
    pub auralsle_compact_width: f32,
    pub auralsle_compact_max_width: f32,
    pub auralsle_lyric_font_size: f32,
    pub auralsle_word_highlight: bool,
    pub auralsle_compact_translation: bool,
    pub auralsle_chinese_two_line: bool,
    pub auralsle_show_translation: bool,
    pub auralsle_translation_button: bool,
    pub auralsle_color_source: String,
    pub auralsle_dynamic_seed: String,
    pub auralsle_capsule_dynamic_color: bool,
    pub auralsle_expanded_dynamic_color: bool,
    pub auralsle_lyric_cover_color: bool,
    pub auralsle_cover_overlay: bool,
    pub salt_flow_bg: bool,
    pub auralsle_text_color: String,
    pub auralsle_component_color: String,
    pub auralsle_lyric_progress_color: String,
    pub auralsle_card_material: String,
    pub auralsle_acrylic_frost: f32,
    pub auralsle_acrylic_opacity: f32,
    pub auralsle_bar_style: String,
    pub auralsle_thumb_style: String,
    pub auralsle_thumb_enabled: bool,
    pub auralsle_thumb_spin: bool,
    pub auralsle_transport_theme: String,
    pub auralsle_music_glow: bool,
    pub auralsle_music_glow_mode: String,
    pub auralsle_click_open_player: bool,
    pub auralsle_hover_tooltip: bool,
    pub auralsle_custom_layout_json: String,
    pub auralsle_otp_capsule: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            enabled: true,
            compact: true,
            details: true,
            active_timeout_secs: 0,
            done_timeout_secs: 45,
            return_button: true,
            status_dir: String::new(),
            open_target: String::new(),
            app_paths: Vec::new(),
            outline: false,
            course_enabled: true,
            course_remind_minutes: 15,
            course_file: String::new(),
            course_xlsx: String::new(),
            course_start: "2026-09-07".to_owned(),
            course_style: 1,
            pill_style: 0,
            pill_placement: 0,
            course_layout: DEFAULT_COURSE_LAYOUT.to_owned(),
            course_show_sep: true,
            course_title_size: 20.0,
            course_place_size: 16.0,
            course_time_size: 13.0,
            course_small_size: 10.0,
            course_letter_spacing: 0.0,
            course_time_color: "#FFFFFF".to_owned(),
            course_title_color: "#FFFFFF".to_owned(),
            course_place_color: "#FFBE5C".to_owned(),
            course_accent_color: "#57B6FF".to_owned(),
            course_dnd_action: 0,
            course_countdown_style: 0,
            course_dnd_label: String::new(),
            music_pause_return_secs: 10,
            islo_face: true,
            islo_fixed: 0,
            aora_style: 0,
            aora_shape: 0,
            aora_color: 0,
            aora_face_style: 0,
            aora_scale: 100,
            aora_custom_fill_enabled: false,
            aora_custom_fill_rgb: [241, 226, 193],
            aora_custom_outline_enabled: false,
            aora_custom_outline_rgb: [56, 189, 248],
            border_effect: 0,
            border_plays_on: 0,
            border_interval: 0,
            liquid_glass_mode: 0,
            lg_refract: 48.0,
            lg_chroma: 8.0,
            lg_blur: 1.5,
            lg_band: 87.0,
            lg_edge_width: 1.5,
            lg_edge_opacity: 0.9,
            lg_morph: 1.0,
            music_ui_style: 6,
            music_show_ai_status: false,
            lyric_island_time_capsule: true,
            hide_nav_arrows: false,
            music_auto_hide_when_idle: false,
            win_theme: 0,
            custom_accent: "#0A84FF".to_owned(),
            glass_opacity: 84,
            fps_120: true,
            touch_gestures: true,
            win_compact_bar: false,
            win_widgets_order: vec![12, 15, 17, 19, 16, 22, 29, 32],
            weather_page_visible: true,
            translate_page_visible: true,
            weather_city: "长沙市".to_owned(),
            weather_street: "岳麓区 · 麓山南路街道".to_owned(),
            weather_lat: 28.1812,
            weather_lon: 112.9445,
            weather_temp: 22,
            weather_desc: "晴间多云".to_owned(),
            weather_source: 0,
            weather_custom_name: "自定义气象源".to_owned(),
            weather_custom_url: String::new(),
            weather_api_key: String::new(),
            weather_refresh_mins: 15,
            weather_show_hourly: true,
            weather_show_daily: true,
            weather_show_aqi: true,
            weather_show_wind: true,
            weather_show_uv_hum: true,
            weather_show_sun: true,
            translate_engine: 0,
            translate_auto_clipboard: true,
            translate_auto_copy: false,
            translate_pin_island: false,
            course_cell_h: 36.0,
            course_corner_r: 6.5,
            course_card_alpha: 88,
            course_show_non_current: true,
            course_sunday_first: false,
            course_holiday_dnd: true,
            monitor_plugin: true,
            monitor_cpu: true,
            monitor_gpu: true,
            monitor_mem: true,
            monitor_disk: true,
            monitor_net: true,
            monitor_battery: true,
            monitor_cores: true,
            monitor_charts: true,
            monitor_interval: 1000,
            fluid_cover_tint: true,
            ai_mascot_link: true,
            ai_mascot_mode: 0,
            ai_mascot_interval: 4,
            plugin_island_h: 216,
            plugin_cols: 3,
            plugin_card_radius: 10,
            plugin_card_opacity: 82,
            plugin_font_scale: 100,
            plugin_show_title: true,
            plugin_show_value: true,
            plugin_show_sub: true,
            plugin_show_bar: true,
            plugin_sizes: vec![0; 34],
            gpu_backend: 2,
            battery_style: 0,
            wechat_read_content: true,
            wechat_camera_guard: false,
            wechat_fingerprint_lock: false,
            wechat_reveal_hotkey: "Alt+W".to_owned(),
            oppo_wechat_dedup: true,
            oppo_notify_mirror_enabled: true,
            oppo_suppress_native: true,
            translate_ai_provider: 0,
            translate_ai_base_url: "https://api.deepseek.com/v1/chat/completions".to_owned(),
            translate_ai_api_key: String::new(),
            translate_ai_model: "deepseek-chat".to_owned(),
            translate_ai_prompt: "你是一个专业、精准的 AI 翻译引擎（AiNiee 模式）。请直接输出准确、自然流畅的译文，不要附加任何解释或额外引号。".to_owned(),
            course_show_weekend: true,
            page_switch_anim: 0,
            plugin_card_styles: vec![0; 34],
            plugin_custom_titles: vec![String::new(); 34],
            show_bt_battery_on_island: true,
            voltide_enabled: true,
            island_pos_locked: false,
            expanded_w_scale: 100,
            expanded_h_scale: 100,
            agent_status_lamp: true,
            compact_max_count: 1,
            autostart_mode: 0,
            agent_hook_approval: true,
            weather_ui_mode: 2,
            messenger_mode: 0,
            quota_ui_mode: 0,
            face_fill_alpha: 255,
            face_stroke_alpha: 255,
            msg_island_filter: 0,
            minimal_time_pill: true,
            temp_avoid_enabled: true,
            temp_avoid_fullscreen: false,
            temp_avoid_show_time: true,
            msg_hub_order: [0, 1, 2],
            clipboard_page_visible: true,
            clip_filter_tab: 0,
            clip_search_query: String::new(),
            clip_notes: Vec::new(),
            todo_sub_tab: 0,
            nav_bar_style: 2,
            nav_badge_pos: 1,
            pet_user_configured: false,
            minimal_pill_width: 62,
            custom_font_path: String::new(),
            minimal_pill_msg_badge: true,
            pill_show_music: true,
            pill_show_focus: true,
            pill_show_quota: true,
            copy_auto_open_clipboard: true,
            copy_auto_open_secs: 10,
            plugin_ui_mode: 0,
            full_page_order: vec![0, 1, 2, 3, 12, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14],
            show_island_page_title: true,
            clipboard_ui_mode: 0,
            clipboard_auto_paste: true,
            clipboard_close_mode: 1,
            clipboard_close_delay_secs: 3,
            device_display_mode: 0,
            glance_music_form: 2,
            glance_widget_form: 0,
            device_page_visible: true,
            device_auto_hide_when_empty: true,
            show_unread_badge_count: true,
            aora_custom_ink_enabled: false,
            aora_custom_ink_rgb: [250, 250, 252],
            aora_custom_ink_alpha: 255,
            classic_music_cover_tint: true,
            custom_backup_dir: String::new(),
            custom_restore_file: String::new(),
            music_round_art_rotate: true,
            island_lyrics_inited: false,
            plugin_custom_values: vec![String::new(); 34],
            plugin_custom_subs: vec![String::new(); 34],
            plugin_title_size: 8.8,
            plugin_value_size: 13.8,
            plugin_sub_size: 9.4,
            brightness_page_visible: true,
            mod_music_enabled: true,
            mod_focus_health_enabled: true,
            mod_shelf_clipboard_enabled: true,
            mod_mail_calendar_enabled: true,
            mod_ai_suite_enabled: true,
            mod_widgets_sysmon_enabled: true,
            mod_weather_translate_enabled: true,
            mod_display_bt_enabled: true,
            mod_focus_enabled: true,
            mod_health_enabled: true,
            mod_shelf_enabled: true,
            mod_clipboard_enabled: true,
            mod_mail_enabled: true,
            mod_calendar_enabled: true,
            mod_course_enabled: true,
            mod_ai_status_enabled: true,
            mod_ai_quota_enabled: true,
            mod_weather_enabled: true,
            mod_translate_enabled: true,
            mod_display_brightness_enabled: true,
            mod_bt_devices_enabled: true,
            mod_oppo_connect_enabled: true,
            auto_check_updates: true,
            update_check_interval_hours: 24,
            ai_island_scheme: 2,
            fly_claude_sessions: true,
            fly_codex_sessions: true,
            fly_dsh_sessions: true,
            fly_doubao_sessions: true,
            fly_antigravity_sessions: true,
            fly_workbuddy_sessions: true,
            fly_grokbot_sessions: true,
            fly_cursor_sessions: true,
            fly_trae_sessions: true,
            fly_muse_sessions: true,
            fly_dot_sessions: true,
            fly_demo_sessions: false,
            auralsle_music_layout: "SideBySide".to_owned(),
            auralsle_auto_compact_width: true,
            lyric_island_width: 300,
            oppo_drag_transfer_enabled: true,
            auralsle_compact_width: 260.0,
            auralsle_compact_max_width: 420.0,
            auralsle_lyric_font_size: 14.0,
            auralsle_word_highlight: true,
            auralsle_compact_translation: true,
            auralsle_chinese_two_line: true,
            auralsle_show_translation: true,
            auralsle_translation_button: true,
            auralsle_color_source: "Content".to_owned(),
            auralsle_dynamic_seed: "#A9E6CD".to_owned(),
            auralsle_capsule_dynamic_color: false,
            auralsle_expanded_dynamic_color: true,
            auralsle_lyric_cover_color: true,
            auralsle_cover_overlay: true,
            salt_flow_bg: true,
            auralsle_text_color: "#F4F6F8".to_owned(),
            auralsle_component_color: "#B1E8D1".to_owned(),
            auralsle_lyric_progress_color: "#A9E6CD".to_owned(),
            auralsle_card_material: "Acrylic".to_owned(),
            auralsle_acrylic_frost: 40.0,
            auralsle_acrylic_opacity: 60.0,
            auralsle_bar_style: "Rounded".to_owned(),
            auralsle_thumb_style: "Pill".to_owned(),
            auralsle_thumb_enabled: false,
            auralsle_thumb_spin: false,
            auralsle_transport_theme: "Reef".to_owned(),
            auralsle_music_glow: false,
            auralsle_music_glow_mode: "Breath".to_owned(),
            auralsle_click_open_player: false,
            auralsle_hover_tooltip: true,
            auralsle_custom_layout_json: String::new(),
            auralsle_otp_capsule: true,
        }
    }
}

fn config_path() -> PathBuf {
    crate::model::config_dir().join(CONFIG_FILE)
}

// ---------- [ai-status-mod] AI 状态检测的数据与工具链默认随软件安装目录走 ----------

/// 软件安装目录：`auralsle.exe` 所在目录；取不到时退回 `%APPDATA%\Auralsle`。
pub fn install_dir() -> PathBuf {
    std::env::current_exe().ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(crate::model::config_dir)
}

/// MCP / 钩子脚本模板目录，默认在软件安装目录下，便于整目录备份与绿色迁移。
pub fn tools_dir() -> PathBuf {
    install_dir().join("ai-status-tools")
}

/// 旧版 ZenDesktop 协议状态目录，仅作兼容回退。
pub fn legacy_status_dir() -> Option<PathBuf> {
    let local = std::env::var("LOCALAPPDATA").unwrap_or_default();
    (!local.is_empty()).then(|| PathBuf::from(local).join("ZenDesktop").join("ai-status"))
}

fn has_status_files(dir: &Path) -> bool {
    fs::read_dir(dir).ok().is_some_and(|entries| {
        entries.flatten().any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
    })
}

fn field<'a>(v: &'a Value, key: &str) -> Option<&'a Value> {
    v.get(key)
}
fn bool_or(v: Option<&Value>, key_default: bool) -> bool {
    v.and_then(Value::as_bool).unwrap_or(key_default)
}
fn i64_or(v: Option<&Value>, d: i64) -> i64 {
    v.and_then(Value::as_i64).unwrap_or(d)
}
fn f64_or(v: Option<&Value>, d: f64) -> f64 {
    v.and_then(Value::as_f64).unwrap_or(d)
}
fn str_or(v: Option<&Value>, d: &str) -> String {
    v.and_then(Value::as_str).unwrap_or(d).to_owned()
}
fn rgb_or(v: Option<&Value>, d: [u8; 3]) -> [u8; 3] {
    if let Some(arr) = v.and_then(Value::as_array) {
        if arr.len() >= 3 {
            return [
                arr[0].as_u64().unwrap_or(d[0] as u64).min(255) as u8,
                arr[1].as_u64().unwrap_or(d[1] as u64).min(255) as u8,
                arr[2].as_u64().unwrap_or(d[2] as u64).min(255) as u8,
            ];
        }
    }
    d
}

impl Config {
    pub fn load() -> Self {
        let d = Self::default();
        let text = fs::read_to_string(config_path()).ok();
        let Some(text) = text else { return d };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { return d };
        Self {
            enabled: bool_or(field(&v, "enabled"), d.enabled),
            compact: bool_or(field(&v, "compact"), d.compact),
            details: bool_or(field(&v, "details"), d.details),
            active_timeout_secs: i64_or(field(&v, "active_timeout_secs"), d.active_timeout_secs).max(0),
            done_timeout_secs: if i64_or(field(&v, "v"), 1) < CONFIG_VERSION {
                d.done_timeout_secs
            } else {
                match i64_or(field(&v, "done_timeout_secs"), d.done_timeout_secs) {
                    v if v <= 0 => d.done_timeout_secs,
                    v => v.min(600),
                }
            },
            return_button: bool_or(field(&v, "return_button"), d.return_button),
            status_dir: str_or(field(&v, "status_dir"), &d.status_dir),
            open_target: str_or(field(&v, "open_target"), &d.open_target),
            app_paths: v
                .get("app_paths")
                .and_then(Value::as_object)
                .map(|m| {
                    m.iter()
                        .filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect()
                })
                .unwrap_or_default(),
            outline: bool_or(field(&v, "outline"), d.outline),
            course_enabled: bool_or(field(&v, "course_enabled"), d.course_enabled),
            course_remind_minutes: i64_or(field(&v, "course_remind_minutes"), d.course_remind_minutes)
                .clamp(0, 120),
            course_file: str_or(field(&v, "course_file"), &d.course_file),
            course_xlsx: str_or(field(&v, "course_xlsx"), &d.course_xlsx),
            course_start: str_or(field(&v, "course_start"), &d.course_start),
            course_style: if bool_or(field(&v, "course_template_v38"), false) {
                i64_or(field(&v, "course_style"), d.course_style as i64).clamp(0, 3) as u8
            } else {
                1
            },
            pill_style: (i64_or(field(&v, "pill_style"), d.pill_style as i64).max(0) as u8) % 4,
            pill_placement: i64_or(field(&v, "pill_placement"), d.pill_placement as i64).clamp(0, 2) as u8,
            course_layout: {
                let raw = str_or(field(&v, "course_layout"), &d.course_layout);
                if !bool_or(field(&v, "course_template_v38"), false)
                    || raw.trim() == "{time} {place}\n{title}\n---\n{countdown} | {dnd}"
                {
                    d.course_layout.clone()
                } else {
                    raw
                }
            },
            course_show_sep: if bool_or(field(&v, "course_template_v38"), false) {
                bool_or(field(&v, "course_show_sep"), d.course_show_sep)
            } else {
                true
            },
            course_title_size: if bool_or(field(&v, "course_template_v38"), false) {
                f64_or(field(&v, "course_title_size"), d.course_title_size as f64).clamp(8.0, 32.0) as f32
            } else {
                20.0
            },
            course_place_size: if bool_or(field(&v, "course_template_v38"), false) {
                f64_or(field(&v, "course_place_size"), d.course_place_size as f64).clamp(8.0, 32.0) as f32
            } else {
                16.0
            },
            course_time_size: f64_or(field(&v, "course_time_size"), d.course_time_size as f64)
                .clamp(6.0, 28.0) as f32,
            course_small_size: f64_or(field(&v, "course_small_size"), d.course_small_size as f64)
                .clamp(6.0, 20.0) as f32,
            course_letter_spacing: f64_or(field(&v, "course_letter_spacing"), d.course_letter_spacing as f64)
                .clamp(-2.0, 8.0) as f32,
            course_time_color: str_or(field(&v, "course_time_color"), &d.course_time_color),
            course_title_color: str_or(field(&v, "course_title_color"), &d.course_title_color),
            course_place_color: str_or(field(&v, "course_place_color"), &d.course_place_color),
            course_accent_color: str_or(field(&v, "course_accent_color"), &d.course_accent_color),
            course_dnd_action: i64_or(field(&v, "course_dnd_action"), d.course_dnd_action as i64)
                .clamp(0, 2) as u8,
            course_countdown_style: i64_or(
                field(&v, "course_countdown_style"),
                d.course_countdown_style as i64,
            )
            .clamp(0, 2) as u8,
            course_dnd_label: str_or(field(&v, "course_dnd_label"), &d.course_dnd_label),
            music_pause_return_secs: i64_or(
                field(&v, "music_pause_return_secs"),
                d.music_pause_return_secs,
            )
            .clamp(0, 3600),
            islo_face: bool_or(field(&v, "islo_face"), d.islo_face),
            islo_fixed: i64_or(field(&v, "islo_fixed"), d.islo_fixed).clamp(0, 44),
            aora_style: i64_or(field(&v, "aora_style"), d.aora_style as i64).clamp(0, 20) as u8,
            aora_shape: i64_or(field(&v, "aora_shape"), d.aora_shape as i64).clamp(0, 71) as u8,
            aora_color: i64_or(field(&v, "aora_color"), d.aora_color as i64).clamp(0, 7) as u8,
            aora_face_style: i64_or(field(&v, "aora_face_style"), d.aora_face_style as i64).clamp(0, 15) as u8,
            aora_scale: i64_or(field(&v, "aora_scale"), d.aora_scale as i64).clamp(60, 150) as u8,
            aora_custom_fill_enabled: bool_or(field(&v, "aora_custom_fill_enabled"), d.aora_custom_fill_enabled),
            aora_custom_fill_rgb: rgb_or(field(&v, "aora_custom_fill_rgb"), d.aora_custom_fill_rgb),
            aora_custom_outline_enabled: bool_or(field(&v, "aora_custom_outline_enabled"), d.aora_custom_outline_enabled),
            aora_custom_outline_rgb: rgb_or(field(&v, "aora_custom_outline_rgb"), d.aora_custom_outline_rgb),
            border_effect: i64_or(field(&v, "border_effect"), d.border_effect as i64).clamp(0, 8) as u8,
            border_plays_on: i64_or(field(&v, "border_plays_on"), d.border_plays_on as i64).clamp(0, 2) as u8,
            border_interval: i64_or(field(&v, "border_interval"), d.border_interval as i64).clamp(0, 3) as u8,
            liquid_glass_mode: i64_or(field(&v, "liquid_glass_mode"), d.liquid_glass_mode as i64).clamp(0, 1) as u8,
            lg_refract: f64_or(field(&v, "lg_refract"), d.lg_refract as f64).clamp(0.0, 120.0) as f32,
            lg_chroma: f64_or(field(&v, "lg_chroma"), d.lg_chroma as f64).clamp(0.0, 30.0) as f32,
            lg_blur: f64_or(field(&v, "lg_blur"), d.lg_blur as f64).clamp(0.0, 4.0) as f32,
            lg_band: f64_or(field(&v, "lg_band"), d.lg_band as f64).clamp(10.0, 140.0) as f32,
            lg_edge_width: f64_or(field(&v, "lg_edge_width"), d.lg_edge_width as f64).clamp(0.0, 4.0) as f32,
            lg_edge_opacity: f64_or(field(&v, "lg_edge_opacity"), d.lg_edge_opacity as f64).clamp(0.0, 1.0) as f32,
            lg_morph: f64_or(field(&v, "lg_morph"), d.lg_morph as f64).clamp(0.0, 3.0) as f32,
            music_ui_style: {
                if i64_or(field(&v, "v"), 1) < 3 {
                    6
                } else {
                    let raw_val = i64_or(field(&v, "music_ui_style"), d.music_ui_style as i64).clamp(0, 6) as u8;
                    let raw = if matches!(raw_val, 2 | 3) { 6 } else { raw_val };
                    if raw == 4 { 1 } else { raw }
                }
            },
            music_auto_hide_when_idle: bool_or(field(&v, "music_auto_hide_when_idle"), d.music_auto_hide_when_idle),
            win_theme: i64_or(field(&v, "win_theme"), d.win_theme as i64).clamp(0, 18) as u8,
            custom_accent: str_or(field(&v, "custom_accent"), &d.custom_accent),
            glass_opacity: i64_or(field(&v, "glass_opacity"), d.glass_opacity as i64).clamp(25, 100) as u8,
            fps_120: bool_or(field(&v, "fps_120"), d.fps_120),
            touch_gestures: bool_or(field(&v, "touch_gestures"), d.touch_gestures),
            win_compact_bar: bool_or(field(&v, "win_compact_bar"), d.win_compact_bar),
            win_widgets_order: v
                .get("win_widgets_order")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|x| x.as_u64().map(|n| (n as u8).min(33)))
                        .collect::<Vec<u8>>()
                })
                .filter(|vec| !vec.is_empty())
                .unwrap_or_else(|| d.win_widgets_order.clone()),
            weather_page_visible: bool_or(field(&v, "weather_page_visible"), d.weather_page_visible),
            translate_page_visible: bool_or(field(&v, "translate_page_visible"), d.translate_page_visible),
            weather_city: str_or(field(&v, "weather_city"), &d.weather_city),
            weather_street: str_or(field(&v, "weather_street"), &d.weather_street),
            weather_lat: f64_or(field(&v, "weather_lat"), d.weather_lat).clamp(-90.0, 90.0),
            weather_lon: f64_or(field(&v, "weather_lon"), d.weather_lon).clamp(-180.0, 180.0),
            weather_temp: i64_or(field(&v, "weather_temp"), d.weather_temp as i64).clamp(-45, 55) as i32,
            weather_desc: str_or(field(&v, "weather_desc"), &d.weather_desc),
            weather_source: i64_or(field(&v, "weather_source"), d.weather_source as i64).clamp(0, 4) as u8,
            weather_custom_name: str_or(field(&v, "weather_custom_name"), &d.weather_custom_name),
            weather_custom_url: str_or(field(&v, "weather_custom_url"), &d.weather_custom_url),
            weather_api_key: str_or(field(&v, "weather_api_key"), &d.weather_api_key),
            weather_refresh_mins: i64_or(field(&v, "weather_refresh_mins"), d.weather_refresh_mins as i64).clamp(5, 180) as u16,
            weather_show_hourly: bool_or(field(&v, "weather_show_hourly"), d.weather_show_hourly),
            weather_show_daily: bool_or(field(&v, "weather_show_daily"), d.weather_show_daily),
            weather_show_aqi: bool_or(field(&v, "weather_show_aqi"), d.weather_show_aqi),
            weather_show_wind: bool_or(field(&v, "weather_show_wind"), d.weather_show_wind),
            weather_show_uv_hum: bool_or(field(&v, "weather_show_uv_hum"), d.weather_show_uv_hum),
            weather_show_sun: bool_or(field(&v, "weather_show_sun"), d.weather_show_sun),
            translate_engine: i64_or(field(&v, "translate_engine"), d.translate_engine as i64).clamp(0, 3) as u8,
            translate_auto_clipboard: bool_or(field(&v, "translate_auto_clipboard"), d.translate_auto_clipboard),
            translate_auto_copy: bool_or(field(&v, "translate_auto_copy"), d.translate_auto_copy),
            translate_pin_island: bool_or(field(&v, "translate_pin_island"), d.translate_pin_island),
            course_cell_h: f64_or(field(&v, "course_cell_h"), d.course_cell_h as f64).clamp(28.0, 58.0) as f32,
            course_corner_r: f64_or(field(&v, "course_corner_r"), d.course_corner_r as f64).clamp(2.0, 16.0) as f32,
            course_card_alpha: i64_or(field(&v, "course_card_alpha"), d.course_card_alpha as i64).clamp(30, 100) as u8,
            course_show_non_current: bool_or(field(&v, "course_show_non_current"), d.course_show_non_current),
            course_sunday_first: bool_or(field(&v, "course_sunday_first"), d.course_sunday_first),
            course_holiday_dnd: bool_or(field(&v, "course_holiday_dnd"), d.course_holiday_dnd),
            monitor_plugin: bool_or(field(&v, "monitor_plugin"), d.monitor_plugin),
            monitor_cpu: bool_or(field(&v, "monitor_cpu"), d.monitor_cpu),
            monitor_gpu: bool_or(field(&v, "monitor_gpu"), d.monitor_gpu),
            monitor_mem: bool_or(field(&v, "monitor_mem"), d.monitor_mem),
            monitor_disk: bool_or(field(&v, "monitor_disk"), d.monitor_disk),
            monitor_net: bool_or(field(&v, "monitor_net"), d.monitor_net),
            monitor_battery: bool_or(field(&v, "monitor_battery"), d.monitor_battery),
            monitor_cores: bool_or(field(&v, "monitor_cores"), d.monitor_cores),
            monitor_charts: bool_or(field(&v, "monitor_charts"), d.monitor_charts),
            monitor_interval: i64_or(field(&v, "monitor_interval"), d.monitor_interval as i64)
                .clamp(250, 3000) as u32,
            fluid_cover_tint: bool_or(field(&v, "fluid_cover_tint"), d.fluid_cover_tint),
            ai_mascot_link: bool_or(field(&v, "ai_mascot_link"), d.ai_mascot_link),
            ai_mascot_mode: i64_or(field(&v, "ai_mascot_mode"), d.ai_mascot_mode as i64).clamp(0, 1) as u8,
            ai_mascot_interval: i64_or(field(&v, "ai_mascot_interval"), d.ai_mascot_interval as i64).clamp(2, 15) as u8,
            plugin_island_h: i64_or(field(&v, "plugin_island_h"), d.plugin_island_h as i64).clamp(140, 248) as u16,
            plugin_cols: i64_or(field(&v, "plugin_cols"), d.plugin_cols as i64).clamp(2, 4) as u8,
            plugin_card_radius: i64_or(field(&v, "plugin_card_radius"), d.plugin_card_radius as i64).clamp(4, 20) as u8,
            plugin_card_opacity: i64_or(field(&v, "plugin_card_opacity"), d.plugin_card_opacity as i64).clamp(15, 100) as u8,
            plugin_font_scale: i64_or(field(&v, "plugin_font_scale"), d.plugin_font_scale as i64).clamp(80, 135) as u8,
            plugin_show_title: bool_or(field(&v, "plugin_show_title"), d.plugin_show_title),
            plugin_show_value: bool_or(field(&v, "plugin_show_value"), d.plugin_show_value),
            plugin_show_sub: bool_or(field(&v, "plugin_show_sub"), d.plugin_show_sub),
            plugin_show_bar: bool_or(field(&v, "plugin_show_bar"), d.plugin_show_bar),
            plugin_sizes: v
                .get("plugin_sizes")
                .and_then(Value::as_array)
                .map(|arr| {
                    let mut out = vec![0u8; 34];
                    for (i, val) in arr.iter().take(34).enumerate() {
                        out[i] = val.as_u64().unwrap_or(0).min(2) as u8;
                    }
                    out
                })
                .unwrap_or_else(|| d.plugin_sizes.clone()),
            gpu_backend: i64_or(field(&v, "gpu_backend"), d.gpu_backend as i64).clamp(0, 3) as u8,
            battery_style: i64_or(field(&v, "battery_style"), d.battery_style as i64).clamp(0, 4) as u8,
            wechat_read_content: bool_or(field(&v, "wechat_read_content"), d.wechat_read_content),
            wechat_camera_guard: bool_or(field(&v, "wechat_camera_guard"), d.wechat_camera_guard),
            wechat_fingerprint_lock: bool_or(field(&v, "wechat_fingerprint_lock"), d.wechat_fingerprint_lock),
            wechat_reveal_hotkey: str_or(field(&v, "wechat_reveal_hotkey"), &d.wechat_reveal_hotkey),
            oppo_wechat_dedup: bool_or(field(&v, "oppo_wechat_dedup"), d.oppo_wechat_dedup),
            oppo_notify_mirror_enabled: bool_or(field(&v, "oppo_notify_mirror_enabled"), d.oppo_notify_mirror_enabled),
            oppo_suppress_native: bool_or(field(&v, "oppo_suppress_native"), d.oppo_suppress_native),
            translate_ai_provider: i64_or(field(&v, "translate_ai_provider"), d.translate_ai_provider as i64).clamp(0, 6) as u8,
            translate_ai_base_url: str_or(field(&v, "translate_ai_base_url"), &d.translate_ai_base_url),
            translate_ai_api_key: str_or(field(&v, "translate_ai_api_key"), &d.translate_ai_api_key),
            translate_ai_model: str_or(field(&v, "translate_ai_model"), &d.translate_ai_model),
            translate_ai_prompt: str_or(field(&v, "translate_ai_prompt"), &d.translate_ai_prompt),
            course_show_weekend: bool_or(field(&v, "course_show_weekend"), d.course_show_weekend),
            page_switch_anim: {
                let raw = i64_or(field(&v, "page_switch_anim"), d.page_switch_anim as i64).clamp(0, 4) as u8;
                if raw >= 4 { 0 } else { raw }
            },
            plugin_card_styles: v
                .get("plugin_card_styles")
                .and_then(Value::as_array)
                .map(|arr| {
                    let mut out = vec![0u8; 34];
                    for (i, val) in arr.iter().take(34).enumerate() {
                        out[i] = val.as_u64().unwrap_or(0).min(3) as u8;
                    }
                    out
                })
                .unwrap_or_else(|| d.plugin_card_styles.clone()),
            plugin_custom_titles: v
                .get("plugin_custom_titles")
                .and_then(Value::as_array)
                .map(|arr| {
                    let mut out = vec![String::new(); 34];
                    for (i, val) in arr.iter().take(34).enumerate() {
                        out[i] = val.as_str().unwrap_or("").to_owned();
                    }
                    out
                })
                .unwrap_or_else(|| d.plugin_custom_titles.clone()),
            show_bt_battery_on_island: bool_or(field(&v, "show_bt_battery_on_island"), d.show_bt_battery_on_island),
            voltide_enabled: bool_or(field(&v, "voltide_enabled"), d.voltide_enabled),
            island_pos_locked: bool_or(field(&v, "island_pos_locked"), d.island_pos_locked),
            expanded_w_scale: i64_or(field(&v, "expanded_w_scale"), d.expanded_w_scale as i64).clamp(80, 130) as u8,
            expanded_h_scale: i64_or(field(&v, "expanded_h_scale"), d.expanded_h_scale as i64).clamp(80, 135) as u8,
            agent_status_lamp: bool_or(field(&v, "agent_status_lamp"), d.agent_status_lamp),
            compact_max_count: i64_or(field(&v, "compact_max_count"), d.compact_max_count as i64).clamp(1, 5) as u8,
            autostart_mode: i64_or(field(&v, "autostart_mode"), d.autostart_mode as i64).clamp(0, 2) as u8,
            agent_hook_approval: bool_or(field(&v, "agent_hook_approval"), d.agent_hook_approval),
            weather_ui_mode: i64_or(field(&v, "weather_ui_mode"), d.weather_ui_mode as i64).clamp(0, 2) as u8,
            messenger_mode: i64_or(field(&v, "messenger_mode"), d.messenger_mode as i64).clamp(0, 1) as u8,
            quota_ui_mode: i64_or(field(&v, "quota_ui_mode"), d.quota_ui_mode as i64).clamp(0, 1) as u8,
            face_fill_alpha: i64_or(field(&v, "face_fill_alpha"), d.face_fill_alpha as i64).clamp(0, 255) as u8,
            face_stroke_alpha: i64_or(field(&v, "face_stroke_alpha"), d.face_stroke_alpha as i64).clamp(0, 255) as u8,
            msg_island_filter: i64_or(field(&v, "msg_island_filter"), d.msg_island_filter as i64).clamp(0, 10) as u8,
            minimal_time_pill: bool_or(field(&v, "minimal_time_pill"), d.minimal_time_pill),
            temp_avoid_enabled: bool_or(field(&v, "temp_avoid_enabled"), d.temp_avoid_enabled),
            temp_avoid_fullscreen: bool_or(field(&v, "temp_avoid_fullscreen"), d.temp_avoid_fullscreen),
            temp_avoid_show_time: bool_or(field(&v, "temp_avoid_show_time"), d.temp_avoid_show_time),
            msg_hub_order: v.get("msg_hub_order").and_then(|a| a.as_array()).and_then(|arr| {
                if arr.len() == 3 {
                    Some([
                        arr[0].as_u64().unwrap_or(0).min(2) as u8,
                        arr[1].as_u64().unwrap_or(1).min(2) as u8,
                        arr[2].as_u64().unwrap_or(2).min(2) as u8,
                    ])
                } else {
                    None
                }
            }).unwrap_or(d.msg_hub_order),
            clipboard_page_visible: bool_or(field(&v, "clipboard_page_visible"), d.clipboard_page_visible),
            clip_filter_tab: i64_or(field(&v, "clip_filter_tab"), d.clip_filter_tab as i64).clamp(0, 4) as u8,
            clip_search_query: str_or(field(&v, "clip_search_query"), &d.clip_search_query),
            clip_notes: v.get("clip_notes").and_then(Value::as_object).map(|m| {
                m.iter().filter_map(|(k, val)| val.as_str().map(|s| (k.clone(), s.to_owned()))).collect()
            }).unwrap_or_default(),
            todo_sub_tab: i64_or(field(&v, "todo_sub_tab"), d.todo_sub_tab as i64).clamp(0, 2) as u8,
            nav_bar_style: i64_or(field(&v, "nav_bar_style"), d.nav_bar_style as i64).clamp(1, 2) as u8,
            nav_badge_pos: i64_or(field(&v, "nav_badge_pos"), d.nav_badge_pos as i64).clamp(0, 1) as u8,
            pet_user_configured: bool_or(field(&v, "pet_user_configured"), d.pet_user_configured),
            minimal_pill_width: i64_or(field(&v, "minimal_pill_width"), d.minimal_pill_width as i64).clamp(48, 128) as u8,
            custom_font_path: str_or(field(&v, "custom_font_path"), &d.custom_font_path),
            minimal_pill_msg_badge: bool_or(field(&v, "minimal_pill_msg_badge"), d.minimal_pill_msg_badge),
            pill_show_music: bool_or(field(&v, "pill_show_music"), d.pill_show_music),
            music_show_ai_status: bool_or(field(&v, "music_show_ai_status"), d.music_show_ai_status),
            lyric_island_time_capsule: bool_or(field(&v, "lyric_island_time_capsule"), d.lyric_island_time_capsule),
            hide_nav_arrows: bool_or(field(&v, "hide_nav_arrows"), d.hide_nav_arrows),
            pill_show_focus: bool_or(field(&v, "pill_show_focus"), d.pill_show_focus),
            pill_show_quota: bool_or(field(&v, "pill_show_quota"), d.pill_show_quota),
            copy_auto_open_clipboard: bool_or(field(&v, "copy_auto_open_clipboard"), d.copy_auto_open_clipboard),
            copy_auto_open_secs: i64_or(field(&v, "copy_auto_open_secs"), d.copy_auto_open_secs as i64).clamp(1, 60) as u32,
            plugin_ui_mode: i64_or(field(&v, "plugin_ui_mode"), d.plugin_ui_mode as i64).clamp(0, 1) as u8,
            show_island_page_title: bool_or(field(&v, "show_island_page_title"), d.show_island_page_title),
            clipboard_ui_mode: i64_or(field(&v, "clipboard_ui_mode"), d.clipboard_ui_mode as i64).clamp(0, 1) as u8,
            clipboard_auto_paste: bool_or(field(&v, "clipboard_auto_paste"), d.clipboard_auto_paste),
            clipboard_close_mode: i64_or(field(&v, "clipboard_close_mode"), d.clipboard_close_mode as i64).clamp(0, 2) as u8,
            clipboard_close_delay_secs: i64_or(field(&v, "clipboard_close_delay_secs"), d.clipboard_close_delay_secs as i64).clamp(1, 15) as u32,
            device_display_mode: i64_or(field(&v, "device_display_mode"), d.device_display_mode as i64).clamp(0, 2) as u8,
            glance_music_form: i64_or(field(&v, "glance_music_form"), d.glance_music_form as i64).clamp(0, 2) as u8,
            glance_widget_form: i64_or(field(&v, "glance_widget_form"), d.glance_widget_form as i64).clamp(0, 2) as u8,
            device_page_visible: bool_or(field(&v, "device_page_visible"), d.device_page_visible),
            device_auto_hide_when_empty: bool_or(
                field(&v, "device_auto_hide_when_empty"),
                d.device_auto_hide_when_empty,
            ),
            show_unread_badge_count: bool_or(
                field(&v, "show_unread_badge_count"),
                d.show_unread_badge_count,
            ),
            aora_custom_ink_enabled: bool_or(
                field(&v, "aora_custom_ink_enabled"),
                d.aora_custom_ink_enabled,
            ),
            aora_custom_ink_rgb: v
                .get("aora_custom_ink_rgb")
                .and_then(Value::as_array)
                .filter(|arr| arr.len() == 3)
                .map(|arr| {
                    [
                        arr[0].as_u64().unwrap_or(250).min(255) as u8,
                        arr[1].as_u64().unwrap_or(250).min(255) as u8,
                        arr[2].as_u64().unwrap_or(252).min(255) as u8,
                    ]
                })
                .unwrap_or(d.aora_custom_ink_rgb),
            aora_custom_ink_alpha: i64_or(
                field(&v, "aora_custom_ink_alpha"),
                d.aora_custom_ink_alpha as i64,
            )
            .clamp(30, 255) as u8,
            full_page_order: {
                let mut ord: Vec<usize> = v
                    .get("full_page_order")
                    .and_then(Value::as_array)
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|x| x.as_u64().map(|n| n as usize))
                            .filter(|&p| p <= 14)
                            .collect()
                    })
                    .unwrap_or_default();
                let mut dedup = Vec::with_capacity(15);
                for p in ord.drain(..) {
                    if !dedup.contains(&p) {
                        dedup.push(p);
                    }
                }
                for &def_p in &[0usize, 1, 2, 3, 12, 4, 5, 6, 7, 8, 9, 10, 11, 13, 14] {
                    if !dedup.contains(&def_p) {
                        dedup.push(def_p);
                    }
                }
                dedup
            },
            classic_music_cover_tint: bool_or(
                field(&v, "classic_music_cover_tint"),
                d.classic_music_cover_tint,
            ),
            custom_backup_dir: str_or(field(&v, "custom_backup_dir"), &d.custom_backup_dir),
            custom_restore_file: str_or(field(&v, "custom_restore_file"), &d.custom_restore_file),
            music_round_art_rotate: bool_or(
                field(&v, "music_round_art_rotate"),
                d.music_round_art_rotate,
            ),
            island_lyrics_inited: bool_or(
                field(&v, "island_lyrics_inited"),
                d.island_lyrics_inited,
            ),
            plugin_custom_values: v
                .get("plugin_custom_values")
                .and_then(Value::as_array)
                .map(|arr| {
                    let mut out = vec![String::new(); 34];
                    for (i, val) in arr.iter().take(34).enumerate() {
                        out[i] = val.as_str().unwrap_or("").to_owned();
                    }
                    out
                })
                .unwrap_or_else(|| d.plugin_custom_values.clone()),
            plugin_custom_subs: v
                .get("plugin_custom_subs")
                .and_then(Value::as_array)
                .map(|arr| {
                    let mut out = vec![String::new(); 34];
                    for (i, val) in arr.iter().take(34).enumerate() {
                        out[i] = val.as_str().unwrap_or("").to_owned();
                    }
                    out
                })
                .unwrap_or_else(|| d.plugin_custom_subs.clone()),
            plugin_title_size: f64_or(field(&v, "plugin_title_size"), d.plugin_title_size as f64)
                .clamp(6.5, 16.0) as f32,
            plugin_value_size: f64_or(field(&v, "plugin_value_size"), d.plugin_value_size as f64)
                .clamp(8.0, 24.0) as f32,
            plugin_sub_size: f64_or(field(&v, "plugin_sub_size"), d.plugin_sub_size as f64)
                .clamp(7.0, 16.0) as f32,
            brightness_page_visible: bool_or(field(&v, "brightness_page_visible"), d.brightness_page_visible),
            mod_music_enabled: bool_or(field(&v, "mod_music_enabled"), d.mod_music_enabled),
            mod_focus_health_enabled: bool_or(field(&v, "mod_focus_health_enabled"), d.mod_focus_health_enabled),
            mod_shelf_clipboard_enabled: bool_or(field(&v, "mod_shelf_clipboard_enabled"), d.mod_shelf_clipboard_enabled),
            mod_mail_calendar_enabled: bool_or(field(&v, "mod_mail_calendar_enabled"), d.mod_mail_calendar_enabled),
            mod_ai_suite_enabled: bool_or(field(&v, "mod_ai_suite_enabled"), d.mod_ai_suite_enabled),
            mod_widgets_sysmon_enabled: bool_or(field(&v, "mod_widgets_sysmon_enabled"), d.mod_widgets_sysmon_enabled),
            mod_weather_translate_enabled: bool_or(field(&v, "mod_weather_translate_enabled"), d.mod_weather_translate_enabled),
            mod_display_bt_enabled: bool_or(field(&v, "mod_display_bt_enabled"), d.mod_display_bt_enabled),
            mod_focus_enabled: bool_or(field(&v, "mod_focus_enabled"), bool_or(field(&v, "mod_focus_health_enabled"), true)),
            mod_health_enabled: bool_or(field(&v, "mod_health_enabled"), bool_or(field(&v, "mod_focus_health_enabled"), true)),
            mod_shelf_enabled: bool_or(field(&v, "mod_shelf_enabled"), bool_or(field(&v, "mod_shelf_clipboard_enabled"), true)),
            mod_clipboard_enabled: bool_or(field(&v, "mod_clipboard_enabled"), bool_or(field(&v, "mod_shelf_clipboard_enabled"), true)),
            mod_mail_enabled: bool_or(field(&v, "mod_mail_enabled"), bool_or(field(&v, "mod_mail_calendar_enabled"), true)),
            mod_calendar_enabled: bool_or(field(&v, "mod_calendar_enabled"), bool_or(field(&v, "mod_mail_calendar_enabled"), true)),
            mod_course_enabled: bool_or(field(&v, "mod_course_enabled"), true),
            mod_ai_status_enabled: bool_or(field(&v, "mod_ai_status_enabled"), bool_or(field(&v, "mod_ai_suite_enabled"), true)),
            mod_ai_quota_enabled: bool_or(field(&v, "mod_ai_quota_enabled"), bool_or(field(&v, "mod_ai_suite_enabled"), true)),
            mod_weather_enabled: bool_or(field(&v, "mod_weather_enabled"), bool_or(field(&v, "mod_weather_translate_enabled"), true)),
            mod_translate_enabled: bool_or(field(&v, "mod_translate_enabled"), bool_or(field(&v, "mod_weather_translate_enabled"), true)),
            mod_display_brightness_enabled: bool_or(field(&v, "mod_display_brightness_enabled"), bool_or(field(&v, "mod_display_bt_enabled"), true)),
            mod_bt_devices_enabled: bool_or(field(&v, "mod_bt_devices_enabled"), bool_or(field(&v, "mod_display_bt_enabled"), true)),
            mod_oppo_connect_enabled: bool_or(field(&v, "mod_oppo_connect_enabled"), true),
            auto_check_updates: bool_or(field(&v, "auto_check_updates"), d.auto_check_updates),
            update_check_interval_hours: i64_or(field(&v, "update_check_interval_hours"), d.update_check_interval_hours as i64).clamp(1, 720) as u32,
            ai_island_scheme: i64_or(field(&v, "ai_island_scheme"), d.ai_island_scheme as i64).clamp(1, 2) as u8,
            fly_claude_sessions: bool_or(field(&v, "fly_claude_sessions"), d.fly_claude_sessions),
            fly_codex_sessions: bool_or(field(&v, "fly_codex_sessions"), d.fly_codex_sessions),
            fly_dsh_sessions: bool_or(field(&v, "fly_dsh_sessions"), d.fly_dsh_sessions),
            fly_doubao_sessions: bool_or(field(&v, "fly_doubao_sessions"), d.fly_doubao_sessions),
            fly_antigravity_sessions: bool_or(field(&v, "fly_antigravity_sessions"), d.fly_antigravity_sessions),
            fly_workbuddy_sessions: bool_or(field(&v, "fly_workbuddy_sessions"), d.fly_workbuddy_sessions),
            fly_grokbot_sessions: bool_or(field(&v, "fly_grokbot_sessions"), d.fly_grokbot_sessions),
            fly_cursor_sessions: bool_or(field(&v, "fly_cursor_sessions"), d.fly_cursor_sessions),
            fly_trae_sessions: bool_or(field(&v, "fly_trae_sessions"), d.fly_trae_sessions),
            fly_muse_sessions: bool_or(field(&v, "fly_muse_sessions"), d.fly_muse_sessions),
            fly_dot_sessions: bool_or(field(&v, "fly_dot_sessions"), d.fly_dot_sessions),
            fly_demo_sessions: false,
            auralsle_music_layout: str_or(field(&v, "auralsle_music_layout"), &d.auralsle_music_layout),
            auralsle_auto_compact_width: bool_or(field(&v, "auralsle_auto_compact_width"), d.auralsle_auto_compact_width),
            lyric_island_width: i64_or(field(&v, "lyric_island_width"), d.lyric_island_width as i64).clamp(200, 440) as u16,
            oppo_drag_transfer_enabled: bool_or(field(&v, "oppo_drag_transfer_enabled"), d.oppo_drag_transfer_enabled),
            auralsle_compact_width: f64_or(field(&v, "auralsle_compact_width"), d.auralsle_compact_width as f64).clamp(140.0, 480.0) as f32,
            auralsle_compact_max_width: f64_or(field(&v, "auralsle_compact_max_width"), d.auralsle_compact_max_width as f64).clamp(140.0, 480.0) as f32,
            auralsle_lyric_font_size: f64_or(field(&v, "auralsle_lyric_font_size"), d.auralsle_lyric_font_size as f64).clamp(11.0, 20.0) as f32,
            auralsle_word_highlight: bool_or(field(&v, "auralsle_word_highlight"), d.auralsle_word_highlight),
            auralsle_compact_translation: bool_or(field(&v, "auralsle_compact_translation"), d.auralsle_compact_translation),
            auralsle_chinese_two_line: bool_or(field(&v, "auralsle_chinese_two_line"), d.auralsle_chinese_two_line),
            auralsle_show_translation: bool_or(field(&v, "auralsle_show_translation"), d.auralsle_show_translation),
            auralsle_translation_button: bool_or(field(&v, "auralsle_translation_button"), d.auralsle_translation_button),
            auralsle_color_source: str_or(field(&v, "auralsle_color_source"), &d.auralsle_color_source),
            auralsle_dynamic_seed: str_or(field(&v, "auralsle_dynamic_seed"), &d.auralsle_dynamic_seed),
            auralsle_capsule_dynamic_color: bool_or(field(&v, "auralsle_capsule_dynamic_color"), d.auralsle_capsule_dynamic_color),
            auralsle_expanded_dynamic_color: bool_or(field(&v, "auralsle_expanded_dynamic_color"), d.auralsle_expanded_dynamic_color),
            auralsle_lyric_cover_color: bool_or(field(&v, "auralsle_lyric_cover_color"), d.auralsle_lyric_cover_color),
            auralsle_cover_overlay: bool_or(field(&v, "auralsle_cover_overlay"), d.auralsle_cover_overlay),
            salt_flow_bg: bool_or(field(&v, "salt_flow_bg"), d.salt_flow_bg),
            auralsle_text_color: str_or(field(&v, "auralsle_text_color"), &d.auralsle_text_color),
            auralsle_component_color: str_or(field(&v, "auralsle_component_color"), &d.auralsle_component_color),
            auralsle_lyric_progress_color: str_or(field(&v, "auralsle_lyric_progress_color"), &d.auralsle_lyric_progress_color),
            auralsle_card_material: str_or(field(&v, "auralsle_card_material"), &d.auralsle_card_material),
            auralsle_acrylic_frost: f64_or(field(&v, "auralsle_acrylic_frost"), d.auralsle_acrylic_frost as f64).clamp(0.0, 100.0) as f32,
            auralsle_acrylic_opacity: f64_or(field(&v, "auralsle_acrylic_opacity"), d.auralsle_acrylic_opacity as f64).clamp(0.0, 100.0) as f32,
            auralsle_bar_style: str_or(field(&v, "auralsle_bar_style"), &d.auralsle_bar_style),
            auralsle_thumb_style: str_or(field(&v, "auralsle_thumb_style"), &d.auralsle_thumb_style),
            auralsle_thumb_enabled: bool_or(field(&v, "auralsle_thumb_enabled"), d.auralsle_thumb_enabled),
            auralsle_thumb_spin: bool_or(field(&v, "auralsle_thumb_spin"), d.auralsle_thumb_spin),
            auralsle_transport_theme: str_or(field(&v, "auralsle_transport_theme"), &d.auralsle_transport_theme),
            auralsle_music_glow: bool_or(field(&v, "auralsle_music_glow"), d.auralsle_music_glow),
            auralsle_music_glow_mode: str_or(field(&v, "auralsle_music_glow_mode"), &d.auralsle_music_glow_mode),
            auralsle_click_open_player: bool_or(field(&v, "auralsle_click_open_player"), d.auralsle_click_open_player),
            auralsle_hover_tooltip: bool_or(field(&v, "auralsle_hover_tooltip"), d.auralsle_hover_tooltip),
            auralsle_custom_layout_json: str_or(field(&v, "auralsle_custom_layout_json"), &d.auralsle_custom_layout_json),
            auralsle_otp_capsule: bool_or(field(&v, "auralsle_otp_capsule"), d.auralsle_otp_capsule),
        }
    }

    pub fn save(&self) {
        let mut v = json!({
            "v": CONFIG_VERSION,
            "enabled": self.enabled,
            "compact": self.compact,
            "details": self.details,
            "active_timeout_secs": self.active_timeout_secs,
            "done_timeout_secs": self.done_timeout_secs,
            "return_button": self.return_button,
            "status_dir": self.status_dir,
            "open_target": self.open_target,
            "app_paths": self
                .app_paths
                .iter()
                .map(|(k, v)| (k.clone(), Value::String(v.clone())))
                .collect::<serde_json::Map<String, Value>>(),
            "outline": self.outline,
            "course_enabled": self.course_enabled,
            "course_remind_minutes": self.course_remind_minutes,
            "course_file": self.course_file,
            "course_xlsx": self.course_xlsx,
            "course_start": self.course_start,
            "aora_face_style": self.aora_face_style,
        });
        let tail = json!({
            "course_template_v38": true,
            "course_style": self.course_style,
            "pill_style": self.pill_style,
            "pill_placement": self.pill_placement,
            "course_layout": self.course_layout,
            "course_show_sep": self.course_show_sep,
            "course_title_size": self.course_title_size,
            "course_place_size": self.course_place_size,
            "course_time_size": self.course_time_size,
            "course_small_size": self.course_small_size,
            "course_letter_spacing": self.course_letter_spacing,
            "course_time_color": self.course_time_color,
            "course_title_color": self.course_title_color,
            "course_place_color": self.course_place_color,
            "course_accent_color": self.course_accent_color,
            "course_dnd_action": self.course_dnd_action,
            "course_countdown_style": self.course_countdown_style,
            "course_dnd_label": self.course_dnd_label,
            "music_pause_return_secs": self.music_pause_return_secs,
            "islo_face": self.islo_face,
            "islo_fixed": self.islo_fixed,
            "aora_style": self.aora_style,
            "aora_shape": self.aora_shape,
            "aora_color": self.aora_color,
            "aora_scale": self.aora_scale,
            "monitor_plugin": self.monitor_plugin,
            "monitor_cpu": self.monitor_cpu,
            "monitor_gpu": self.monitor_gpu,
            "monitor_mem": self.monitor_mem,
            "monitor_disk": self.monitor_disk,
            "monitor_net": self.monitor_net,
            "monitor_battery": self.monitor_battery,
            "monitor_cores": self.monitor_cores,
            "monitor_charts": self.monitor_charts,
            "monitor_interval": self.monitor_interval,
        });
        if let (Value::Object(head), Value::Object(tail)) = (&mut v, tail) {
            head.extend(tail);
            head.insert("aora_custom_fill_enabled".to_owned(), json!(self.aora_custom_fill_enabled));
            head.insert("aora_custom_fill_rgb".to_owned(), json!(self.aora_custom_fill_rgb));
            head.insert("aora_custom_outline_enabled".to_owned(), json!(self.aora_custom_outline_enabled));
            head.insert("aora_custom_outline_rgb".to_owned(), json!(self.aora_custom_outline_rgb));
            head.insert("face_fill_alpha".to_owned(), json!(self.face_fill_alpha));
            head.insert("face_stroke_alpha".to_owned(), json!(self.face_stroke_alpha));
            head.insert("msg_island_filter".to_owned(), json!(self.msg_island_filter));
            head.insert("minimal_time_pill".to_owned(), json!(self.minimal_time_pill));
            head.insert("temp_avoid_enabled".to_owned(), json!(self.temp_avoid_enabled));
            head.insert("temp_avoid_fullscreen".to_owned(), json!(self.temp_avoid_fullscreen));
            head.insert("temp_avoid_show_time".to_owned(), json!(self.temp_avoid_show_time));
            head.insert("msg_hub_order".to_owned(), json!(self.msg_hub_order));
            head.insert("clipboard_page_visible".to_owned(), json!(self.clipboard_page_visible));
            head.insert("clip_filter_tab".to_owned(), json!(self.clip_filter_tab));
            head.insert("clip_search_query".to_owned(), json!(self.clip_search_query));
            head.insert("clip_notes".to_owned(), Value::Object(self.clip_notes.iter().map(|(k, val)| (k.clone(), Value::String(val.clone()))).collect()));
            head.insert("todo_sub_tab".to_owned(), json!(self.todo_sub_tab));
            head.insert("nav_bar_style".to_owned(), json!(self.nav_bar_style));
            head.insert("nav_badge_pos".to_owned(), json!(self.nav_badge_pos));
            head.insert("pet_user_configured".to_owned(), json!(self.pet_user_configured));
            head.insert("minimal_pill_width".to_owned(), json!(self.minimal_pill_width));
            head.insert("custom_font_path".to_owned(), json!(self.custom_font_path));
            head.insert("minimal_pill_msg_badge".to_owned(), json!(self.minimal_pill_msg_badge));
            head.insert("pill_show_music".to_owned(), json!(self.pill_show_music));
            head.insert("music_show_ai_status".to_owned(), json!(self.music_show_ai_status));
            head.insert("lyric_island_time_capsule".to_owned(), json!(self.lyric_island_time_capsule));
            head.insert("hide_nav_arrows".to_owned(), json!(self.hide_nav_arrows));
            head.insert("pill_show_focus".to_owned(), json!(self.pill_show_focus));
            head.insert("pill_show_quota".to_owned(), json!(self.pill_show_quota));
            head.insert("copy_auto_open_clipboard".to_owned(), json!(self.copy_auto_open_clipboard));
            head.insert("copy_auto_open_secs".to_owned(), json!(self.copy_auto_open_secs));
            head.insert("plugin_ui_mode".to_owned(), json!(self.plugin_ui_mode));
            head.insert("show_island_page_title".to_owned(), json!(self.show_island_page_title));
            head.insert("clipboard_ui_mode".to_owned(), json!(self.clipboard_ui_mode));
            head.insert("clipboard_auto_paste".to_owned(), json!(self.clipboard_auto_paste));
            head.insert("clipboard_close_mode".to_owned(), json!(self.clipboard_close_mode));
            head.insert("clipboard_close_delay_secs".to_owned(), json!(self.clipboard_close_delay_secs));
            head.insert("device_display_mode".to_owned(), json!(self.device_display_mode));
            head.insert("glance_music_form".to_owned(), json!(self.glance_music_form));
            head.insert("glance_widget_form".to_owned(), json!(self.glance_widget_form));
            head.insert("device_page_visible".to_owned(), json!(self.device_page_visible));
            head.insert(
                "device_auto_hide_when_empty".to_owned(),
                json!(self.device_auto_hide_when_empty),
            );
            head.insert(
                "show_unread_badge_count".to_owned(),
                json!(self.show_unread_badge_count),
            );
            head.insert(
                "aora_custom_ink_enabled".to_owned(),
                json!(self.aora_custom_ink_enabled),
            );
            head.insert(
                "aora_custom_ink_rgb".to_owned(),
                json!(self.aora_custom_ink_rgb),
            );
            head.insert(
                "aora_custom_ink_alpha".to_owned(),
                json!(self.aora_custom_ink_alpha),
            );
            head.insert(
                "show_unread_badge_count".to_owned(),
                json!(self.show_unread_badge_count),
            );
            head.insert(
                "aora_custom_ink_enabled".to_owned(),
                json!(self.aora_custom_ink_enabled),
            );
            head.insert(
                "aora_custom_ink_rgb".to_owned(),
                json!(self.aora_custom_ink_rgb),
            );
            head.insert(
                "aora_custom_ink_alpha".to_owned(),
                json!(self.aora_custom_ink_alpha),
            );
            head.insert("full_page_order".to_owned(), json!(self.full_page_order));
            head.insert("classic_music_cover_tint".to_owned(), json!(self.classic_music_cover_tint));
            head.insert("custom_backup_dir".to_owned(), json!(self.custom_backup_dir));
            head.insert("custom_restore_file".to_owned(), json!(self.custom_restore_file));
            head.insert("music_round_art_rotate".to_owned(), json!(self.music_round_art_rotate));
            head.insert("island_lyrics_inited".to_owned(), json!(self.island_lyrics_inited));
            head.insert("plugin_custom_values".to_owned(), json!(self.plugin_custom_values));
            head.insert("plugin_custom_subs".to_owned(), json!(self.plugin_custom_subs));
            head.insert("plugin_title_size".to_owned(), json!(self.plugin_title_size));
            head.insert("plugin_value_size".to_owned(), json!(self.plugin_value_size));
            head.insert("plugin_sub_size".to_owned(), json!(self.plugin_sub_size));
            head.insert("brightness_page_visible".to_owned(), json!(self.brightness_page_visible));
            head.insert("mod_music_enabled".to_owned(), json!(self.mod_music_enabled));
            head.insert("mod_focus_health_enabled".to_owned(), json!(self.mod_focus_health_enabled));
            head.insert("mod_shelf_clipboard_enabled".to_owned(), json!(self.mod_shelf_clipboard_enabled));
            head.insert("mod_mail_calendar_enabled".to_owned(), json!(self.mod_mail_calendar_enabled));
            head.insert("mod_ai_suite_enabled".to_owned(), json!(self.mod_ai_suite_enabled));
            head.insert("mod_widgets_sysmon_enabled".to_owned(), json!(self.mod_widgets_sysmon_enabled));
            head.insert("mod_weather_translate_enabled".to_owned(), json!(self.mod_weather_translate_enabled));
            head.insert("mod_display_bt_enabled".to_owned(), json!(self.mod_display_bt_enabled));
            head.insert("mod_focus_enabled".to_owned(), json!(self.mod_focus_enabled));
            head.insert("mod_health_enabled".to_owned(), json!(self.mod_health_enabled));
            head.insert("mod_shelf_enabled".to_owned(), json!(self.mod_shelf_enabled));
            head.insert("mod_clipboard_enabled".to_owned(), json!(self.mod_clipboard_enabled));
            head.insert("mod_mail_enabled".to_owned(), json!(self.mod_mail_enabled));
            head.insert("mod_calendar_enabled".to_owned(), json!(self.mod_calendar_enabled));
            head.insert("mod_course_enabled".to_owned(), json!(self.mod_course_enabled));
            head.insert("mod_ai_status_enabled".to_owned(), json!(self.mod_ai_status_enabled));
            head.insert("mod_ai_quota_enabled".to_owned(), json!(self.mod_ai_quota_enabled));
            head.insert("mod_weather_enabled".to_owned(), json!(self.mod_weather_enabled));
            head.insert("mod_translate_enabled".to_owned(), json!(self.mod_translate_enabled));
            head.insert("mod_display_brightness_enabled".to_owned(), json!(self.mod_display_brightness_enabled));
            head.insert("mod_bt_devices_enabled".to_owned(), json!(self.mod_bt_devices_enabled));
            head.insert("mod_oppo_connect_enabled".to_owned(), json!(self.mod_oppo_connect_enabled));
            head.insert("auto_check_updates".to_owned(), json!(self.auto_check_updates));
            head.insert("update_check_interval_hours".to_owned(), json!(self.update_check_interval_hours));
            head.insert("ai_island_scheme".to_owned(), json!(self.ai_island_scheme));
            head.insert("fly_claude_sessions".to_owned(), json!(self.fly_claude_sessions));
            head.insert("fly_codex_sessions".to_owned(), json!(self.fly_codex_sessions));
            head.insert("fly_dsh_sessions".to_owned(), json!(self.fly_dsh_sessions));
            head.insert("fly_doubao_sessions".to_owned(), json!(self.fly_doubao_sessions));
            head.insert("fly_antigravity_sessions".to_owned(), json!(self.fly_antigravity_sessions));
            head.insert("fly_workbuddy_sessions".to_owned(), json!(self.fly_workbuddy_sessions));
            head.insert("fly_grokbot_sessions".to_owned(), json!(self.fly_grokbot_sessions));
            head.insert("fly_cursor_sessions".to_owned(), json!(self.fly_cursor_sessions));
            head.insert("fly_trae_sessions".to_owned(), json!(self.fly_trae_sessions));
            head.insert("fly_muse_sessions".to_owned(), json!(self.fly_muse_sessions));
            head.insert("fly_dot_sessions".to_owned(), json!(self.fly_dot_sessions));
            head.insert("fly_demo_sessions".to_owned(), json!(self.fly_demo_sessions));
            head.insert("border_effect".to_owned(), json!(self.border_effect));
            head.insert("border_plays_on".to_owned(), json!(self.border_plays_on));
            head.insert("border_interval".to_owned(), json!(self.border_interval));
            head.insert("liquid_glass_mode".to_owned(), json!(self.liquid_glass_mode));
            head.insert("lg_refract".to_owned(), json!(self.lg_refract));
            head.insert("lg_chroma".to_owned(), json!(self.lg_chroma));
            head.insert("lg_blur".to_owned(), json!(self.lg_blur));
            head.insert("lg_band".to_owned(), json!(self.lg_band));
            head.insert("lg_edge_width".to_owned(), json!(self.lg_edge_width));
            head.insert("lg_edge_opacity".to_owned(), json!(self.lg_edge_opacity));
            head.insert("lg_morph".to_owned(), json!(self.lg_morph));
            head.insert("music_ui_style".to_owned(), json!(self.music_ui_style));
            head.insert("music_auto_hide_when_idle".to_owned(), json!(self.music_auto_hide_when_idle));
            head.insert("win_theme".to_owned(), json!(self.win_theme));
            head.insert("custom_accent".to_owned(), json!(self.custom_accent));
            head.insert("glass_opacity".to_owned(), json!(self.glass_opacity));
            head.insert("fps_120".to_owned(), json!(self.fps_120));
            head.insert("touch_gestures".to_owned(), json!(self.touch_gestures));
            head.insert("win_compact_bar".to_owned(), json!(self.win_compact_bar));
            head.insert("win_widgets_order".to_owned(), json!(self.win_widgets_order));
            head.insert("weather_page_visible".to_owned(), json!(self.weather_page_visible));
            head.insert("translate_page_visible".to_owned(), json!(self.translate_page_visible));
            head.insert("weather_city".to_owned(), json!(self.weather_city));
            head.insert("weather_street".to_owned(), json!(self.weather_street));
            head.insert("weather_lat".to_owned(), json!(self.weather_lat));
            head.insert("weather_lon".to_owned(), json!(self.weather_lon));
            head.insert("weather_temp".to_owned(), json!(self.weather_temp));
            head.insert("weather_desc".to_owned(), json!(self.weather_desc));
            head.insert("weather_source".to_owned(), json!(self.weather_source));
            head.insert("weather_custom_name".to_owned(), json!(self.weather_custom_name));
            head.insert("weather_custom_url".to_owned(), json!(self.weather_custom_url));
            head.insert("weather_api_key".to_owned(), json!(self.weather_api_key));
            head.insert("weather_refresh_mins".to_owned(), json!(self.weather_refresh_mins));
            head.insert("weather_show_hourly".to_owned(), json!(self.weather_show_hourly));
            head.insert("weather_show_daily".to_owned(), json!(self.weather_show_daily));
            head.insert("weather_show_aqi".to_owned(), json!(self.weather_show_aqi));
            head.insert("weather_show_wind".to_owned(), json!(self.weather_show_wind));
            head.insert("weather_show_uv_hum".to_owned(), json!(self.weather_show_uv_hum));
            head.insert("weather_show_sun".to_owned(), json!(self.weather_show_sun));
            head.insert("translate_engine".to_owned(), json!(self.translate_engine));
            head.insert("translate_auto_clipboard".to_owned(), json!(self.translate_auto_clipboard));
            head.insert("translate_auto_copy".to_owned(), json!(self.translate_auto_copy));
            head.insert("translate_pin_island".to_owned(), json!(self.translate_pin_island));
            head.insert("course_cell_h".to_owned(), json!(self.course_cell_h));
            head.insert("course_corner_r".to_owned(), json!(self.course_corner_r));
            head.insert("course_card_alpha".to_owned(), json!(self.course_card_alpha));
            head.insert("course_show_non_current".to_owned(), json!(self.course_show_non_current));
            head.insert("course_sunday_first".to_owned(), json!(self.course_sunday_first));
            head.insert("course_holiday_dnd".to_owned(), json!(self.course_holiday_dnd));
            head.insert("fluid_cover_tint".to_owned(), json!(self.fluid_cover_tint));
            head.insert("ai_mascot_link".to_owned(), json!(self.ai_mascot_link));
            head.insert("ai_mascot_mode".to_owned(), json!(self.ai_mascot_mode));
            head.insert("ai_mascot_interval".to_owned(), json!(self.ai_mascot_interval));
            head.insert("plugin_island_h".to_owned(), json!(self.plugin_island_h));
            head.insert("plugin_cols".to_owned(), json!(self.plugin_cols));
            head.insert("plugin_card_radius".to_owned(), json!(self.plugin_card_radius));
            head.insert("plugin_card_opacity".to_owned(), json!(self.plugin_card_opacity));
            head.insert("plugin_font_scale".to_owned(), json!(self.plugin_font_scale));
            head.insert("plugin_show_title".to_owned(), json!(self.plugin_show_title));
            head.insert("plugin_show_value".to_owned(), json!(self.plugin_show_value));
            head.insert("plugin_show_sub".to_owned(), json!(self.plugin_show_sub));
            head.insert("plugin_show_bar".to_owned(), json!(self.plugin_show_bar));
            head.insert("plugin_sizes".to_owned(), json!(self.plugin_sizes));
            head.insert("gpu_backend".to_owned(), json!(self.gpu_backend));
            head.insert("battery_style".to_owned(), json!(self.battery_style));
            head.insert("wechat_read_content".to_owned(), json!(self.wechat_read_content));
            head.insert("wechat_camera_guard".to_owned(), json!(self.wechat_camera_guard));
            head.insert("wechat_fingerprint_lock".to_owned(), json!(self.wechat_fingerprint_lock));
            head.insert("wechat_reveal_hotkey".to_owned(), json!(self.wechat_reveal_hotkey));
            head.insert("oppo_wechat_dedup".to_owned(), json!(self.oppo_wechat_dedup));
            head.insert("oppo_notify_mirror_enabled".to_owned(), json!(self.oppo_notify_mirror_enabled));
            head.insert("oppo_suppress_native".to_owned(), json!(self.oppo_suppress_native));
            head.insert("translate_ai_provider".to_owned(), json!(self.translate_ai_provider));
            head.insert("translate_ai_base_url".to_owned(), json!(self.translate_ai_base_url));
            head.insert("translate_ai_api_key".to_owned(), json!(self.translate_ai_api_key));
            head.insert("translate_ai_model".to_owned(), json!(self.translate_ai_model));
            head.insert("translate_ai_prompt".to_owned(), json!(self.translate_ai_prompt));
            head.insert("course_show_weekend".to_owned(), json!(self.course_show_weekend));
            head.insert("page_switch_anim".to_owned(), json!(self.page_switch_anim));
            head.insert("plugin_card_styles".to_owned(), json!(self.plugin_card_styles));
            head.insert("plugin_custom_titles".to_owned(), json!(self.plugin_custom_titles));
            head.insert("show_bt_battery_on_island".to_owned(), json!(self.show_bt_battery_on_island));
            head.insert("voltide_enabled".to_owned(), json!(self.voltide_enabled));
            head.insert("island_pos_locked".to_owned(), json!(self.island_pos_locked));
            head.insert("expanded_w_scale".to_owned(), json!(self.expanded_w_scale));
            head.insert("expanded_h_scale".to_owned(), json!(self.expanded_h_scale));
            head.insert("agent_status_lamp".to_owned(), json!(self.agent_status_lamp));
            head.insert("compact_max_count".to_owned(), json!(self.compact_max_count.clamp(1, 5)));
            head.insert("autostart_mode".to_owned(), json!(self.autostart_mode.min(2)));
            head.insert("agent_hook_approval".to_owned(), json!(self.agent_hook_approval));
            head.insert("weather_ui_mode".to_owned(), json!(self.weather_ui_mode));
            head.insert("messenger_mode".to_owned(), json!(self.messenger_mode));
            head.insert("quota_ui_mode".to_owned(), json!(self.quota_ui_mode));
            head.insert("auralsle_music_layout".to_owned(), json!(self.auralsle_music_layout));
            head.insert("auralsle_auto_compact_width".to_owned(), json!(self.auralsle_auto_compact_width));
            head.insert("lyric_island_width".to_owned(), json!(self.lyric_island_width.clamp(200, 440)));
            head.insert("oppo_drag_transfer_enabled".to_owned(), json!(self.oppo_drag_transfer_enabled));
            head.insert("auralsle_compact_width".to_owned(), json!(self.auralsle_compact_width));
            head.insert("auralsle_compact_max_width".to_owned(), json!(self.auralsle_compact_max_width));
            head.insert("auralsle_lyric_font_size".to_owned(), json!(self.auralsle_lyric_font_size));
            head.insert("auralsle_word_highlight".to_owned(), json!(self.auralsle_word_highlight));
            head.insert("auralsle_compact_translation".to_owned(), json!(self.auralsle_compact_translation));
            head.insert("auralsle_chinese_two_line".to_owned(), json!(self.auralsle_chinese_two_line));
            head.insert("auralsle_show_translation".to_owned(), json!(self.auralsle_show_translation));
            head.insert("auralsle_translation_button".to_owned(), json!(self.auralsle_translation_button));
            head.insert("auralsle_color_source".to_owned(), json!(self.auralsle_color_source));
            head.insert("auralsle_dynamic_seed".to_owned(), json!(self.auralsle_dynamic_seed));
            head.insert("auralsle_capsule_dynamic_color".to_owned(), json!(self.auralsle_capsule_dynamic_color));
            head.insert("auralsle_expanded_dynamic_color".to_owned(), json!(self.auralsle_expanded_dynamic_color));
            head.insert("auralsle_lyric_cover_color".to_owned(), json!(self.auralsle_lyric_cover_color));
            head.insert("auralsle_cover_overlay".to_owned(), json!(self.auralsle_cover_overlay));
            head.insert("salt_flow_bg".to_owned(), json!(self.salt_flow_bg));
            head.insert("auralsle_text_color".to_owned(), json!(self.auralsle_text_color));
            head.insert("auralsle_component_color".to_owned(), json!(self.auralsle_component_color));
            head.insert("auralsle_lyric_progress_color".to_owned(), json!(self.auralsle_lyric_progress_color));
            head.insert("auralsle_card_material".to_owned(), json!(self.auralsle_card_material));
            head.insert("auralsle_acrylic_frost".to_owned(), json!(self.auralsle_acrylic_frost));
            head.insert("auralsle_acrylic_opacity".to_owned(), json!(self.auralsle_acrylic_opacity));
            head.insert("auralsle_bar_style".to_owned(), json!(self.auralsle_bar_style));
            head.insert("auralsle_thumb_style".to_owned(), json!(self.auralsle_thumb_style));
            head.insert("auralsle_thumb_enabled".to_owned(), json!(self.auralsle_thumb_enabled));
            head.insert("auralsle_thumb_spin".to_owned(), json!(self.auralsle_thumb_spin));
            head.insert("auralsle_transport_theme".to_owned(), json!(self.auralsle_transport_theme));
            head.insert("auralsle_music_glow".to_owned(), json!(self.auralsle_music_glow));
            head.insert("auralsle_music_glow_mode".to_owned(), json!(self.auralsle_music_glow_mode));
            head.insert("auralsle_click_open_player".to_owned(), json!(self.auralsle_click_open_player));
            head.insert("auralsle_hover_tooltip".to_owned(), json!(self.auralsle_hover_tooltip));
            head.insert("auralsle_custom_layout_json".to_owned(), json!(self.auralsle_custom_layout_json));
            head.insert("auralsle_otp_capsule".to_owned(), json!(self.auralsle_otp_capsule));
        }
        let path = config_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if fs::write(&tmp, serde_json::to_string_pretty(&v).unwrap_or_default()).is_ok() {
            let _ = fs::rename(&tmp, &path);
        }
    }

    pub fn status_dir(&self) -> PathBuf {
        if cfg!(test) {
            return PathBuf::new();
        }
        if !self.status_dir.trim().is_empty() {
            return PathBuf::from(self.status_dir.trim());
        }
        let preferred = install_dir().join("ai-status");
        if has_status_files(&preferred) {
            return preferred;
        }
        let appdata_dir = crate::model::config_dir().join("ai-status");
        if has_status_files(&appdata_dir) {
            return appdata_dir;
        }
        if let Some(old) = legacy_status_dir() {
            if has_status_files(&old) {
                return old;
            }
        }
        preferred
    }

    /// 返回本机所有可能写入 AI 状态的目录列表（自定义目录、安装目录、AppData 目录、ZenDesktop 兼容目录），
    /// 解决不同 AI Agent 工具写往不同目录导致状态失联的问题。
    pub fn all_status_dirs(&self) -> Vec<PathBuf> {
        if cfg!(test) {
            return Vec::new();
        }
        let mut dirs = Vec::new();
        let mut push_unique = |p: PathBuf| {
            if !p.as_os_str().is_empty() && !dirs.iter().any(|existing: &PathBuf| existing.to_string_lossy().eq_ignore_ascii_case(&p.to_string_lossy())) {
                dirs.push(p);
            }
        };
        if !self.status_dir.trim().is_empty() {
            push_unique(PathBuf::from(self.status_dir.trim()));
        }
        push_unique(install_dir().join("ai-status"));
        push_unique(crate::model::config_dir().join("ai-status"));
        if let Some(old) = legacy_status_dir() {
            push_unique(old);
        }
        dirs
    }

    pub fn set_app_path(&mut self, key: &str, exe: &str) {
        if let Some(entry) = self.app_paths.iter_mut().find(|(k, _)| k == key) {
            entry.1 = exe.to_owned();
        } else {
            self.app_paths.push((key.to_owned(), exe.to_owned()));
        }
    }

    pub fn app_path(&self, key: &str) -> Option<&str> {
        self.app_paths
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, p)| p.as_str())
    }
}

pub fn ensure_toolchain() {
    if cfg!(test) {
        return;
    }
    let dir = tools_dir();
    if fs::create_dir_all(&dir).is_err() {
        return;
    }
    let _ = fs::create_dir_all(install_dir().join("ai-status"));
    let put = |name: &str, body: &str| {
        let p = dir.join(name);
        if !p.exists() {
            let _ = fs::write(p, body);
        }
    };
    let put_always = |name: &str, body: &str| {
        let p = dir.join(name);
        let _ = fs::write(p, body);
    };
    let d = dir.to_string_lossy().replace('\\', "/");
    let mcp = json!({
        "mcpServers": {
            "zen-status": {
                "command": "powershell.exe",
                "args": ["-NoProfile", "-ExecutionPolicy", "Bypass", "-File",
                         format!("{d}/wb-hook.ps1"), "-Agent", "workbuddy", "-Name", "WorkBuddy"]
            }
        }
    });
    put("mcp.json", &(serde_json::to_string_pretty(&mcp).unwrap_or_default() + "\n"));
    put_always("wb-hook.ps1", include_str!("assets/wb-hook.ps1"));
    put_always("fly-agent-hook.ps1", include_str!("assets/wb-hook.ps1"));
    put("README.txt", &format!(
        "灵屿 AuraIsle · AI 工作状态检测（MCP / 钩子）\n\
         --------------------------------------\n\
         1) 状态文件默认目录：{status}\n\
            旧位置 %LOCALAPPDATA%\\ZenDesktop\\ai-status 会自动兼容；\n\
            也可在「设置 → AI 工作 → 状态来源」手动指定任意目录。\n\
         2) 把 zen-status.exe 与 zen-status-mcp.ps1 放到本目录，\n\
            wb-hook.ps1 优先使用本目录的 exe，找不到才回退到旧工具目录。\n\
         3) mcp.json 是 MCP 注册模板：把 mcpServers 节点合并到\n\
            %USERPROFILE%\\.workbuddy\\mcp.json，再到自定义连接器里信任 zen-status。\n\
         4) 钩子需要用户自行启用（本目录不会自动写 hooks 事件绑定）：\n\
            运行 node mod/tools/install-hook.mjs --install-dir <灵屿 AuraIsle 安装目录>，\n\
            或手动把 settings.json 里 zen-status 的 hook command 指向本目录的 wb-hook.ps1。\n\
            安装脚本会先备份 settings.json，--revert 可还原。\n\
         5) 本目录文件只在缺失时生成，用户改动不会被覆盖。\n",
        status = install_dir().join("ai-status").display()
    ));
}

static CONFIG: OnceLock<Mutex<Config>> = OnceLock::new();
static CONFIG_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub fn revision() -> u64 {
    CONFIG_GEN.load(std::sync::atomic::Ordering::Relaxed)
}

fn lock() -> &'static Mutex<Config> {
    CONFIG.get_or_init(|| {
        ensure_toolchain();
        Mutex::new(Config::load())
    })
}

/// 后台异步落盘通道（避免在 UI 线程点击开关或拖动滑块时同步阻塞文件 IO，彻底解决设置页卡顿掉帧）。
fn async_save_config(snapshot: Config) {
    if cfg!(test) {
        snapshot.save();
        return;
    }
    static TX: OnceLock<std::sync::mpsc::Sender<Config>> = OnceLock::new();
    let tx = TX.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Config>();
        let _ = std::thread::Builder::new()
            .name("auralsle-cfg-writer".into())
            .spawn(move || {
                while let Ok(mut latest) = rx.recv() {
                    // 合并短时间内连续多次滑块/开关修改，仅落盘最新一份
                    while let Ok(newer) = rx.try_recv() {
                        latest = newer;
                    }
                    latest.save();
                }
            });
        tx
    });
    let _ = tx.send(snapshot);
}

pub fn get() -> Config {
    if cfg!(test) {
        return lock().lock().map(|c| c.clone()).unwrap_or_default();
    }
    thread_local! {
        static TLS_CFG: std::cell::RefCell<(u64, Config)> = std::cell::RefCell::new((0, Config::default()));
    }
    let cur_gen = CONFIG_GEN.load(std::sync::atomic::Ordering::Relaxed);
    TLS_CFG.with(|cell| {
        let mut b = cell.borrow_mut();
        if b.0 != cur_gen {
            if let Ok(guard) = lock().lock() {
                b.1 = guard.clone();
                b.0 = cur_gen;
            }
        }
        b.1.clone()
    })
}

/// 修改配置并通知线程本地缓存更新，运行时异步写入磁盘（不阻塞 120Hz 渲染线程）。
pub fn update(f: impl FnOnce(&mut Config)) {
    if let Ok(mut c) = lock().lock() {
        f(&mut c);
        CONFIG_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let snap = c.clone();
        drop(c);
        async_save_config(snap);
    }
}

pub fn default_open_url(agent: &str) -> &'static str {
    let k = agent.to_ascii_lowercase();
    if k.contains("doubao") || agent.contains("豆包") {
        "https://www.doubao.com/chat/"
    } else if k.contains("claude") || k.contains("anthropic") {
        "https://claude.ai/new"
    } else if k.contains("codex") || k.contains("openai") || k.contains("chatgpt") {
        "https://chatgpt.com/"
    } else if k.contains("antigravity") || k == "agy" {
        "https://antigravity.google/"
    } else if k.contains("gemini") {
        "https://gemini.google.com/app"
    } else if k.contains("deepseek") || k == "dsh" {
        "https://chat.deepseek.com/"
    } else if k.contains("kimi") {
        "https://www.kimi.com/"
    } else if k.contains("grok") {
        "https://grok.com/"
    } else if k.contains("cursor") {
        "cursor://"
    } else if k.contains("windsurf") {
        "windsurf://"
    } else if k.contains("kiro") {
        "https://kiro.dev/"
    } else if k.contains("zcode") || k.contains("glm") {
        "https://bigmodel.cn/"
    } else {
        "https://www.codebuddy.cn/"
    }
}


/// 导出全量设置与数据备份（纯 Rust 多文件 `.zip` 或 `.json` v2，覆盖全部配置、图片、歌词、凭据与课表）。
pub fn export_settings_backup(dest_path: &std::path::Path) -> Result<String, String> {
    if let Some(parent) = dest_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut entries = collect_backup_entries();
    let manifest = build_backup_manifest(entries.len());
    entries.push(("__manifest__.json".to_owned(), manifest));

    let ext = dest_path
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("json")
        .to_ascii_lowercase();
    if ext == "zip" {
        let zip_bytes = encode_zip_archive(&entries)?;
        crate::model::atomic_write(dest_path, &zip_bytes)
            .map_err(|e| format!("写入 ZIP 备份失败: {e}"))?;
        let count = entries.len().saturating_sub(1);
        Ok(format!("已完整备份 {count} 个数据文件（ZIP）至 {}", dest_path.display()))
    } else {
        export_settings_json_v2(dest_path, &entries)
    }
}

fn build_backup_manifest(file_count: usize) -> Vec<u8> {
    let manifest = json!({
        "format": "auraisle-backup-v2",
        "version": env!("CARGO_PKG_VERSION"),
        "created_at": chrono::Local::now().to_rfc3339(),
        "file_count": file_count,
    });
    serde_json::to_vec_pretty(&manifest).unwrap_or_default()
}

fn should_skip_backup_dir(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    matches!(
        low.as_str(),
        "diagnostics" | "restore_tmp" | "browser-login" | "updates" | "backups"
    )
}

fn should_skip_backup_file(name: &str) -> bool {
    let low = name.to_ascii_lowercase();
    low.ends_with(".tmp")
        || low.ends_with(".log")
        || low == "auralsle-backup-snapshot.json"
        || low == "pre-restore-safety.zip"
        || ((low.starts_with("auraisle-backup") || low.starts_with("auralsle-settings-backup"))
            && (low.ends_with(".zip") || low.ends_with(".json")))
}

fn collect_dir_entries_rec(base: &Path, cur: &Path, out: &mut Vec<(String, Vec<u8>)>) {
    let Ok(rd) = fs::read_dir(cur) else { return; };
    for entry in rd.flatten() {
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(|s| s.to_owned()) else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            if cur == base && should_skip_backup_dir(&name) {
                continue;
            }
            collect_dir_entries_rec(base, &path, out);
        } else if meta.is_file() {
            if cur == base && should_skip_backup_file(&name) {
                continue;
            }
            if meta.len() > 32 * 1024 * 1024 {
                continue;
            }
            if let Ok(rel) = path.strip_prefix(base) {
                if let Ok(bytes) = fs::read(&path) {
                    let rel_str = rel.to_string_lossy().replace('\\', "/");
                    out.push((rel_str, bytes));
                }
            }
        }
    }
}

fn collect_backup_entries() -> Vec<(String, Vec<u8>)> {
    let dir = crate::model::config_dir();
    let mut entries = Vec::new();
    collect_dir_entries_rec(&dir, &dir, &mut entries);

    // 若用户通过外部 Excel 导入了课程表，额外打包副本以便跨设备迁移恢复
    let cfg = Config::load();
    let xlsx = cfg.course_xlsx.trim();
    if !xlsx.is_empty() {
        let p = Path::new(xlsx);
        if p.is_file() {
            if let Ok(meta) = p.metadata() {
                if meta.len() <= 25 * 1024 * 1024 {
                    if let (Some(fname), Ok(bytes)) = (p.file_name().and_then(|s| s.to_str()), fs::read(p)) {
                        entries.push((format!("__external__/course_xlsx/{fname}"), bytes));
                    }
                }
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn export_settings_json_v2(dest_path: &Path, entries: &[(String, Vec<u8>)]) -> Result<String, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    let mut payload = serde_json::Map::new();
    let mut count = 0usize;
    for (rel, bytes) in entries {
        if rel == "__manifest__.json" {
            continue;
        }
        let item = match std::str::from_utf8(bytes) {
            Ok(text) => json!({ "encoding": "utf8", "data": text }),
            Err(_) => json!({ "encoding": "base64", "data": B64.encode(bytes) }),
        };
        payload.insert(rel.clone(), item);
        count += 1;
    }
    let root = json!({
        "format": "auraisle-backup-v2",
        "version": env!("CARGO_PKG_VERSION"),
        "created_at": chrono::Local::now().to_rfc3339(),
        "file_count": count,
        "entries": Value::Object(payload),
    });
    let serialized = serde_json::to_vec_pretty(&root).map_err(|e| format!("序列化失败: {e}"))?;
    crate::model::atomic_write(dest_path, &serialized).map_err(|e| format!("写入备份失败: {e}"))?;
    Ok(format!("已完整备份 {count} 个数据文件（JSON）至 {}", dest_path.display()))
}

fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc ^= b as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn push_u16(buf: &mut Vec<u8>, v: u16) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn push_u32(buf: &mut Vec<u8>, v: u32) {
    buf.extend_from_slice(&v.to_le_bytes());
}

fn encode_zip_archive(entries: &[(String, Vec<u8>)]) -> Result<Vec<u8>, String> {
    use flate2::{write::DeflateEncoder, Compression};
    use std::io::Write;

    let mut out = Vec::new();
    let mut cd = Vec::new();
    let dos_time: u16 = 0;
    let dos_date: u16 = 0x5421; // 2022-01-01 valid MS-DOS date

    for (name, raw) in entries {
        let name_bytes = name.as_bytes();
        if name_bytes.len() > u16::MAX as usize {
            return Err(format!("文件名过长: {name}"));
        }
        let crc = crc32(raw);
        let mut enc = DeflateEncoder::new(Vec::new(), Compression::default());
        let (method, payload): (u16, Vec<u8>) = if enc.write_all(raw).is_ok() {
            match enc.finish() {
                Ok(def) if def.len() < raw.len() => (8, def),
                _ => (0, raw.clone()),
            }
        } else {
            (0, raw.clone())
        };

        let local_offset = out.len() as u32;
        // Local File Header (30 bytes + name)
        push_u32(&mut out, 0x0403_4B50);
        push_u16(&mut out, 20);
        push_u16(&mut out, 0x0800); // UTF-8 filename flag
        push_u16(&mut out, method);
        push_u16(&mut out, dos_time);
        push_u16(&mut out, dos_date);
        push_u32(&mut out, crc);
        push_u32(&mut out, payload.len() as u32);
        push_u32(&mut out, raw.len() as u32);
        push_u16(&mut out, name_bytes.len() as u16);
        push_u16(&mut out, 0);
        out.extend_from_slice(name_bytes);
        out.extend_from_slice(&payload);

        // Central Directory File Header (46 bytes + name)
        push_u32(&mut cd, 0x0201_4B50);
        push_u16(&mut cd, 20);
        push_u16(&mut cd, 20);
        push_u16(&mut cd, 0x0800);
        push_u16(&mut cd, method);
        push_u16(&mut cd, dos_time);
        push_u16(&mut cd, dos_date);
        push_u32(&mut cd, crc);
        push_u32(&mut cd, payload.len() as u32);
        push_u32(&mut cd, raw.len() as u32);
        push_u16(&mut cd, name_bytes.len() as u16);
        push_u16(&mut cd, 0);
        push_u16(&mut cd, 0);
        push_u16(&mut cd, 0);
        push_u16(&mut cd, 0);
        push_u32(&mut cd, 0);
        push_u32(&mut cd, local_offset);
        cd.extend_from_slice(name_bytes);
    }

    let cd_offset = out.len() as u32;
    let cd_size = cd.len() as u32;
    out.extend_from_slice(&cd);

    // End of Central Directory (22 bytes)
    let count = u16::try_from(entries.len()).map_err(|_| "备份文件数量过多".to_owned())?;
    push_u32(&mut out, 0x0605_4B50);
    push_u16(&mut out, 0);
    push_u16(&mut out, 0);
    push_u16(&mut out, count);
    push_u16(&mut out, count);
    push_u32(&mut out, cd_size);
    push_u32(&mut out, cd_offset);
    push_u16(&mut out, 0);
    Ok(out)
}

fn read_u16(b: &[u8], off: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(off..off + 2)?.try_into().ok()?))
}

fn read_u32(b: &[u8], off: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(off..off + 4)?.try_into().ok()?))
}

fn decode_zip_archive(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use flate2::read::DeflateDecoder;
    use std::io::Read;

    if bytes.len() < 22 {
        return Err("ZIP 文件过短或损坏".into());
    }
    let min_search = bytes.len().saturating_sub(22 + 65_535);
    let mut eocd_pos = None;
    for pos in (min_search..=bytes.len() - 22).rev() {
        if read_u32(bytes, pos) == Some(0x0605_4B50) {
            let comment_len = read_u16(bytes, pos + 20).unwrap_or(u16::MAX) as usize;
            if pos + 22 + comment_len == bytes.len() {
                eocd_pos = Some(pos);
                break;
            }
        }
    }
    let eocd = eocd_pos.ok_or_else(|| "无法定位 ZIP 中央目录结尾标记".to_owned())?;
    let total_entries = read_u16(bytes, eocd + 10).ok_or("ZIP 条目数损坏")? as usize;
    let cd_size = read_u32(bytes, eocd + 12).ok_or("ZIP 中央目录长度损坏")? as usize;
    let cd_offset = read_u32(bytes, eocd + 16).ok_or("ZIP 中央目录偏移损坏")? as usize;
    let cd_end = cd_offset.checked_add(cd_size).ok_or("ZIP 偏移溢出")?;
    if cd_end > bytes.len() {
        return Err("ZIP 中央目录越界".into());
    }

    let mut cursor = cd_offset;
    let mut out = Vec::with_capacity(total_entries);
    for _ in 0..total_entries {
        if read_u32(bytes, cursor) != Some(0x0201_4B50) {
            return Err("ZIP 中央目录签名损坏".into());
        }
        let method = read_u16(bytes, cursor + 10).ok_or("ZIP 压缩方式缺失")?;
        let expected_crc = read_u32(bytes, cursor + 16).ok_or("ZIP CRC 缺失")?;
        let comp_size = read_u32(bytes, cursor + 20).ok_or("ZIP 压缩大小缺失")? as usize;
        let uncomp_size = read_u32(bytes, cursor + 24).ok_or("ZIP 原始大小缺失")? as usize;
        let name_len = read_u16(bytes, cursor + 28).ok_or("ZIP 文件名长度缺失")? as usize;
        let extra_len = read_u16(bytes, cursor + 30).ok_or("ZIP extra 长度缺失")? as usize;
        let comment_len = read_u16(bytes, cursor + 32).ok_or("ZIP comment 长度缺失")? as usize;
        let local_off = read_u32(bytes, cursor + 42).ok_or("ZIP 本地头偏移缺失")? as usize;

        let name_start = cursor + 46;
        let name_end = name_start.checked_add(name_len).ok_or("ZIP 文件名越界")?;
        let name = String::from_utf8_lossy(bytes.get(name_start..name_end).ok_or("ZIP 文件名越界")?).into_owned();
        cursor = name_end + extra_len + comment_len;

        if name.ends_with('/') || name.ends_with('\\') {
            continue;
        }
        if read_u32(bytes, local_off) != Some(0x0403_4B50) {
            return Err(format!("ZIP 条目本地头损坏: {name}"));
        }
        let lh_name_len = read_u16(bytes, local_off + 26).ok_or("ZIP 本地文件名长度缺失")? as usize;
        let lh_extra_len = read_u16(bytes, local_off + 28).ok_or("ZIP 本地 extra 长度缺失")? as usize;
        let data_start = local_off + 30 + lh_name_len + lh_extra_len;
        let data_end = data_start.checked_add(comp_size).ok_or("ZIP 数据偏移溢出")?;
        let comp_slice = bytes.get(data_start..data_end).ok_or_else(|| format!("ZIP 条目数据越界: {name}"))?;

        if uncomp_size > 64 * 1024 * 1024 {
            return Err(format!("ZIP 条目过大: {name}"));
        }
        let raw = match method {
            0 => comp_slice.to_vec(),
            8 => {
                let mut dec = DeflateDecoder::new(comp_slice);
                let mut buf = Vec::with_capacity(uncomp_size);
                dec.read_to_end(&mut buf).map_err(|e| format!("ZIP 解压 {name} 失败: {e}"))?;
                buf
            }
            other => return Err(format!("不支持的 ZIP 压缩算法 ({other}): {name}")),
        };
        if raw.len() != uncomp_size || crc32(&raw) != expected_crc {
            return Err(format!("ZIP 校验失败（文件可能已损坏）: {name}"));
        }
        out.push((name, raw));
    }
    Ok(out)
}

fn sanitize_backup_rel_path(raw: &str) -> Option<String> {
    let norm = raw.replace('\\', "/");
    let trimmed = norm.trim_start_matches('/');
    if trimmed.is_empty() || trimmed.contains(':') {
        return None;
    }
    let parts: Vec<&str> = trimmed.split('/').collect();
    if parts.iter().any(|p| p.is_empty() || *p == "." || *p == "..") {
        return None;
    }
    if parts.first().is_some_and(|first| should_skip_backup_dir(first)) {
        return None;
    }
    Some(parts.join("/"))
}

fn decode_json_backup_entries(bytes: &[u8]) -> Result<(Option<String>, Vec<(String, Vec<u8>)>), String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
    let root: Value = serde_json::from_slice(bytes).map_err(|e| format!("解析备份 JSON 失败: {e}"))?;
    let ver = root.get("version").and_then(|v| v.as_str()).map(|s| s.to_owned());

    if let Some(entries) = root.get("entries").and_then(|v| v.as_object()) {
        let mut out = Vec::with_capacity(entries.len());
        for (k, v) in entries {
            let enc = v.get("encoding").and_then(|s| s.as_str()).unwrap_or("utf8");
            let data = v.get("data").and_then(|s| s.as_str()).ok_or_else(|| format!("条目 {k} 缺少 data"))?;
            let raw = if enc == "base64" {
                B64.decode(data).map_err(|e| format!("解码条目 {k} 失败: {e}"))?
            } else {
                data.as_bytes().to_vec()
            };
            out.push((k.clone(), raw));
        }
        return Ok((ver, out));
    }

    if let Some(files) = root.get("files").and_then(|v| v.as_object()) {
        let mut out = Vec::with_capacity(files.len());
        for (k, v) in files {
            if let Some(content) = v.as_str() {
                out.push((k.clone(), content.as_bytes().to_vec()));
            }
        }
        return Ok((ver, out));
    }

    Err("备份文件格式无效：缺少 entries 或 files 字段".into())
}

fn legacy_alias_target(rel: &str) -> &str {
    match rel {
        "ai-status.conf" => CONFIG_FILE,
        "todos.txt" => "todo.txt",
        "quota.json" => "ai-quota.json",
        "notification_apps.json" => "notification-apps.json",
        _ => rel,
    }
}

/// 从 `.zip` 或 `.json` 恢复全量设置与全部本地数据，恢复前先自动生成安全快照并刷新所有内存缓存。
pub fn import_settings_backup(src_path: &std::path::Path) -> Result<String, String> {
    let dir = crate::model::config_dir();
    let _ = fs::create_dir_all(&dir);
    let src_bytes = fs::read(src_path).map_err(|e| format!("读取备份文件失败: {e}"))?;

    let is_zip = src_bytes.starts_with(b"PK")
        || src_path
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"));

    let (mut backup_ver, mut raw_entries): (Option<String>, Vec<(String, Vec<u8>)>) = if is_zip && src_bytes.starts_with(b"PK") {
        let zip_items = decode_zip_archive(&src_bytes)?;
        // 兼容旧版 PowerShell 生成的单文件快照 ZIP (`auralsle-backup-snapshot.json`)
        if zip_items.len() == 1 && zip_items[0].0.eq_ignore_ascii_case("auralsle-backup-snapshot.json") {
            decode_json_backup_entries(&zip_items[0].1)?
        } else {
            let ver = zip_items
                .iter()
                .find(|(n, _)| n == "__manifest__.json")
                .and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok())
                .and_then(|v| v.get("version").and_then(|s| s.as_str()).map(|s| s.to_owned()));
            (ver, zip_items)
        }
    } else {
        decode_json_backup_entries(&src_bytes)?
    };

    if backup_ver.is_none() {
        backup_ver = raw_entries
            .iter()
            .find(|(n, _)| n == "__manifest__.json")
            .and_then(|(_, b)| serde_json::from_slice::<Value>(b).ok())
            .and_then(|v| v.get("version").and_then(|s| s.as_str()).map(|s| s.to_owned()));
    }

    // 规范化路径并检测有哪些标准目标文件名已存在，避免旧名别名覆盖新名文件
    let mut present: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (name, _) in &raw_entries {
        if let Some(clean) = sanitize_backup_rel_path(name) {
            present.insert(clean);
        }
    }

    // 覆盖前生成自动安全快照，避免误操作损坏当前配置
    let safety_dir = dir.join("backups");
    let _ = fs::create_dir_all(&safety_dir);
    let safety_path = safety_dir.join("pre-restore-safety.zip");
    if present.iter().any(|n| n != "__manifest__.json") {
        let _ = export_settings_backup(&safety_path);
    }

    let mut count = 0usize;
    let mut restored_xlsx_fallback: Option<PathBuf> = None;

    for (raw_name, data) in raw_entries.drain(..) {
        let Some(clean) = sanitize_backup_rel_path(&raw_name) else { continue; };
        if clean == "__manifest__.json" {
            continue;
        }
        if let Some(fname) = clean.strip_prefix("__external__/course_xlsx/") {
            if !fname.contains('/') && !fname.is_empty() {
                let dest = dir.join("restored-courses").join(fname);
                if let Some(p) = dest.parent() {
                    let _ = fs::create_dir_all(p);
                }
                if crate::model::atomic_write(&dest, &data).is_ok() {
                    restored_xlsx_fallback = Some(dest);
                    count += 1;
                }
            }
            continue;
        }
        let mapped = legacy_alias_target(&clean);
        if mapped != clean && present.contains(mapped) {
            continue;
        }
        let target = dir.join(mapped.replace('/', "\\"));
        if let Some(parent) = target.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if crate::model::atomic_write(&target, &data).is_ok() {
            count += 1;
        }
    }

    reload_from_disk();
    if let Some(fallback_xlsx) = restored_xlsx_fallback {
        let cur_xlsx = get().course_xlsx;
        if !cur_xlsx.trim().is_empty() && !Path::new(cur_xlsx.trim()).is_file() {
            let fb_str = fallback_xlsx.to_string_lossy().into_owned();
            update(|c| c.course_xlsx = fb_str);
        }
    }
    crate::monitors::reload_twinkle_config();
    crate::auralsle_ai_status::course::reload_from_disk();

    let ver_note = backup_ver
        .map(|v| format!(" · 备份版本 {v}"))
        .unwrap_or_default();
    Ok(format!("已完整恢复 {count} 个配置与数据文件{ver_note}"))
}

pub fn reload_from_disk() {
    let loaded = Config::load();
    if let Ok(mut g) = lock().lock() {
        *g = loaded;
        CONFIG_GEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    }
}
