//! 定制组件系统与独立「天气」「翻译」页面及设置面板。
//!
//! - **定制组件 (`page == 9` / `settings_tab == 13`)**：16 项高价值实时硬件、电源、时间与课表/AI 组件，
//!   小卡片统一采用 `theme::CARD` 底色以跟随所有主题，响应式列宽彻底消除窄窗口右侧裁切；
//! - **独立天气页面 (`page == 10` / `settings_tab == 18`)**：卡片式气象 UI，支持精准到街道级定位（省/市/区/街道 + 经纬度逆地理编码）、
//!   自定义接入天气源（支持 `{lat}` / `{lon}` / `{city}` / `{street}` / `{key}` 占位符与多格式 JSON 自动解析）及显示信息模块自由勾选；
//! - **独立翻译页面 (`page == 11` / `settings_tab == 19`)**：支持「暂时冻结岛体（不自动收回）」、多引擎切换、剪贴板自动识别与中/英/日互译。

use crate::{
    auralsle_ai_status::{config, course, state, sysmon},
    extras::settings_label,
};
use chrono::{Datelike, Local, Timelike};
use eframe::egui::{self, Color32, Pos2, Rect, RichText, Sense, Stroke, Vec2};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Mutex, OnceLock,
};

// ---------------------------------------------------------------
// 街道级精准天气定位 + 自定义接入天气源 + 24h逐时曲线与5日预报引擎
// ---------------------------------------------------------------

pub const WEATHER_SOURCES: [(u8, &str, &str); 5] = [
    (0, "Open-Meteo 高精经纬源", "根据街道级经纬度实时获取实况、24h 逐时趋势与 5 日预报（免 Key）"),
    (1, "和风 / 中国天气街道源", "支持国内省市区街道实况、空气质量 AQI 与生活指数（可填和风 API Key）"),
    (2, "Wttr.in 全球气象源", "轻量 JSON 气象聚合服务，支持全球城市与街道坐标查询"),
    (3, "彩云天气街道分钟级源", "精准到街道级降水与空气质量，支持填写彩云 Token 接入实时数据"),
    (4, "自定义接入天气源", "自由接入任意第三方/自建天气 API（支持 {lat} {lon} {city} {street} {key} 模板）"),
];

/// 内置常见城市与高校/核心街道坐标库（离线秒级匹配 + 在线逆地理编码双引擎）。
pub const STREET_PRESETS: [(&str, &str, f64, f64); 10] = [
    ("长沙市", "岳麓区 · 麓山南路街道", 28.1812, 112.9445),
    ("长沙市", "五一广场 · 黄兴中路街道", 28.1963, 112.9773),
    ("北京市", "海淀区 · 中关村街道", 39.9836, 116.3164),
    ("上海市", "浦东新区 · 陆家嘴街道", 31.2397, 121.4998),
    ("广州市", "天河区 · 天河南街道", 23.1317, 113.3245),
    ("深圳市", "南山区 · 粤海街道科技园", 22.5405, 113.9534),
    ("武汉市", "洪山区 · 珞喻路街道", 30.5284, 114.3612),
    ("成都市", "武侯区 · 望江路街道", 30.6308, 104.0815),
    ("杭州市", "西湖区 · 灵隐街道浙大路", 30.2636, 120.1232),
    ("南京市", "鼓楼区 · 宁海路街道", 32.0584, 118.7778),
];

#[derive(Clone, Debug)]
pub struct HourlyPoint {
    pub hour_label: String,
    pub temp: i32,
}

#[derive(Clone, Debug)]
pub struct DailyForecast {
    pub day_label: String,
    pub desc: String,
    pub t_min: i32,
    pub t_max: i32,
}

#[derive(Clone, Debug)]
pub struct WislandWeatherState {
    pub city: String,
    pub street: String,
    pub lat: f64,
    pub lon: f64,
    pub desc: String,
    pub temp: i32,
    pub feels_like: i32,
    pub humidity: u8,
    pub wind: String,
    pub pressure: String,
    pub aqi: String,
    pub aqi_num: u16,
    pub uv: String,
    pub sun_rise_set: String,
    pub life_tip: String,
    pub source_name: String,
    pub updated_at: String,
    pub locate_status: String,
    pub hourly: Vec<HourlyPoint>,
    pub forecast: Vec<DailyForecast>,
    pub synced_from_live: bool,
}

pub fn build_hourly_realistic(
    cur_temp: i32,
    t_min: i32,
    t_max: i32,
    now_hour: u32,
    lat: f64,
    lon: f64,
) -> Vec<HourlyPoint> {
    let lo = t_min.min(cur_temp - 2);
    let hi = t_max.max(cur_temp + 2).max(lo + 3);
    let mid = (lo + hi) as f32 * 0.5;
    let amp = ((hi - lo) as f32 * 0.5).max(1.5);
    let micro = (((lat * 100.0 + lon * 100.0).sin()) * 0.6).clamp(-0.6, 0.6) as f32;
    // 昼夜温度相位：05:00 最低，14:00 最高
    let diurnal_at = |h: u32| -> f32 {
        let rad = ((h as f32 - 5.0) / 24.0) * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
        mid + amp * rad.sin() + micro
    };
    let anchor_delta = cur_temp as f32 - diurnal_at(now_hour % 24);
    (0..8)
        .map(|i| {
            let h = (now_hour + i as u32 * 3) % 24;
            let t_val = if i == 0 {
                cur_temp
            } else {
                let decay = (1.0 - i as f32 * 0.14).clamp(0.15, 0.88);
                (diurnal_at(h) + anchor_delta * decay).round() as i32
            };
            HourlyPoint {
                hour_label: if i == 0 { "现在".into() } else { format!("{h:02}:00") },
                temp: t_val,
            }
        })
        .collect()
}

fn build_hourly(base_temp: i32, now_hour: u32) -> Vec<HourlyPoint> {
    build_hourly_realistic(base_temp, base_temp - 4, base_temp + 3, now_hour, 28.1812, 112.9445)
}

/// 判断字符串是否为有效的区县/街道级地址（排除空白、`湖南 · 中国`、`中国` 等粗粒度国家/省份后缀）。
pub fn is_valid_street_label(s: &str) -> bool {
    let t = s.trim();
    if t.is_empty() || t == "实时气象站" || t == "当地" {
        return false;
    }
    if t.contains("中国") || t.eq_ignore_ascii_case("china") {
        return false;
    }
    t.contains('区')
        || t.contains('街')
        || t.contains('路')
        || t.contains('道')
        || t.contains('镇')
        || t.contains('县')
        || t.contains('园')
        || t.contains("广场")
        || t.contains('巷')
        || t.contains('里')
        || t.contains('湾')
        || t.contains('村')
        || t.chars().count() >= 4
}

/// 根据城市名称与经纬度匹配默认核心区县与街道（兜底映射表）。
pub fn default_street_for_city(city: &str, lat: f64, lon: f64) -> String {
    for &(c, st, la, lo) in &STREET_PRESETS {
        if (lat - la).abs() < 0.015 && (lon - lo).abs() < 0.015 {
            return st.to_owned();
        }
        let _ = c;
    }
    let c_short = city.trim().trim_end_matches(['市', '省', '区', '县']);
    for &(c, st, _, _) in &STREET_PRESETS {
        let p_short = c.trim_end_matches('市');
        if !c_short.is_empty() && (c_short.contains(p_short) || p_short.contains(c_short)) {
            return st.to_owned();
        }
    }
    const EXTRA_STREETS: &[(&str, &str)] = &[
        ("重庆", "渝中区 · 解放碑街道"),
        ("天津", "和平区 · 五大道街道"),
        ("西安", "雁塔区 · 小寨路街道"),
        ("苏州", "姑苏区 · 平江街道"),
        ("厦门", "思明区 · 厦港街道"),
        ("青岛", "市南区 · 五四广场街道"),
        ("大连", "中山区 · 人民路街道"),
        ("郑州", "金水区 · 花园路街道"),
        ("合肥", "蜀山区 · 三里庵街道"),
        ("福州", "鼓楼区 · 东街街道"),
        ("济南", "历下区 · 泉城路街道"),
        ("昆明", "五华区 · 护国街道"),
        ("沈阳", "和平区 · 太原街街道"),
        ("哈尔滨", "南岗区 · 奋斗路街道"),
        ("长春", "朝阳区 · 桂林街道"),
        ("石家庄", "长安区 · 中山东路街道"),
        ("太原", "迎泽区 · 柳巷街道"),
        ("南昌", "东湖区 · 八一公园街道"),
        ("南宁", "青秀区 · 中山街道"),
        ("贵阳", "云岩区 · 中华中路街道"),
        ("兰州", "城关区 · 酒泉路街道"),
        ("海口", "龙华区 · 海垦街道"),
        ("乌鲁木齐", "天山区 · 解放北路街道"),
        ("拉萨", "城关区 · 八廓街道"),
        ("银川", "兴庆区 · 解放西街街道"),
        ("西宁", "城中区 · 人民街街道"),
        ("呼和浩特", "赛罕区 · 大学西路街道"),
        ("佛山", "禅城区 · 祖庙街道"),
        ("东莞", "南城街道 · 鸿福路"),
        ("宁波", "海曙区 · 鼓楼街道"),
        ("无锡", "梁溪区 · 崇安寺街道"),
        ("珠海", "香洲区 · 吉大街道"),
    ];
    for &(k, st) in EXTRA_STREETS {
        if !c_short.is_empty() && (c_short.contains(k) || k.contains(c_short)) {
            return st.to_owned();
        }
    }
    if !c_short.is_empty() {
        format!("{c_short}核心区 · 中心街道")
    } else {
        "岳麓区 · 麓山南路街道".to_owned()
    }
}

/// 根据城市与经纬度解析精准街道名（优先匹配预设坐标，其次在线逆地理编码，最后城市核心街道兜底）。
pub fn resolve_street_for_position(city: &str, lat: f64, lon: f64) -> String {
    for &(_c, st, la, lo) in &STREET_PRESETS {
        if (lat - la).abs() < 0.015 && (lon - lo).abs() < 0.015 {
            return st.to_owned();
        }
    }
    let cfg = config::get();
    let city_short = city.trim().trim_end_matches('市');
    let cfg_short = cfg.weather_city.trim().trim_end_matches('市');
    if !city_short.is_empty()
        && city_short == cfg_short
        && is_valid_street_label(&cfg.weather_street)
        && ((lat - cfg.weather_lat).abs() < 0.08 && (lon - cfg.weather_lon).abs() < 0.08
            || (lat.abs() < 0.01 && lon.abs() < 0.01))
    {
        return cfg.weather_street.trim().to_owned();
    }
    if !cfg!(test) && (lat.abs() > 0.05 || lon.abs() > 0.05) {
        let nom_url = format!(
            "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat={lat:.5}&lon={lon:.5}&zoom=18&addressdetails=1&accept-language=zh-CN"
        );
        if let Some(v) = ureq_get_json(&nom_url, 3) {
            if let Some(addr) = v.get("address") {
                let dist = addr
                    .get("suburb")
                    .or_else(|| addr.get("city_district"))
                    .or_else(|| addr.get("district"))
                    .or_else(|| addr.get("town"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let rd = addr
                    .get("road")
                    .or_else(|| addr.get("neighbourhood"))
                    .or_else(|| addr.get("quarter"))
                    .or_else(|| addr.get("pedestrian"))
                    .or_else(|| addr.get("Subdistrict"))
                    .and_then(|x| x.as_str())
                    .unwrap_or("");
                let combined = match (!dist.is_empty(), !rd.is_empty()) {
                    (true, true) => format!("{dist} · {rd}"),
                    (true, false) => dist.to_owned(),
                    (false, true) => rd.to_owned(),
                    (false, false) => String::new(),
                };
                if is_valid_street_label(&combined) {
                    return combined;
                }
            }
        }
    }
    default_street_for_city(city, lat, lon)
}

/// 统一将城市与街道格式化为精准到街道的显示文本（如：`长沙 · 岳麓区 · 麓山南路街道`）。
pub fn format_street_address(city: &str, street: &str) -> String {
    let city_clean = city.trim().trim_end_matches('市');
    let city_disp = if city_clean.is_empty() { "长沙" } else { city_clean };
    let valid_st = if is_valid_street_label(street) {
        street.trim().to_owned()
    } else {
        default_street_for_city(city_disp, 0.0, 0.0)
    };
    if valid_st.starts_with(city_disp) || valid_st.starts_with(city.trim()) {
        valid_st
    } else {
        format!("{city_disp} · {valid_st}")
    }
}

impl Default for WislandWeatherState {
    fn default() -> Self {
        let now_h = Local::now().hour();
        Self {
            city: "长沙市".into(),
            street: "岳麓区 · 麓山南路街道".into(),
            lat: 28.1812,
            lon: 112.9445,
            desc: "晴间多云".into(),
            temp: 22,
            feels_like: 21,
            humidity: 54,
            wind: "东南风 2级 · 11km/h".into(),
            pressure: "1016 hPa".into(),
            aqi: "优 28".into(),
            aqi_num: 28,
            uv: "弱 UV2".into(),
            sun_rise_set: "06:44 日出 · 17:36 日落".into(),
            life_tip: "适宜户外运动 · 昼夜温差较大注意添衣".into(),
            source_name: "Open-Meteo".into(),
            updated_at: "刚刚更新".into(),
            locate_status: "已定位到街道级坐标".into(),
            hourly: build_hourly(22, now_h),
            forecast: vec![
                DailyForecast { day_label: "今天".into(), desc: "晴间多云".into(), t_min: 14, t_max: 23 },
                DailyForecast { day_label: "明天".into(), desc: "多云".into(), t_min: 13, t_max: 21 },
                DailyForecast { day_label: "后天".into(), desc: "晴转多云".into(), t_min: 12, t_max: 22 },
                DailyForecast { day_label: "大后天".into(), desc: "小雨".into(), t_min: 10, t_max: 17 },
                DailyForecast { day_label: "第5天".into(), desc: "晴朗".into(), t_min: 11, t_max: 20 },
            ],
            synced_from_live: false,
        }
    }
}

static WISLAND_WEATHER: OnceLock<Mutex<WislandWeatherState>> = OnceLock::new();
static WEATHER_FETCHING: AtomicBool = AtomicBool::new(false);
static STREET_LOCATING: AtomicBool = AtomicBool::new(false);
static STREET_WEATHER_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static LAST_APPLIED_STREET_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static WEATHER_REQ_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub fn is_weather_busy() -> bool {
    WEATHER_FETCHING.load(Ordering::Relaxed) || STREET_LOCATING.load(Ordering::Relaxed)
}

pub fn bump_street_weather_seq() {
    STREET_WEATHER_SEQ.fetch_add(1, Ordering::SeqCst);
}

/// 当 Open-Meteo 在 `weather::parse` 中解析出 `hourly.temperature_2m` 时，实时同步至街道逐时趋势列表。
pub fn record_open_meteo_hourly(_place: &str, cur_temp: f32, value: &serde_json::Value) {
    let Some(h_temps) = value.pointer("/hourly/temperature_2m").and_then(|x| x.as_array()) else {
        return;
    };
    let now_h = Local::now().hour() as usize;
    let h_times = value.pointer("/hourly/time").and_then(|x| x.as_array());
    let start_idx = h_times
        .and_then(|times| {
            let suffix = format!("T{now_h:02}:00");
            times.iter().position(|t| t.as_str().is_some_and(|s| s.ends_with(&suffix)))
        })
        .unwrap_or(now_h.min(h_temps.len().saturating_sub(1)));
    let mut hp = Vec::with_capacity(8);
    for i in 0..8 {
        let idx = (start_idx + i * 3).min(h_temps.len().saturating_sub(1));
        if let Some(tv) = h_temps.get(idx).and_then(|x| x.as_f64()) {
            let h = (now_h + i * 3) % 24;
            hp.push(HourlyPoint {
                hour_label: if i == 0 { "现在".into() } else { format!("{h:02}:00") },
                temp: if i == 0 { cur_temp.round() as i32 } else { tv.round() as i32 },
            });
        }
    }
    if hp.len() >= 4 {
        if let Ok(mut g) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
            g.hourly = hp;
        }
    }
}

/// 返回当前选中的天气数据源名称与官网链接（修复需求 2：选择彩云源/和风源后顶部数据来源仍显示 Open-Meteo 的问题）。
pub fn active_weather_source_info() -> (String, &'static str) {
    let cfg = config::get();
    match cfg.weather_source {
        1 => ("和风/中国天气".to_owned(), "https://www.qweather.com/"),
        2 => ("Wttr.in 全球源".to_owned(), "https://wttr.in/"),
        3 => ("彩云天气街道源".to_owned(), "https://caiyunapp.com/"),
        4 => {
            let name = if cfg.weather_custom_name.trim().is_empty() {
                "自定义天气源".to_owned()
            } else {
                cfg.weather_custom_name.trim().to_owned()
            };
            (name, "https://open-meteo.com/")
        }
        _ => ("Open-Meteo".to_owned(), "https://open-meteo.com/"),
    }
}

static INITIAL_WEATHER_BOOTSTRAPPED: AtomicBool = AtomicBool::new(false);

pub fn ensure_initial_weather_fetch(city_hint: &str) {
    if cfg!(test) {
        return;
    }
    if !INITIAL_WEATHER_BOOTSTRAPPED.swap(true, Ordering::SeqCst) {
        let hint = city_hint.trim();
        if hint.is_empty() {
            locate_street_async();
        } else {
            refresh_weather_async(hint.to_owned());
        }
    }
}

pub fn weather_state() -> WislandWeatherState {
    let cfg = config::get();
    let mut g = WISLAND_WEATHER
        .get_or_init(|| Mutex::new(WislandWeatherState::default()))
        .lock()
        .map(|s| s.clone())
        .unwrap_or_default();
    if !g.synced_from_live {
        if !cfg.weather_city.trim().is_empty() {
            g.city = cfg.weather_city.clone();
        }
        if is_valid_street_label(&cfg.weather_street) {
            g.street = cfg.weather_street.clone();
        }
        g.lat = cfg.weather_lat;
        g.lon = cfg.weather_lon;
        if !cfg.weather_desc.trim().is_empty() {
            g.desc = cfg.weather_desc.clone();
        }
        g.temp = cfg.weather_temp;
    }
    if !is_valid_street_label(&g.street) {
        g.street = if is_valid_street_label(&cfg.weather_street) {
            cfg.weather_street.clone()
        } else {
            default_street_for_city(&g.city, g.lat, g.lon)
        };
    }
    if g.hourly.is_empty() || g.hourly.first().map(|p| p.temp) != Some(g.temp) {
        let t_min = g.forecast.first().map(|d| d.t_min).unwrap_or(g.temp - 4);
        let t_max = g.forecast.first().map(|d| d.t_max).unwrap_or(g.temp + 3);
        g.hourly = build_hourly_realistic(g.temp, t_min, t_max, Local::now().hour(), g.lat, g.lon);
    }
    g.source_name = match cfg.weather_source {
        1 => "和风/中国天气".into(),
        2 => "Wttr.in".into(),
        3 => "彩云街道源".into(),
        4 => {
            if cfg.weather_custom_name.trim().is_empty() {
                "自定义源".into()
            } else {
                cfg.weather_custom_name.trim().to_owned()
            }
        }
        _ => "Open-Meteo".into(),
    };
    g
}

fn wmo_code_desc(code: i64) -> &'static str {
    match code {
        0 => "晴朗",
        1 | 2 => "晴间多云",
        3 => "阴天",
        45 | 48 => "雾霾",
        51..=57 => "毛毛雨",
        61..=67 | 80..=82 => "阵雨",
        71..=77 | 85..=86 => "降雪",
        95..=99 => "雷阵雨",
        _ => "多云",
    }
}

fn desc_to_wmo_code(desc: &str) -> u8 {
    let d = desc.trim();
    if d.contains("雷") {
        95
    } else if d.contains("雪") {
        71
    } else if d.contains("毛毛雨") || d.contains("细雨") {
        51
    } else if d.contains("阵雨") {
        80
    } else if d.contains("雨") {
        61
    } else if d.contains("雾") || d.contains("霾") {
        45
    } else if d.contains("阴") {
        3
    } else if d.contains("多云") {
        2
    } else {
        0
    }
}

fn preset_metrics_for(city: &str, street: &str) -> (i32, &'static str, u8) {
    let short_city = city.trim_end_matches('市');
    if street.contains("黄兴中路") || street.contains("五一广场") {
        (23, "晴转多云", 50)
    } else if short_city == "北京" {
        (16, "晴朗", 38)
    } else if short_city == "上海" {
        (21, "多云", 62)
    } else if short_city == "广州" {
        (26, "晴间多云", 68)
    } else if short_city == "深圳" {
        (27, "晴朗", 70)
    } else if short_city == "武汉" {
        (20, "晴转多云", 56)
    } else if short_city == "成都" {
        (19, "阴转多云", 66)
    } else if short_city == "杭州" {
        (21, "晴间多云", 58)
    } else if short_city == "南京" {
        (19, "晴朗", 52)
    } else {
        (22, "晴间多云", 54)
    }
}

fn match_preset_location(query: &str) -> Option<(&'static str, &'static str, f64, f64, i32, &'static str, u8)> {
    let q = query.trim();
    // 1) 优先按具体街道匹配（避免同城不同街道如「麓山南路街道」与「黄兴中路街道」串位）
    for &(city, street, lat, lon) in &STREET_PRESETS {
        let st_tail = street.split('·').next_back().unwrap_or(street).trim();
        if !q.is_empty() && (q.contains(street) || (!st_tail.is_empty() && q.contains(st_tail))) {
            let (t, desc, hum) = preset_metrics_for(city, street);
            return Some((city, street, lat, lon, t, desc, hum));
        }
    }
    // 2) 再按城市匹配
    for &(city, street, lat, lon) in &STREET_PRESETS {
        let short_city = city.trim_end_matches('市');
        if !q.is_empty() && (q.contains(short_city) || street.contains(q)) {
            let (t, desc, hum) = preset_metrics_for(city, street);
            return Some((city, street, lat, lon, t, desc, hum));
        }
    }
    None
}

fn url_encode_utf8(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "%20".to_owned(),
            _ => format!("%{:02X}", b),
        })
        .collect()
}

/// 一键自动定位到当前街道（通过本机公网/网络定位 + OpenStreetMap Nominatim `zoom=18` 街道级逆地理编码）。

fn ureq_get_json_with_agent(agent: &ureq::Agent, url: &str) -> Option<serde_json::Value> {
    use std::io::Read;
    let resp = agent
        .get(url)
        .set("User-Agent", "AuralsleDynamicIsland/1.0")
        .set("Accept", "application/json")
        .call()
        .ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().take(512 * 1024).read_to_end(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn ureq_get_json(url: &str, timeout_secs: u64) -> Option<serde_json::Value> {
    let direct = ureq::AgentBuilder::new()
        .try_proxy_from_env(false)
        .timeout_connect(std::time::Duration::from_secs(timeout_secs.min(4)))
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    if let Some(v) = ureq_get_json_with_agent(&direct, url) {
        return Some(v);
    }
    if let Some(proxy) = crate::weather::network_proxy() {
        let proxied = crate::weather::http_agent_for(Some(&proxy));
        if let Some(v) = ureq_get_json_with_agent(&proxied, url) {
            return Some(v);
        }
    }
    None
}

fn ureq_post_json(
    url: &str,
    payload: &serde_json::Value,
    api_key: &str,
    is_anthropic: bool,
    timeout_secs: u64,
) -> Option<serde_json::Value> {
    use std::io::Read;
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build();
    let mut req = agent
        .post(url)
        .set("User-Agent", "AuralsleDynamicIsland/1.0")
        .set("Content-Type", "application/json");
    if !api_key.is_empty() {
        if is_anthropic {
            req = req
                .set("x-api-key", api_key)
                .set("anthropic-version", "2023-06-01");
        } else {
            req = req.set("Authorization", &format!("Bearer {api_key}"));
        }
    }
    let resp = req.send_string(&payload.to_string()).ok()?;
    let mut bytes = Vec::new();
    resp.into_reader().take(512 * 1024).read_to_end(&mut bytes).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn json_as_f64(v: &serde_json::Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str().and_then(|s| s.trim().parse::<f64>().ok()))
}

#[cfg(windows)]
fn windows_winrt_geolocate() -> Option<(f64, f64)> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let script = r#"
try {
  Add-Type -AssemblyName System.Runtime.WindowsRuntime | Out-Null
  $asTask = ([System.WindowsRuntimeSystemExtensions].GetMethods() | Where-Object {
    $_.Name -eq 'AsTask' -and $_.GetParameters().Count -eq 1 -and $_.GetParameters()[0].ParameterType.Name -eq 'IAsyncOperation`1'
  })[0]
  $null = [Windows.Devices.Geolocation.Geolocator,Windows.Foundation,ContentType=WindowsRuntime]
  $geo = [Windows.Devices.Geolocation.Geolocator]::new()
  $geo.DesiredAccuracy = [Windows.Devices.Geolocation.PositionAccuracy]::High
  $op = $geo.GetGeopositionAsync()
  $task = $asTask.MakeGenericMethod([Windows.Devices.Geolocation.Geoposition]).Invoke($null, @($op))
  if ($task.Wait(4500)) {
    $p = $task.Result.Coordinate.Point.Position
    "$($p.Latitude),$($p.Longitude)"
  }
} catch {}
"#;
    let out = std::process::Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-WindowStyle", "Hidden", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().map(str::trim).find(|l| l.contains(','))?;
    let (a, b) = line.split_once(',')?;
    let lat = a.trim().parse::<f64>().ok()?;
    let lon = b.trim().parse::<f64>().ok()?;
    if (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && (lat.abs() > 0.05 || lon.abs() > 0.05) {
        Some((lat, lon))
    } else {
        None
    }
}

/// 综合 Windows Wi-Fi/GPS 定位与多源 IP/逆地理定位，返回 `(lat, lon, city, street)`。
pub(crate) fn geolocate_precise() -> Option<(f64, f64, String, String)> {
    let mut lat = 0.0_f64;
    let mut lon = 0.0_f64;
    let mut city = String::new();
    let mut street = String::new();
    let mut has_coords = false;

    #[cfg(windows)]
    if let Some((la, lo)) = windows_winrt_geolocate() {
        lat = la;
        lon = lo;
        has_coords = true;
    }

    if !has_coords {
        // 1) 国内直连优先：百度云 IP 区县级定位（中文 city + district + lat/lng）
        if let Some(v) = ureq_get_json("https://qifu-api.baidubce.com/ip/local/geo/v1/district", 4) {
            if let Some(d) = v.get("data") {
                if let (Some(la), Some(lo)) = (
                    d.get("lat").and_then(json_as_f64),
                    d.get("lng").and_then(json_as_f64),
                ) {
                    if la.abs() > 0.05 || lo.abs() > 0.05 {
                        lat = la;
                        lon = lo;
                        has_coords = true;
                    }
                }
                if let Some(c) = d.get("city").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    city = c.to_owned();
                } else if let Some(p) = d.get("prov").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    city = p.to_owned();
                }
                if let Some(dist) = d.get("district").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    street = dist.to_owned();
                }
            }
        }
    }

    if !has_coords {
        if let Some(v) = ureq_get_json("https://ipwho.is/?lang=zh-CN", 4) {
            if v.get("success").and_then(|x| x.as_bool()).unwrap_or(false) {
                if let (Some(la), Some(lo)) = (
                    v.get("latitude").and_then(json_as_f64),
                    v.get("longitude").and_then(json_as_f64),
                ) {
                    lat = la;
                    lon = lo;
                    has_coords = true;
                }
                if let Some(c) = v.get("city").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    city = c.to_owned();
                }
            }
        }
    }

    if !has_coords {
        if let Some(v) = ureq_get_json("http://ip-api.com/json/?lang=zh-CN&fields=status,lat,lon,city,district", 4) {
            if v.get("status").and_then(|x| x.as_str()) == Some("success") {
                if let (Some(la), Some(lo)) = (
                    v.get("lat").and_then(json_as_f64),
                    v.get("lon").and_then(json_as_f64),
                ) {
                    lat = la;
                    lon = lo;
                    has_coords = true;
                }
                if let Some(c) = v.get("city").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    city = c.to_owned();
                }
                if let Some(dist) = v.get("district").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    street = dist.to_owned();
                }
            }
        }
    }

    if !has_coords {
        if let Some(v) = ureq_get_json("https://free.freeipapi.com/api/v1/json", 4) {
            if let (Some(la), Some(lo)) = (
                v.get("latitude").and_then(json_as_f64),
                v.get("longitude").and_then(json_as_f64),
            ) {
                lat = la;
                lon = lo;
                has_coords = true;
            }
            if let Some(c) = v.get("cityName").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                city = c.to_owned();
            }
        }
    }

    if !has_coords {
        return None;
    }

    // 逆地理编码：优先 OpenStreetMap Nominatim (zoom=18) 获取具体区县 + 街道/路名，
    // 不可达时由 BigDataCloud 中文逆地理兜底，避免只停在省/市级。
    let mut rev_ok = false;
    let nom_url = format!(
        "https://nominatim.openstreetmap.org/reverse?format=jsonv2&lat={lat:.5}&lon={lon:.5}&zoom=18&addressdetails=1&accept-language=zh-CN"
    );
    if let Some(v) = ureq_get_json(&nom_url, 5) {
        if let Some(addr) = v.get("address") {
            let c_str = addr
                .get("city")
                .or_else(|| addr.get("municipality"))
                .or_else(|| addr.get("county"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            if !c_str.is_empty() {
                city = c_str.to_owned();
            }
            let dist = addr
                .get("suburb")
                .or_else(|| addr.get("city_district"))
                .or_else(|| addr.get("district"))
                .or_else(|| addr.get("town"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let rd = addr
                .get("road")
                .or_else(|| addr.get("neighbourhood"))
                .or_else(|| addr.get("quarter"))
                .or_else(|| addr.get("pedestrian"))
                .or_else(|| addr.get("Subdistrict"))
                .and_then(|x| x.as_str())
                .unwrap_or("");
            let combined = match (!dist.is_empty(), !rd.is_empty()) {
                (true, true) => format!("{dist} · {rd}"),
                (true, false) => dist.to_owned(),
                (false, true) => rd.to_owned(),
                (false, false) => String::new(),
            };
            if !combined.is_empty() {
                street = combined;
                rev_ok = true;
            }
        }
    }

    if !rev_ok || city.is_empty() || street.is_empty() {
        let bdc_url = format!(
            "https://api.bigdatacloud.net/data/reverse-geocode-client?latitude={lat:.5}&longitude={lon:.5}&localityLanguage=zh"
        );
        if let Some(v) = ureq_get_json(&bdc_url, 4) {
            let c_str = v.get("city").and_then(|x| x.as_str()).unwrap_or("");
            let loc_str = v.get("locality").and_then(|x| x.as_str()).unwrap_or("");
            if !c_str.is_empty() && city.is_empty() {
                city = c_str.to_owned();
            }
            if city.is_empty() {
                if let Some(sub) = v.get("principalSubdivision").and_then(|x| x.as_str()).filter(|s| !s.is_empty()) {
                    city = sub.to_owned();
                }
            }
            if !loc_str.is_empty() && (street.is_empty() || !rev_ok) {
                street = loc_str.to_owned();
            }
        }
    }

    Some((lat, lon, city, street))
}

pub fn wisland_to_weather_snapshot(w_st: &WislandWeatherState) -> crate::weather::Snapshot {
    let clean_desc = w_st
        .desc
        .trim_start_matches(['☀', '☁', '❄', '⛈', '⛅', ' '])
        .trim()
        .to_owned();
    let code = desc_to_wmo_code(&clean_desc);
    let wind_kmh = w_st
        .wind
        .split(|c: char| !c.is_ascii_digit() && c != '.')
        .filter(|s| !s.is_empty())
        .next_back()
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(12.0);
    let mut sr_str = "06:44".to_owned();
    let mut ss_str = "17:36".to_owned();
    for part in w_st.sun_rise_set.split('·') {
        let p = part.trim();
        let hhmm: String = p.chars().take_while(|c| c.is_ascii_digit() || *c == ':').collect();
        if hhmm.len() == 5 && hhmm.contains(':') {
            if p.contains("日出") {
                sr_str = hhmm;
            } else if p.contains("日落") {
                ss_str = hhmm;
            }
        }
    }
    let today = Local::now().date_naive();
    let weekdays = ["一", "二", "三", "四", "五", "六", "日"];
    let mut days = Vec::with_capacity(crate::weather::FORECAST_DAYS);
    for idx in 0..crate::weather::FORECAST_DAYS {
        let d_date = today
            .checked_add_days(chrono::Days::new(idx as u64))
            .unwrap_or(today);
        let date_str = d_date.format("%Y-%m-%d").to_string();
        let label = match idx {
            0 => "今天".to_owned(),
            1 => "明天".to_owned(),
            _ => format!("周{}", weekdays[d_date.weekday().num_days_from_monday() as usize]),
        };
        if let Some(fc) = w_st.forecast.get(idx) {
            let fc_code = desc_to_wmo_code(&fc.desc);
            days.push(crate::weather::Day {
                label,
                date: date_str,
                high: fc.t_max as f32,
                low: fc.t_min as f32,
                code: fc_code,
                rain: if fc.desc.contains("雨") { 65 } else { 0 },
            });
        } else {
            let base_hi = w_st.forecast.last().map(|d| d.t_max).unwrap_or(w_st.temp + 3);
            let base_lo = w_st.forecast.last().map(|d| d.t_min).unwrap_or(w_st.temp - 4);
            let delta = if idx % 2 == 0 { 1 } else { -1 };
            days.push(crate::weather::Day {
                label,
                date: date_str,
                high: (base_hi + delta) as f32,
                low: (base_lo + delta) as f32,
                code,
                rain: 0,
            });
        }
    }
    let high = days.first().map(|d| d.high).or(Some((w_st.temp + 3) as f32));
    let low = days.first().map(|d| d.low).or(Some((w_st.temp - 4) as f32));
    let valid_street = if is_valid_street_label(&w_st.street) {
        w_st.street.trim().to_owned()
    } else {
        default_street_for_city(&w_st.city, w_st.lat, w_st.lon)
    };
    let pos = crate::weather::Position {
        lat: w_st.lat,
        lon: w_st.lon,
        label: w_st.city.trim().to_owned(),
        region: valid_street,
    };
    crate::weather::Snapshot {
        condition: if clean_desc.is_empty() { None } else { Some(clean_desc) },
        attributions: Vec::new(),
        place: w_st.city.trim().to_owned(),
        position: Some(pos),
        auto: false,
        temp: w_st.temp as f32,
        feels: Some(w_st.feels_like as f32),
        humidity: Some(w_st.humidity as u32),
        wind: Some(wind_kmh),
        code,
        day: (6..19).contains(&Local::now().hour()),
        high,
        low,
        sunrise: Some(sr_str),
        sunset: Some(ss_str),
        sun_times: None,
        days,
        at: chrono::Utc::now().timestamp(),
    }
}

/// 若「街道级定位」触发了街道切换或街道实况更新，取出最新合成的 `Snapshot` 同步至 `App::weather`。
pub fn take_pending_street_snapshot() -> Option<(crate::weather::Snapshot, String, crate::weather::Position)> {
    let cur = STREET_WEATHER_SEQ.load(Ordering::SeqCst);
    let prev = LAST_APPLIED_STREET_SEQ.swap(cur, Ordering::SeqCst);
    if cur == prev {
        return None;
    }
    let w_st = weather_state();
    let snap = wisland_to_weather_snapshot(&w_st);
    let pos = snap.position.clone().unwrap_or(crate::weather::Position {
        lat: w_st.lat,
        lon: w_st.lon,
        label: w_st.city.clone(),
        region: w_st.street.clone(),
    });
    Some((snap, w_st.city, pos))
}

/// 立即应用用户在「街道级定位」中选择的城市与街道（0ms 更新内存状态与两个预览界面，并启动后台实况拉取）。
pub fn apply_street_selection(city: &str, street: &str, lat: f64, lon: f64) {
    let city_clean = if city.trim().is_empty() {
        "长沙市".to_owned()
    } else {
        city.trim().to_owned()
    };
    let street_clean = if is_valid_street_label(street) {
        street.trim().to_owned()
    } else {
        default_street_for_city(&city_clean, lat, lon)
    };
    let (fallback_t, fallback_desc, fallback_hum) = preset_metrics_for(&city_clean, &street_clean);
    let now_h = Local::now().hour();
    let t_min = fallback_t - 5;
    let t_max = fallback_t + 3;
    let hourly = build_hourly_realistic(fallback_t, t_min, t_max, now_h, lat, lon);
    let forecast = vec![
        DailyForecast { day_label: "今天".into(), desc: fallback_desc.to_owned(), t_min, t_max },
        DailyForecast { day_label: "明天".into(), desc: "多云".into(), t_min: t_min - 1, t_max: t_max - 1 },
        DailyForecast { day_label: "后天".into(), desc: "晴间多云".into(), t_min, t_max },
        DailyForecast { day_label: "大后天".into(), desc: "阵雨".into(), t_min: t_min - 2, t_max: t_max - 3 },
        DailyForecast { day_label: "第5天".into(), desc: "晴朗".into(), t_min: t_min - 1, t_max: t_max - 1 },
    ];
    config::update(|c| {
        c.weather_city = city_clean.clone();
        c.weather_street = street_clean.clone();
        c.weather_lat = lat;
        c.weather_lon = lon;
        c.weather_desc = fallback_desc.to_owned();
        c.weather_temp = fallback_t;
    });
    if let Ok(mut g) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
        g.city = city_clean.clone();
        g.street = street_clean.clone();
        g.lat = lat;
        g.lon = lon;
        g.temp = fallback_t;
        g.feels_like = fallback_t - 1;
        g.humidity = fallback_hum;
        g.desc = fallback_desc.to_owned();
        g.hourly = hourly;
        g.forecast = forecast;
        g.updated_at = Local::now().format("%H:%M 更新").to_string();
        g.locate_status = format!("已切换街道：{} · {} ({lat:.4}, {lon:.4})", city_clean, street_clean);
        g.synced_from_live = true;
    }
    bump_street_weather_seq();
    refresh_weather_for_street_async(city_clean, street_clean, lat, lon);
}

pub fn locate_street_async() {
    if STREET_LOCATING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(mut slot) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
        slot.locate_status = "正在高精度定位当前位置…".into();
    }
    std::thread::spawn(move || {
        let cfg = config::get();
        let mut lat = cfg.weather_lat;
        let mut lon = cfg.weather_lon;
        let mut city = cfg.weather_city.clone();
        let mut street = cfg.weather_street.clone();
        let mut resolved = false;

        if let Some((la, lo, c_out, s_out)) = geolocate_precise() {
            lat = la;
            lon = lo;
            if !c_out.is_empty() {
                city = c_out;
            }
            if is_valid_street_label(&s_out) {
                street = s_out;
            } else {
                street = resolve_street_for_position(&city, lat, lon);
            }
            resolved = !city.is_empty();
        }

        if !resolved {
            let query = format!("{} {}", city, street);
            if let Some((p_city, p_street, p_lat, p_lon, _, _, _)) = match_preset_location(&query) {
                city = p_city.to_owned();
                street = p_street.to_owned();
                lat = p_lat;
                lon = p_lon;
            } else {
                if let Ok(mut slot) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
                    slot.locate_status = "自动定位失败，请检查网络/系统定位权限或手动搜索城市".into();
                }
                STREET_LOCATING.store(false, Ordering::SeqCst);
                return;
            }
        }

        STREET_LOCATING.store(false, Ordering::SeqCst);
        apply_street_selection(&city, &street, lat, lon);
    });
}

/// 根据用户输入的「城市 + 区县 + 街道/路名」检索精确街道经纬度并刷新天气。
pub fn search_street_async(query: String) {
    let q = query.trim().to_owned();
    if q.is_empty() {
        locate_street_async();
        return;
    }
    if STREET_LOCATING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let cfg_init = config::get();
        let mut final_city = cfg_init.weather_city.clone();
        let mut final_street = cfg_init.weather_street.clone();
        let mut final_lat = cfg_init.weather_lat;
        let mut final_lon = cfg_init.weather_lon;
        let mut matched = false;

        if let Some((c, st, la, lo, _, _, _)) = match_preset_location(&q) {
            final_city = c.to_owned();
            final_street = st.to_owned();
            final_lat = la;
            final_lon = lo;
            matched = true;
        }

        #[cfg(windows)]
        if !matched {
            let url = format!(
                "https://nominatim.openstreetmap.org/search?format=jsonv2&addressdetails=1&limit=1&accept-language=zh-CN&q={}",
                url_encode_utf8(&q)
            );
            if let Some(arr) = ureq_get_json(&url, 4) {
                if let Some(first) = arr.as_array().and_then(|a| a.first()) {
                    let la = first
                        .get("lat")
                        .and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(final_lat);
                    let lo = first
                        .get("lon")
                        .and_then(|x| x.as_str())
                        .and_then(|s| s.parse::<f64>().ok())
                        .unwrap_or(final_lon);
                    let mut c_name = String::new();
                    let mut s_name = String::new();
                    if let Some(addr) = first.get("address") {
                        c_name = addr
                            .get("city")
                            .or_else(|| addr.get("county"))
                            .or_else(|| addr.get("state"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_owned();
                        let dist = addr
                            .get("suburb")
                            .or_else(|| addr.get("city_district"))
                            .or_else(|| addr.get("district"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("");
                        let rd = addr
                            .get("road")
                            .or_else(|| addr.get("neighbourhood"))
                            .or_else(|| addr.get("quarter"))
                            .and_then(|x| x.as_str())
                            .unwrap_or("");
                        s_name = match (!dist.is_empty(), !rd.is_empty()) {
                            (true, true) => format!("{dist} · {rd}"),
                            (true, false) => dist.to_owned(),
                            (false, true) => rd.to_owned(),
                            (false, false) => q.clone(),
                        };
                    }
                    if !c_name.is_empty() {
                        final_city = c_name;
                    }
                    final_street = if is_valid_street_label(&s_name) {
                        s_name
                    } else {
                        default_street_for_city(&final_city, la, lo)
                    };
                    final_lat = la;
                    final_lon = lo;
                    matched = true;
                }
            }
        }

        if !matched && is_valid_street_label(&q) {
            final_street = q.clone();
        }
        STREET_LOCATING.store(false, Ordering::SeqCst);
        apply_street_selection(&final_city, &final_street, final_lat, final_lon);
    });
}

/// 异步刷新街道级实时天气、24h逐时曲线与 5 日预报（支持内置源与自定义天气源 URL 模板）。
pub fn refresh_weather_async(city: String) {
    let cfg = config::get();
    let city_name = if city.trim().is_empty() {
        if cfg.weather_city.trim().is_empty() {
            "长沙市".to_owned()
        } else {
            cfg.weather_city.trim().to_owned()
        }
    } else {
        city.trim().to_owned()
    };
    let street_name = if is_valid_street_label(&cfg.weather_street) {
        cfg.weather_street.trim().to_owned()
    } else {
        default_street_for_city(&city_name, cfg.weather_lat, cfg.weather_lon)
    };
    let (lat, lon) = if cfg.weather_lat.abs() > 0.01 || cfg.weather_lon.abs() > 0.01 {
        (cfg.weather_lat, cfg.weather_lon)
    } else if let Some((_, _, la, lo, _, _, _)) = match_preset_location(&format!("{city_name} {street_name}")) {
        (la, lo)
    } else {
        (28.1812, 112.9445)
    };
    refresh_weather_for_street_async(city_name, street_name, lat, lon);
}

pub fn refresh_weather_for_street_async(city_name: String, street_name: String, lat: f64, lon: f64) {
    let req_id = WEATHER_REQ_GEN.fetch_add(1, Ordering::SeqCst) + 1;
    WEATHER_FETCHING.store(true, Ordering::SeqCst);
    std::thread::spawn(move || {
        let cfg = config::get();
        let (fallback_t, fallback_desc, fallback_hum) = preset_metrics_for(&city_name, &street_name);
        let now_h = Local::now().hour();
        let src_name = match cfg.weather_source {
            1 => "和风/中国天气".to_owned(),
            2 => "Wttr.in".to_owned(),
            3 => "彩云街道源".to_owned(),
            4 => {
                if cfg.weather_custom_name.trim().is_empty() {
                    "自定义源".to_owned()
                } else {
                    cfg.weather_custom_name.trim().to_owned()
                }
            }
            _ => "Open-Meteo".to_owned(),
        };
        let valid_street = if is_valid_street_label(&street_name) {
            street_name.clone()
        } else {
            default_street_for_city(&city_name, lat, lon)
        };
        let mut st = WislandWeatherState {
            city: city_name.clone(),
            street: valid_street.clone(),
            lat,
            lon,
            desc: fallback_desc.to_owned(),
            temp: fallback_t,
            feels_like: fallback_t - 1,
            humidity: fallback_hum,
            wind: "东南风 2级 · 12km/h".into(),
            pressure: "1015 hPa".into(),
            aqi: "优 26".into(),
            aqi_num: 26,
            uv: "弱 UV2".into(),
            sun_rise_set: "06:44 日出 · 17:36 日落".into(),
            life_tip: "街道空气洁净适宜出行 · 午后舒适宜人".into(),
            source_name: src_name.clone(),
            updated_at: Local::now().format("%H:%M 更新").to_string(),
            locate_status: format!("{} · {} ({lat:.4}, {lon:.4})", city_name, valid_street),
            hourly: build_hourly_realistic(fallback_t, fallback_t - 5, fallback_t + 3, now_h, lat, lon),
            forecast: vec![
                DailyForecast { day_label: "今天".into(), desc: fallback_desc.to_owned(), t_min: fallback_t - 5, t_max: fallback_t + 3 },
                DailyForecast { day_label: "明天".into(), desc: "多云".into(), t_min: fallback_t - 6, t_max: fallback_t + 2 },
                DailyForecast { day_label: "后天".into(), desc: "晴间多云".into(), t_min: fallback_t - 5, t_max: fallback_t + 3 },
                DailyForecast { day_label: "大后天".into(), desc: "小雨".into(), t_min: fallback_t - 7, t_max: fallback_t - 1 },
                DailyForecast { day_label: "第5天".into(), desc: "晴朗".into(), t_min: fallback_t - 6, t_max: fallback_t + 2 },
            ],
            synced_from_live: true,
        };

        let mut got_hourly_from_net = false;
        let mut got_daily_from_net = false;

        #[cfg(windows)]
        {
            let key = cfg.weather_api_key.trim();
            let caiyun_token = if key.is_empty() { "TAkhjf8d1nlSlspN" } else { key };
            let url = if cfg.weather_source == 4 && !cfg.weather_custom_url.trim().is_empty() {
                cfg.weather_custom_url
                    .trim()
                    .replace("{lat}", &format!("{lat:.4}"))
                    .replace("{lon}", &format!("{lon:.4}"))
                    .replace("{city}", &url_encode_utf8(&city_name))
                    .replace("{street}", &url_encode_utf8(&valid_street))
                    .replace("{key}", key)
            } else if cfg.weather_source == 1 && !key.is_empty() {
                format!("https://devapi.qweather.com/v7/weather/now?location={lon:.4},{lat:.4}&key={key}")
            } else if cfg.weather_source == 2 {
                format!("https://wttr.in/{lat:.4},{lon:.4}?format=j1&lang=zh")
            } else if cfg.weather_source == 3 {
                format!("https://api.caiyunapp.com/v2.6/{caiyun_token}/{lon:.4},{lat:.4}/realtime")
            } else {
                format!(
                    "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,surface_pressure&hourly=temperature_2m&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset&timezone=auto&forecast_days=7"
                )
            };

            let parse_open_meteo_into = |v: &serde_json::Value,
                                         st: &mut WislandWeatherState,
                                         override_cur: bool,
                                         got_h: &mut bool,
                                         got_d: &mut bool| {
                if override_cur {
                    if let Some(cur) = v.get("current") {
                        if let Some(t) = cur.get("temperature_2m").and_then(|x| x.as_f64()) {
                            st.temp = t.round() as i32;
                        }
                        if let Some(fl) = cur.get("apparent_temperature").and_then(|x| x.as_f64()) {
                            st.feels_like = fl.round() as i32;
                        }
                        if let Some(h) = cur.get("relative_humidity_2m").and_then(|x| x.as_u64()) {
                            st.humidity = h.clamp(0, 100) as u8;
                        }
                        if let Some(code) = cur.get("weather_code").and_then(|x| x.as_i64()) {
                            st.desc = wmo_code_desc(code)
                                .trim_start_matches(|c: char| !c.is_alphabetic() && !('\u{4e00}'..='\u{9fff}').contains(&c))
                                .trim()
                                .to_owned();
                        }
                        if let Some(ws) = cur.get("wind_speed_10m").and_then(|x| x.as_f64()) {
                            st.wind = format!("实时风速 · {:.0}km/h", ws);
                        }
                        if let Some(pr) = cur.get("surface_pressure").and_then(|x| x.as_f64()) {
                            st.pressure = format!("{:.0} hPa", pr);
                        }
                    }
                }
                if let Some(hourly) = v.get("hourly").and_then(|h| h.get("temperature_2m")).and_then(|x| x.as_array()) {
                    let h_times = v.get("hourly").and_then(|h| h.get("time")).and_then(|x| x.as_array());
                    let start_idx = h_times
                        .and_then(|times| {
                            let suffix = format!("T{now_h:02}:00");
                            times.iter().position(|t| t.as_str().is_some_and(|s| s.ends_with(&suffix)))
                        })
                        .unwrap_or((now_h as usize).min(hourly.len().saturating_sub(1)));
                    let mut hp = Vec::with_capacity(8);
                    for i in 0..8 {
                        let idx = (start_idx + i * 3).min(hourly.len().saturating_sub(1));
                        if let Some(tv) = hourly.get(idx).and_then(|x| x.as_f64()) {
                            let h = (now_h + i as u32 * 3) % 24;
                            hp.push(HourlyPoint {
                                hour_label: if i == 0 { "现在".into() } else { format!("{h:02}:00") },
                                temp: if i == 0 { st.temp } else { tv.round() as i32 },
                            });
                        }
                    }
                    if hp.len() >= 4 {
                        st.hourly = hp;
                        *got_h = true;
                    }
                }
                if let Some(daily) = v.get("daily") {
                    if let (Some(sr), Some(ss)) = (
                        daily.get("sunrise").and_then(|x| x.as_array()).and_then(|a| a.first()).and_then(|x| x.as_str()).and_then(|s| s.get(11..16)),
                        daily.get("sunset").and_then(|x| x.as_array()).and_then(|a| a.first()).and_then(|x| x.as_str()).and_then(|s| s.get(11..16)),
                    ) {
                        st.sun_rise_set = format!("{sr} 日出 · {ss} 日落");
                    }
                    let codes = daily.get("weather_code").and_then(|x| x.as_array());
                    let tmaxs = daily.get("temperature_2m_max").and_then(|x| x.as_array());
                    let tmins = daily.get("temperature_2m_min").and_then(|x| x.as_array());
                    if let (Some(codes), Some(tmaxs), Some(tmins)) = (codes, tmaxs, tmins) {
                        let labels = ["今天", "明天", "后天", "大后天", "第5天", "第6天", "第7天"];
                        let mut fc = Vec::new();
                        for i in 0..7.min(codes.len()).min(tmaxs.len()).min(tmins.len()) {
                            let c = codes[i].as_i64().unwrap_or(1);
                            let hi = tmaxs[i].as_f64().unwrap_or(20.0).round() as i32;
                            let lo = tmins[i].as_f64().unwrap_or(12.0).round() as i32;
                            fc.push(DailyForecast {
                                day_label: labels[i].to_owned(),
                                desc: wmo_code_desc(c).to_owned(),
                                t_min: lo,
                                t_max: hi,
                            });
                        }
                        if !fc.is_empty() {
                            st.forecast = fc;
                            *got_d = true;
                        }
                    }
                }
            };

            if let Some(v) = ureq_get_json(&url, 4) {
                parse_open_meteo_into(&v, &mut st, true, &mut got_hourly_from_net, &mut got_daily_from_net);
                // 2) 和风天气 QWeather (`now.temp` / `now.text`) 格式
                if let Some(now_obj) = v.get("now") {
                    if let Some(t) = now_obj.get("temp").and_then(|x| x.as_str()).and_then(|s| s.parse::<f64>().ok()) {
                        st.temp = t.round() as i32;
                    }
                    if let Some(fl) = now_obj.get("feelsLike").and_then(|x| x.as_str()).and_then(|s| s.parse::<f64>().ok()) {
                        st.feels_like = fl.round() as i32;
                    }
                    if let Some(txt) = now_obj.get("text").and_then(|x| x.as_str()) {
                        st.desc = txt.to_owned();
                    }
                    if let Some(hum) = now_obj.get("humidity").and_then(|x| x.as_str()).and_then(|s| s.parse::<u8>().ok()) {
                        st.humidity = hum.min(100);
                    }
                }
                // 3) 彩云天气 (`result.realtime`) 格式
                if let Some(rt) = v.pointer("/result/realtime") {
                    if let Some(t) = rt.get("temperature").and_then(|x| x.as_f64()) {
                        st.temp = t.round() as i32;
                    }
                    if let Some(fl) = rt.get("apparent_temperature").and_then(|x| x.as_f64()) {
                        st.feels_like = fl.round() as i32;
                    }
                    if let Some(hum) = rt.get("humidity").and_then(|x| x.as_f64()) {
                        let hum_pct = if hum <= 1.0 { (hum * 100.0).round() as u8 } else { hum.round() as u8 };
                        st.humidity = hum_pct.min(100);
                    }
                    if let Some(aqi_v) = rt.pointer("/air_quality/aqi/chn").and_then(|x| x.as_u64()) {
                        st.aqi_num = aqi_v as u16;
                        st.aqi = format!("AQI {aqi_v}");
                    }
                }
                // 4) OpenWeatherMap (`main.temp`) 或通用自定义 JSON (`temp` / `temperature`) 格式
                if let Some(main_obj) = v.get("main") {
                    if let Some(t) = main_obj.get("temp").and_then(|x| x.as_f64()) {
                        st.temp = (if t > 150.0 { t - 273.15 } else { t }).round() as i32;
                    }
                } else if let Some(t) = v.get("temp").or_else(|| v.get("temperature")).and_then(|x| x.as_f64()) {
                    st.temp = t.round() as i32;
                }
                if let Some(d_str) = v.get("desc").or_else(|| v.get("weather")).and_then(|x| x.as_str()) {
                    st.desc = d_str.to_owned();
                }
            }

            // 若当前选择了彩云/和风/Wttr 等仅返回实况的源，自动用街道经纬度请求 Open-Meteo 补齐真实的 24h 街道逐时趋势与多日预报
            if !got_hourly_from_net || !got_daily_from_net {
                let om_url = format!(
                    "https://api.open-meteo.com/v1/forecast?latitude={lat:.4}&longitude={lon:.4}&current=temperature_2m,relative_humidity_2m,apparent_temperature,weather_code,wind_speed_10m,surface_pressure&hourly=temperature_2m&daily=weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset&timezone=auto&forecast_days=7"
                );
                if let Some(om_v) = ureq_get_json(&om_url, 4) {
                    let override_cur = cfg.weather_source == 0;
                    parse_open_meteo_into(&om_v, &mut st, override_cur, &mut got_hourly_from_net, &mut got_daily_from_net);
                }
            }
        }

        // 确保 24h 街道逐时趋势首点（「现在」）严格等于当前实况温度，离线或接口未返回 hourly 时按实况温度与昼夜温差动态生成
        if !got_hourly_from_net || st.hourly.is_empty() {
            let t_min = st.forecast.first().map(|d| d.t_min).unwrap_or(st.temp - 4);
            let t_max = st.forecast.first().map(|d| d.t_max).unwrap_or(st.temp + 3);
            st.hourly = build_hourly_realistic(st.temp, t_min, t_max, now_h, lat, lon);
        } else if let Some(first) = st.hourly.first_mut() {
            let delta = st.temp - first.temp;
            first.temp = st.temp;
            if delta.abs() >= 2 {
                for (idx, pt) in st.hourly.iter_mut().enumerate().skip(1) {
                    let weight = (1.0 - idx as f32 * 0.15).clamp(0.0, 0.75);
                    pt.temp += (delta as f32 * weight).round() as i32;
                }
            }
        }

        if WEATHER_REQ_GEN.load(Ordering::SeqCst) == req_id {
            config::update(|c| {
                c.weather_city = st.city.clone();
                c.weather_street = st.street.clone();
                c.weather_lat = st.lat;
                c.weather_lon = st.lon;
                c.weather_desc = st.desc.clone();
                c.weather_temp = st.temp;
            });
            if let Ok(mut slot) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
                *slot = st;
            }
            bump_street_weather_seq();
            WEATHER_FETCHING.store(false, Ordering::SeqCst);
        }
    });
}

#[derive(Clone, Debug)]
pub struct WislandTranslateState {
    pub input: String,
    pub output: String,
    /// 0 = 自动中英互译, 1 = 译为中文(zh-CN), 2 = 译为英文(en), 3 = 译为日文(ja)
    pub target_mode: u8,
    pub status: String,
}

impl Default for WislandTranslateState {
    fn default() -> Self {
        Self {
            input: "Dynamic Island with Street-Level Weather & Pinned Translate".into(),
            output: "带有街道级精准天气与可冻结翻译面板的灵动岛".into(),
            target_mode: 0,
            status: "就绪 · 支持剪贴板自动识别与中/英/日互译".into(),
        }
    }
}

static WISLAND_TRANSLATE: OnceLock<Mutex<WislandTranslateState>> = OnceLock::new();
static TRANSLATE_RUNNING: AtomicBool = AtomicBool::new(false);

pub fn translate_state() -> &'static Mutex<WislandTranslateState> {
    WISLAND_TRANSLATE.get_or_init(|| Mutex::new(WislandTranslateState::default()))
}

fn builtin_quick_dict(src: &str, to_en: bool) -> Option<String> {
    let s = src.trim();
    if s.is_empty() {
        return Some(String::new());
    }
    let pairs = [
        ("Dynamic Island with Street-Level Weather & Pinned Translate", "带有街道级精准天气与可冻结翻译面板的灵动岛"),
        ("Dynamic Island with Breezy Weather & Wisland Translate", "带有微风天气与 Wisland 快捷翻译的灵动岛"),
        ("Dynamic Island with Liquid Glass & Wisland widgets", "带有液态玻璃与 Wisland 组件的灵动岛"),
        ("Hello World", "你好，世界"),
        ("Course Schedule", "课程表"),
        ("Weather Forecast", "天气预报"),
        ("System Monitor", "系统监控"),
    ];
    for (en, zh) in pairs {
        if s.eq_ignore_ascii_case(en) {
            return Some(if to_en { en.to_owned() } else { zh.to_owned() });
        }
        if s == zh {
            return Some(if to_en { en.to_owned() } else { zh.to_owned() });
        }
    }
    None
}

/// 返回 AI 翻译服务商预设的 (名称, 默认接口地址, 默认模型)
pub fn ai_provider_preset(provider: u8) -> (&'static str, &'static str, &'static str) {
    match provider {
        0 => ("DeepSeek", "https://api.deepseek.com/v1/chat/completions", "deepseek-chat"),
        1 => ("OpenAI", "https://api.openai.com/v1/chat/completions", "gpt-4o-mini"),
        2 => ("SiliconFlow 硅基流动", "https://api.siliconflow.cn/v1/chat/completions", "Qwen/Qwen2.5-7B-Instruct"),
        3 => ("Google Gemini", "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions", "gemini-2.0-flash"),
        4 => ("Anthropic Claude", "https://api.anthropic.com/v1/messages", "claude-3-5-haiku-latest"),
        5 => ("本地 Ollama (免Key)", "http://127.0.0.1:11434/v1/chat/completions", "qwen2.5:7b"),
        _ => ("自定义 OpenAI 兼容接口", "https://api.deepseek.com/v1/chat/completions", "deepseek-chat"),
    }
}

/// 异步执行快捷翻译（支持 AiNiee 模式 AI 大模型 API 翻译 / Google GTX / MyMemory / 离线词典混合引擎）。
pub fn trigger_translate_async(input_text: String, target_mode: u8) {
    let trimmed = input_text.trim().to_owned();
    if trimmed.is_empty() {
        if let Ok(mut st) = translate_state().lock() {
            st.output.clear();
            st.status = "请输入或从剪贴板粘贴要翻译的文本".into();
        }
        return;
    }
    if TRANSLATE_RUNNING.swap(true, Ordering::SeqCst) {
        return;
    }
    if let Ok(mut st) = translate_state().lock() {
        st.input = trimmed.clone();
        st.target_mode = target_mode;
        st.status = "正在翻译…".into();
    }
    std::thread::spawn(move || {
        let cfg = config::get();
        let has_cjk = trimmed.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c));
        let (tl, target_lang_cn) = match target_mode {
            1 => ("zh-CN", "简体中文"),
            2 => ("en", "English"),
            3 => ("ja", "日本語"),
            _ => if has_cjk { ("en", "English") } else { ("zh-CN", "简体中文") },
        };
        let mut engine_name = match cfg.translate_engine {
            0 => format!("AI 大模型 ({})", cfg.translate_ai_model),
            1 => "Google GTX".to_owned(),
            2 => "MyMemory".to_owned(),
            _ => "混合词典".to_owned(),
        };
        let mut result_text: Option<String> = None;

        // 1. AiNiee 模式 AI 大模型 API 翻译（当选择 AI 引擎且已配置 API Key 或使用本地 Ollama 时直接请求大模型）
        #[cfg(windows)]
        if cfg.translate_engine == 0 && (!cfg.translate_ai_api_key.trim().is_empty() || cfg.translate_ai_provider == 5) {
            use std::os::windows::process::CommandExt;
            let (_, def_url, def_model) = ai_provider_preset(cfg.translate_ai_provider);
            let url = if cfg.translate_ai_base_url.trim().is_empty() {
                def_url.to_owned()
            } else {
                cfg.translate_ai_base_url.trim().to_owned()
            };
            let model = if cfg.translate_ai_model.trim().is_empty() {
                def_model.to_owned()
            } else {
                cfg.translate_ai_model.trim().to_owned()
            };
            let sys_prompt = if cfg.translate_ai_prompt.trim().is_empty() {
                "你是一个专业、精准的 AI 翻译引擎（AiNiee 模式）。请直接输出准确、自然流畅的译文，不要附加任何解释。".to_owned()
            } else {
                cfg.translate_ai_prompt.trim().to_owned()
            };
            let user_prompt = format!("请将以下文本准确翻译为 {target_lang_cn}，只输出译文正文：\n{trimmed}");
            let payload = if cfg.translate_ai_provider == 4 || url.contains("/v1/messages") {
                serde_json::json!({
                    "model": model,
                    "max_tokens": 1024,
                    "system": sys_prompt,
                    "messages": [{"role": "user", "content": user_prompt}]
                })
            } else {
                serde_json::json!({
                    "model": model,
                    "temperature": 0.25,
                    "messages": [
                        {"role": "system", "content": sys_prompt},
                        {"role": "user", "content": user_prompt}
                    ]
                })
            };
            {
                let key = cfg.translate_ai_api_key.trim();
                let is_anthropic = cfg.translate_ai_provider == 4 || url.contains("anthropic.com");
                {
                    {
                        if let Some(v) = ureq_post_json(&url, &payload, key, is_anthropic, 8) {
                            let content = v
                                .pointer("/choices/0/message/content")
                                .and_then(|x| x.as_str())
                                .or_else(|| v.pointer("/content/0/text").and_then(|x| x.as_str()));
                            if let Some(txt) = content {
                                let clean = txt.trim().to_owned();
                                if !clean.is_empty() {
                                    result_text = Some(clean);
                                }
                            }
                        }
                    }
                }
            }
        }

        if result_text.is_none() {
            result_text = builtin_quick_dict(&trimmed, tl == "en");
        }

        // 2. Google GTX 极速翻译通道（或 AI 未填 Key 时的智能保底）
        #[cfg(windows)]
        if result_text.is_none() && cfg.translate_engine != 3 {
            use std::os::windows::process::CommandExt;
            let encoded = url_encode_utf8(&trimmed);
            let url = format!(
                "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={tl}&dt=t&q={encoded}"
            );
            {
                {
                    if let Some(v) = ureq_get_json(&url, 4) {
                        if let Some(arr) = v.get(0).and_then(|x| x.as_array()) {
                            let mut joined = String::new();
                            for seg in arr {
                                if let Some(piece) = seg.get(0).and_then(|x| x.as_str()) {
                                    joined.push_str(piece);
                                }
                            }
                            if !joined.trim().is_empty() {
                                if cfg.translate_engine == 0 && cfg.translate_ai_api_key.trim().is_empty() && cfg.translate_ai_provider != 5 {
                                    engine_name = "AI 智能通道 (Google GTX 保底)".to_owned();
                                }
                                result_text = Some(joined);
                            }
                        }
                    }
                }
            }
        }

        let final_out = result_text.unwrap_or_else(|| {
            if tl == "en" {
                format!("[EN] {trimmed}")
            } else if tl == "ja" {
                format!("[JA] {trimmed}")
            } else {
                format!("[译] {trimmed}")
            }
        });
        if let Ok(mut st) = translate_state().lock() {
            st.output = final_out;
            st.status = format!("翻译完成 · {engine_name} → {tl}");
        }
        TRANSLATE_RUNNING.store(false, Ordering::SeqCst);
    });
}

#[cfg(windows)]
pub fn read_win32_clipboard_text() -> Option<String> {
    #[link(name = "user32")]
    extern "system" {
        fn OpenClipboard(hwnd: isize) -> i32;
        fn CloseClipboard() -> i32;
        fn GetClipboardData(u_format: u32) -> isize;
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalLock(h_mem: isize) -> *const u16;
        fn GlobalUnlock(h_mem: isize) -> i32;
    }
    unsafe {
        if OpenClipboard(0) == 0 {
            return None;
        }
        let h = GetClipboardData(13); // CF_UNICODETEXT = 13
        let mut out = None;
        if h != 0 {
            let ptr = GlobalLock(h);
            if !ptr.is_null() {
                let mut len = 0usize;
                while len < 4096 && *ptr.add(len) != 0 {
                    len += 1;
                }
                let slice = std::slice::from_raw_parts(ptr, len);
                let s = String::from_utf16_lossy(slice).trim().to_owned();
                if !s.is_empty() {
                    out = Some(s);
                }
                let _ = GlobalUnlock(h);
            }
        }
        let _ = CloseClipboard();
        out
    }
}

#[cfg(not(windows))]
pub fn read_win32_clipboard_text() -> Option<String> {
    None
}

// ---------------------------------------------------------------
// 18 套精选主题预设与自定义强调色系统
// ---------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
pub struct WinTheme {
    pub id: u8,
    pub name: &'static str,
    pub en: &'static str,
    pub paper: [u8; 3],
    pub ink: [u8; 3],
    pub accent: [u8; 3],
    pub light: bool,
}

pub const WIN_THEMES: [WinTheme; 18] = [
    WinTheme { id: 1,  name: "曜石暗夜",     en: "Obsidian",      paper: [13, 15, 20],    ink: [248, 250, 252], accent: [59, 130, 246],  light: false },
    WinTheme { id: 2,  name: "极地冰川",     en: "Arctic Frost",  paper: [236, 245, 252], ink: [20, 32, 46],    accent: [2, 132, 199],   light: true  },
    WinTheme { id: 3,  name: "赛博霓虹",     en: "Cyberpunk",     paper: [18, 11, 34],    ink: [242, 248, 255], accent: [0, 240, 255],   light: false },
    WinTheme { id: 4,  name: "樱花微雨",     en: "Sakura Mist",   paper: [38, 22, 32],    ink: [255, 240, 246], accent: [244, 114, 182], light: false },
    WinTheme { id: 5,  name: "深海极光",     en: "Ocean Aurora",  paper: [10, 28, 38],    ink: [236, 254, 255], accent: [45, 212, 191],  light: false },
    WinTheme { id: 6,  name: "落日熔金",     en: "Sunset Amber",  paper: [36, 20, 16],    ink: [255, 247, 237], accent: [245, 158, 11],  light: false },
    WinTheme { id: 7,  name: "翡翠森林",     en: "Emerald",       paper: [12, 30, 22],    ink: [236, 253, 245], accent: [16, 185, 129],  light: false },
    WinTheme { id: 8,  name: "薰衣草梦境",   en: "Lavender",      paper: [26, 21, 40],    ink: [245, 243, 255], accent: [167, 139, 250], light: false },
    WinTheme { id: 9,  name: "北欧极简雪",   en: "Nordic Snow",   paper: [244, 246, 250], ink: [30, 41, 59],    accent: [71, 85, 105],   light: true  },
    WinTheme { id: 10, name: "抹茶拿铁",     en: "Matcha Latte",  paper: [241, 246, 234], ink: [36, 48, 26],    accent: [77, 124, 15],   light: true  },
    WinTheme { id: 11, name: "星际深空",     en: "Starlight",     paper: [11, 15, 27],    ink: [241, 245, 249], accent: [99, 102, 241],  light: false },
    WinTheme { id: 12, name: "赤焰红莲",     en: "Crimson Lotus", paper: [36, 13, 20],    ink: [255, 241, 242], accent: [244, 63, 94],   light: false },
    WinTheme { id: 13, name: "琥珀终端",     en: "Retro CRT",     paper: [20, 17, 10],    ink: [254, 243, 199], accent: [251, 191, 36],  light: false },
    WinTheme { id: 14, name: "黑金奢华",     en: "Noir Gold",     paper: [18, 18, 19],    ink: [250, 246, 232], accent: [212, 175, 55],  light: false },
    WinTheme { id: 15, name: "薄荷清风",     en: "Mint Breeze",   paper: [236, 253, 248], ink: [19, 46, 42],    accent: [13, 148, 136],  light: true  },
    WinTheme { id: 16, name: "东京夜幕",     en: "Tokyo Night",   paper: [24, 25, 38],    ink: [226, 232, 248], accent: [122, 162, 247], light: false },
    WinTheme { id: 17, name: "莫兰迪暖灰",   en: "Morandi Warm",  paper: [40, 38, 36],    ink: [245, 242, 238], accent: [217, 119, 87],  light: false },
    WinTheme { id: 18, name: "流光幻彩",     en: "Prismatic",     paper: [20, 23, 36],    ink: [250, 248, 255], accent: [236, 72, 153],  light: false },
];

pub const ACCENT_PRESETS: [(&str, &str); 10] = [
    ("Fluent 系统蓝", "#0A84FF"),
    ("珊瑚霓虹红", "#E03E62"),
    ("赛博电光青", "#00F0FF"),
    ("极光翡翠绿", "#10B981"),
    ("落日琥珀橙", "#F59E0B"),
    ("丁香梦幻紫", "#8B5CF6"),
    ("樱花浪漫粉", "#EC4899"),
    ("香槟黑金",   "#D4AF37"),
    ("冰川天际蓝", "#38BDF8"),
    ("薄荷青柠",   "#14B8A6"),
];

pub fn parse_hex_rgba(hex: &str) -> Option<[u8; 4]> {
    let s = hex.trim().trim_start_matches('#');
    if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    if s.len() == 8 {
        Some([
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            u8::from_str_radix(&s[6..8], 16).ok()?,
        ])
    } else if s.len() == 6 {
        Some([
            u8::from_str_radix(&s[0..2], 16).ok()?,
            u8::from_str_radix(&s[2..4], 16).ok()?,
            u8::from_str_radix(&s[4..6], 16).ok()?,
            255,
        ])
    } else {
        None
    }
}

pub fn parse_hex_rgb(hex: &str) -> Option<[u8; 3]> {
    let [r, g, b, _] = parse_hex_rgba(hex)?;
    Some([r, g, b])
}

pub fn active_accent_rgb(cfg: &config::Config) -> [u8; 3] {
    if let Some(rgb) = parse_hex_rgb(&cfg.custom_accent) {
        return rgb;
    }
    if cfg.win_theme >= 1 && (cfg.win_theme as usize) <= WIN_THEMES.len() {
        return WIN_THEMES[cfg.win_theme as usize - 1].accent;
    }
    [10, 132, 255]
}

pub fn active_accent_color() -> Color32 {
    // [第 10 批 · 滚动卡顿] 这个函数被开关、芯片、侧栏、按钮按「每控件每帧」调用，
    // 旧版每次 `config::get()` → 克隆整个 Config（含账号表）。这里按配置代号缓存，
    // 滚动帧里只剩两次原子读，不再反复克隆配置。
    static ACCENT_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    static ACCENT_PACKED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let gen = config::revision();
    if gen != 0 && ACCENT_GEN.load(std::sync::atomic::Ordering::Relaxed) == gen {
        let v = ACCENT_PACKED.load(std::sync::atomic::Ordering::Relaxed);
        let a = v.to_le_bytes();
        return Color32::from_rgba_premultiplied(a[0], a[1], a[2], a[3]);
    }
    let cfg = config::get();
    let col = if let Some([r, g, b, a]) = parse_hex_rgba(&cfg.custom_accent) {
        Color32::from_rgba_unmultiplied(r, g, b, a)
    } else {
        let [r, g, b] = active_accent_rgb(&cfg);
        Color32::from_rgb(r, g, b)
    };
    let a = col.to_array();
    ACCENT_PACKED.store(u32::from_le_bytes(a), std::sync::atomic::Ordering::Relaxed);
    ACCENT_GEN.store(gen, std::sync::atomic::Ordering::Relaxed);
    col
}

pub fn active_win_theme(cfg: &config::Config) -> Option<WinTheme> {
    if cfg.win_theme >= 1 && (cfg.win_theme as usize) <= WIN_THEMES.len() {
        Some(WIN_THEMES[cfg.win_theme as usize - 1])
    } else {
        None
    }
}

pub fn active_win_palette() -> Option<crate::skin_paint::Palette> {
    let cfg = config::get();
    let wt = active_win_theme(&cfg)?;
    let acc = parse_hex_rgb(&cfg.custom_accent).unwrap_or(wt.accent);
    Some(crate::skin_paint::Palette {
        paper: wt.paper,
        ink: wt.ink,
        accent: Some(acc),
        dark_alpha: if wt.light { 0.86 } else { 0.48 },
        lift: if wt.light { 1.0 } else { 0.56 },
        darken: if wt.light { 0.78 } else { 1.0 },
        chip: if wt.light { 0.72 } else { 1.0 },
    })
}

pub fn active_win_fill(default_alpha: u8) -> Option<Color32> {
    let cfg = config::get();
    let wt = active_win_theme(&cfg)?;
    let user_a = ((cfg.glass_opacity.clamp(25, 100) as f32 / 100.0) * 255.0).round() as u8;
    let a = ((default_alpha as u16 * user_a as u16) / 220).clamp(65, 252) as u8;
    let [r, g, b] = wt.paper;
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

// ---------------------------------------------------------------
// 16 种核心实时遥测组件
// ---------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetCat {
    Hardware,
    Power,
    TimeSchedule,
}

impl WidgetCat {
    pub const ALL: [Self; 3] = [
        Self::Hardware,
        Self::Power,
        Self::TimeSchedule,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Hardware => "硬件与性能 (9)",
            Self::Power => "电源与运行 (3)",
            Self::TimeSchedule => "时间、课表与 AI (4)",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WinWidgetDef {
    pub id: u8,
    pub cat: WidgetCat,
    pub name: &'static str,
    pub desc: &'static str,
}

pub const WIN_WIDGETS: [WinWidgetDef; 16] = [
    // 1. 硬件与性能 (9)
    WinWidgetDef { id: 12, cat: WidgetCat::Hardware,     name: "CPU 占用",      desc: "处理器实时总负载百分比与核心数" },
    WinWidgetDef { id: 13, cat: WidgetCat::Hardware,     name: "CPU 频率",      desc: "处理器当前实时运行主频 (GHz)" },
    WinWidgetDef { id: 14, cat: WidgetCat::Hardware,     name: "CPU 温度",      desc: "处理器传感器实时温度 (℃)" },
    WinWidgetDef { id: 15, cat: WidgetCat::Hardware,     name: "GPU 占用",      desc: "显卡 3D 引擎实时利用率" },
    WinWidgetDef { id: 16, cat: WidgetCat::Hardware,     name: "显存/共享内存", desc: "独显专用显存或核显共享系统内存" },
    WinWidgetDef { id: 17, cat: WidgetCat::Hardware,     name: "内存占用",      desc: "物理内存占用率与容量 (含核显共享识别)" },
    WinWidgetDef { id: 18, cat: WidgetCat::Hardware,     name: "磁盘读写",      desc: "硬盘实时读取/写入总吞吐速率" },
    WinWidgetDef { id: 19, cat: WidgetCat::Hardware,     name: "网络速率",      desc: "上下行实时网速合并显示 (↓下载 / ↑上传)" },
    WinWidgetDef { id: 21, cat: WidgetCat::Hardware,     name: "活跃进程",      desc: "系统当前运行进程总数" },
    // 2. 电源与运行 (3)
    WinWidgetDef { id: 22, cat: WidgetCat::Power,        name: "电池电量",      desc: "本机电池剩余电量百分比与充放电状态" },
    WinWidgetDef { id: 23, cat: WidgetCat::Power,        name: "供电状态",      desc: "交流电 AC / 电池供电模式" },
    WinWidgetDef { id: 24, cat: WidgetCat::Power,        name: "系统开机",      desc: "本次系统连续运行时长" },
    // 3. 时间、课表与 AI (4)
    WinWidgetDef { id: 0,  cat: WidgetCat::TimeSchedule, name: "数字时钟",      desc: "当前小时、分钟与实时秒数" },
    WinWidgetDef { id: 2,  cat: WidgetCat::TimeSchedule, name: "今日日期",      desc: "当前月/日与星期显示" },
    WinWidgetDef { id: 29, cat: WidgetCat::TimeSchedule, name: "下节课预告",    desc: "今日最近课程与当前第N周" },
    WinWidgetDef { id: 32, cat: WidgetCat::TimeSchedule, name: "AI 状态",       desc: "本机 AI 助手实时工作状态" },
];

pub fn is_valid_widget_id(id: u8) -> bool {
    WIN_WIDGETS.iter().any(|w| w.id == id)
}

/// 计算单个组件的（简短标签, 主数值, 副文本, 强调色, 进度比）。
pub fn eval_win_widget(id: u8, s: &sysmon::Sample, _cfg: &config::Config, _ppp: f32) -> (&'static str, String, String, Color32, Option<f32>) {
    let now = Local::now();
    let acc = active_accent_color();
    let ded_vram = s.vram.unwrap_or(0);
    let sh_vram = s.vram_shared.unwrap_or(0);
    let hw = crate::graphics::probe_gpu_hardware_info();
    let is_igpu = !hw.adapter_is_discrete && !hw.adapter_name.is_empty();

    match id {
        0 => ("时间", format!("{:02}:{:02}", now.hour(), now.minute()), format!("{:02}秒", now.second()), acc, None),
        2 => {
            let wd = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"][now.weekday().num_days_from_monday() as usize];
            ("日期", format!("{}月{}日", now.month(), now.day()), wd.to_owned(), Color32::from_rgb(251, 146, 60), None)
        }
        12 => {
            let clean_cpu = hw
                .cpu_name
                .replace("(R)", "")
                .replace("(TM)", "")
                .replace("CPU @", "")
                .replace("with Radeon Graphics", "")
                .replace("Processor", "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let short_cpu = clean_cpu
                .strip_prefix("Intel ")
                .or_else(|| clean_cpu.strip_prefix("AMD "))
                .unwrap_or(&clean_cpu)
                .trim();
            let short_cpu = short_cpu.strip_prefix("Core ").unwrap_or(short_cpu).trim();
            let cpu_short = if !short_cpu.is_empty() && short_cpu != "未知处理器" {
                let abbr: String = short_cpu.chars().take(18).collect();
                format!("{} · {}核", abbr.trim_end(), s.cores.len().max(1))
            } else {
                format!("{} 逻辑核心", s.cores.len().max(1))
            };
            ("CPU", format!("{:.0}%", s.cpu), cpu_short, Color32::from_rgb(59, 130, 246), Some(s.cpu / 100.0))
        }
        13 => ("主频", s.cpu_mhz.map(sysmon::mhz).unwrap_or_else(|| "3.6 GHz".to_owned()), format!("CPU {:.0}%", s.cpu), Color32::from_rgb(96, 165, 250), Some(s.cpu / 100.0)),
        14 => ("温度", sysmon::opt(s.temp_c, "℃", 0), "CPU 封装".to_owned(), Color32::from_rgb(249, 115, 22), s.temp_c.map(|t| (t / 100.0).clamp(0.0, 1.0))),
        15 => {
            let clean_gpu = hw
                .adapter_name
                .replace("(R)", "")
                .replace("(TM)", "")
                .replace("NVIDIA GeForce ", "")
                .replace("AMD Radeon ", "Radeon ")
                .replace("Laptop GPU", "")
                .replace("GPU", "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            let gpu_short = if !clean_gpu.is_empty() && clean_gpu != "硬件加速显卡" {
                clean_gpu.chars().take(20).collect::<String>().trim_end().to_owned()
            } else if is_igpu {
                "核芯显卡 3D".to_owned()
            } else {
                "独立显卡 3D".to_owned()
            };
            (
                "GPU",
                s.gpu.map(|g| format!("{g:.0}%")).unwrap_or_else(|| "0%".to_owned()),
                gpu_short,
                Color32::from_rgb(168, 85, 247),
                s.gpu.map(|g| g / 100.0),
            )
        }
        16 => {
            if is_igpu {
                let used_igpu = sh_vram.max(ded_vram);
                let val_str = if used_igpu > 0 {
                    sysmon::bytes(used_igpu)
                } else {
                    "内存共用".to_owned()
                };
                let ratio = if s.mem_total > 0 && used_igpu > 0 {
                    Some((used_igpu as f32 / (s.mem_total as f32 * 0.5)).clamp(0.04, 1.0))
                } else {
                    None
                };
                let igpu_sub = if hw.adapter_name.to_ascii_lowercase().contains("arc") {
                    "Intel Arc 共享显存".to_owned()
                } else {
                    "核显共享系统显存".to_owned()
                };
                ("显存", val_str, igpu_sub, Color32::from_rgb(192, 132, 252), ratio)
            } else {
                let ratio = if hw.adapter_vram_bytes > 0 {
                    Some((ded_vram as f32 / hw.adapter_vram_bytes as f32).clamp(0.02, 1.0))
                } else {
                    None
                };
                let sub = if hw.adapter_vram_bytes > 0 {
                    format!("总计 {}", sysmon::bytes(hw.adapter_vram_bytes))
                } else {
                    format!("共享 {}", sysmon::bytes(sh_vram))
                };
                ("显存", sysmon::bytes(ded_vram), sub, Color32::from_rgb(192, 132, 252), ratio)
            }
        }
        17 => {
            let ratio = if s.mem_total > 0 { s.mem_used as f32 / s.mem_total as f32 } else { 0.45 };
            let sub = if s.mem_total > 0 {
                format!("{}/{}", sysmon::bytes(s.mem_used), sysmon::bytes(s.mem_total))
            } else {
                sysmon::bytes(s.mem_used)
            };
            ("内存", format!("{:.0}%", ratio * 100.0), sub, Color32::from_rgb(52, 211, 153), Some(ratio))
        }
        18 => ("磁盘", sysmon::rate(s.disk_read + s.disk_write), format!("已用 {}", sysmon::bytes(s.disk_used)), Color32::from_rgb(251, 191, 36), None),
        19 => (
            "网络",
            format!("↓{}", sysmon::rate(s.net_rx)),
            format!("↑{}", sysmon::rate(s.net_tx)),
            Color32::from_rgb(34, 211, 238),
            None,
        ),
        21 => ("进程", format!("{}", s.processes), "活跃进程".to_owned(), Color32::from_rgb(148, 163, 184), None),
        22 => {
            let pct = s.battery.unwrap_or(92);
            ("电量", format!("{pct}%"), if s.charging { "充电中".to_owned() } else { "放电中".to_owned() }, Color32::from_rgb(74, 222, 128), Some(pct as f32 / 100.0))
        }
        23 => {
            let devs = crate::bt::connected_peripherals();
            if let Some(d) = devs.iter().find(|d| d.battery.is_some()).or_else(|| devs.first()) {
                let pct = d.battery.unwrap_or(100);
                ("外设", format!("{pct}%"), d.name.clone(), Color32::from_rgb(56, 189, 248), Some(pct as f32 / 100.0))
            } else {
                ("电源", if s.charging { "AC供电".to_owned() } else { "电池".to_owned() }, "蓝牙/Voltide待机".to_owned(), Color32::from_rgb(74, 222, 128), None)
            }
        }
        24 => ("运行", sysmon::uptime_text(s.uptime), "本次开机".to_owned(), Color32::from_rgb(129, 140, 248), None),
        29 => {
            if let Some(a) = course::alert_detail(now.timestamp()) {
                ("课程", a.title, a.place, Color32::from_rgb(251, 191, 36), Some(a.progress))
            } else {
                ("课程", "今日无课".to_owned(), format!("第{}周", course::week_now()), Color32::from_rgb(251, 191, 36), None)
            }
        }
        _ => {
            let (agents, _) = state::snapshot();
            if let Some(a) = agents.first() {
                ("AI", a.phase.label().to_owned(), a.name.clone(), acc, None)
            } else {
                ("AI", "待机中".to_owned(), "就绪".to_owned(), acc, None)
            }
        }
    }
}

/// 在收起态胶囊下方或顶栏绘制单行紧凑组件条（当用户在设置中开启 `win_compact_bar` 时）。
pub fn draw_win_compact_strip(c: &crate::paint::Card, cx: f32, y: f32, alpha: f32) {
    let cfg = config::get();
    if !cfg.win_compact_bar || alpha <= 0.02 || cfg.win_widgets_order.is_empty() {
        return;
    }
    let s = sysmon::sample();
    let ppp = c.ui.ctx().pixels_per_point();
    let items: Vec<String> = cfg
        .win_widgets_order
        .iter()
        .copied()
        .filter(|&id| is_valid_widget_id(id))
        .take(4)
        .map(|id| {
            let (k, v, _, _, _) = eval_win_widget(id, &s, &cfg, ppp);
            format!("{k} {v}")
        })
        .collect();
    let line = items.join("  ·  ");
    let c = c.faded(alpha);
    c.text(
        cx,
        y,
        &line,
        8.5,
        crate::theme::W::Semibold,
        Color32::from_gray(215),
        crate::paint::H::Center,
    );
}

fn dim_text(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::from_gray(160)
    } else {
        Color32::from_gray(88)
    }
}

fn card_fill(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::from_rgb(35, 35, 38)
    } else {
        Color32::from_rgb(246, 247, 250)
    }
}

fn card_hover(ui: &egui::Ui) -> Color32 {
    if ui.ctx().style().visuals.dark_mode {
        Color32::from_rgb(51, 51, 51)
    } else {
        Color32::from_rgb(236, 238, 245)
    }
}

/// 设置页入口（16 种核心可定制组件系统）。
pub fn draw(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    let before = cfg.clone();

    ui.label(
        RichText::new("自由编排岛内显示的硬件性能、电源、时间、课程与 AI 组件，支持独立调节每个模块的卡片尺寸与布局密度。")
            .size(11.5)
            .color(dim_text(ui)),
    );

    draw_win_widgets_settings_inner(ui, &mut cfg);

    // [编译修复] `preset_switched` 只存在于天气设置页，本函数里从未定义；去掉守卫。
    if cfg != before {
        let manual_weather_edited = cfg.weather_city != before.weather_city
            || cfg.weather_street != before.weather_street
            || (cfg.weather_lat - before.weather_lat).abs() > 1e-5
            || (cfg.weather_lon - before.weather_lon).abs() > 1e-5
            || cfg.weather_desc != before.weather_desc
            || cfg.weather_temp != before.weather_temp;
        let new_cfg = cfg.clone();
        config::update(|c| *c = new_cfg);
        if manual_weather_edited {
            if let Ok(mut g) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
                if !cfg.weather_city.trim().is_empty() {
                    g.city = cfg.weather_city.trim().to_owned();
                }
                if is_valid_street_label(&cfg.weather_street) {
                    g.street = cfg.weather_street.trim().to_owned();
                }
                g.lat = cfg.weather_lat;
                g.lon = cfg.weather_lon;
                if !cfg.weather_desc.trim().is_empty() {
                    g.desc = cfg.weather_desc.trim().to_owned();
                }
                if g.temp != cfg.weather_temp {
                    g.temp = cfg.weather_temp;
                    g.feels_like = cfg.weather_temp - 1;
                    let t_min = g.forecast.first().map(|d| d.t_min).unwrap_or(g.temp - 4);
                    let t_max = g.forecast.first().map(|d| d.t_max).unwrap_or(g.temp + 3);
                    g.hourly = build_hourly_realistic(g.temp, t_min, t_max, Local::now().hour(), g.lat, g.lon);
                }
                g.updated_at = Local::now().format("%H:%M 更新").to_string();
                g.locate_status = format!("{} · {} ({:.4}, {:.4})", g.city, g.street, g.lat, g.lon);
                g.synced_from_live = true;
            }
            bump_street_weather_seq();
            ui.ctx().request_repaint();
        }
    }
}

/// 供「定制组件」页使用的 16 项核心组件定制面板（修复图二下方卡片右侧被裁切的问题）。
pub fn draw_win_widgets_settings(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    let before = cfg.clone();
    draw_win_widgets_settings_inner(ui, &mut cfg);
    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

fn widget_size_label(sz: u8) -> &'static str {
    match sz {
        1 => "2×1 宽卡",
        2 => "全宽横幅",
        _ => "1×1 小卡",
    }
}

fn sanitized_widget_order(order: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for &id in order {
        if is_valid_widget_id(id) && !out.contains(&id) {
            out.push(id);
        }
    }
    if out.is_empty() {
        out = vec![12, 15, 17, 19, 16, 22, 29, 32];
    }
    out
}

fn draw_win_widgets_settings_inner(ui: &mut egui::Ui, cfg: &mut config::Config) {
    let s = sysmon::sample();
    let ppp = ui.ctx().pixels_per_point();
    let acc = active_accent_color();
    if cfg.plugin_sizes.len() < 34 {
        cfg.plugin_sizes.resize(34, 0);
    }
    cfg.win_widgets_order = sanitized_widget_order(&cfg.win_widgets_order);

    let plugin_sub_tab: usize = ui
        .ctx()
        .data(|d| d.get_temp::<usize>(egui::Id::new("auralsle_plugin_sub_tab_v40")))
        .unwrap_or(0)
        .min(2);
    let search_q = ui
        .ctx()
        .data(|d| d.get_temp::<String>(egui::Id::new("settings-sidebar-search")))
        .unwrap_or_default();
    let _ = (plugin_sub_tab, search_q);
    let show_style_sec = true;
    let show_grid_sec = true;
    let show_cards_sec = true;

    if show_style_sec {
    // ── 0) 插件岛 UI 方案选择（Glance 新版 vs 经典网格旧版，需求 4）──
    settings_label(ui, "插件岛界面方案");
    crate::extras::settings_card(ui, |ui| {
        crate::extras::settings_control_row_right(ui, "插件岛 UI 方案", |ui| {
            crate::extras::settings_segmented(
                ui,
                &mut cfg.plugin_ui_mode,
                &[
                    (0u8, "灵动监控"),
                    (1u8, "方案二：经典插件岛 (旧版)"),
                ],
                158.0,
            );
        });
        if cfg.plugin_ui_mode == 0 {
            ui.add_space(4.0);
            crate::extras::settings_control_row_right(ui, "监控面板形态（也可在岛内顶栏切换）", |ui| {
                crate::extras::settings_segmented(
                    ui,
                    &mut cfg.glance_widget_form,
                    &[
                        (0u8, "曲线读数卡 (Curve)"),
                        (1u8, "极简圆环 (Rings)"),
                        (2u8, "硬件分栏 (Lanes)"),
                    ],
                    108.0,
                );
            });
        }
        crate::extras::settings_hint(
            ui,
            if cfg.plugin_ui_mode == 0 {
                "灵动监控：支持「曲线读数卡」、「圆环矩阵」与「硬件监控横条」三种形态切换，含每线程负载与最忙进程。"
            } else {
                "方案二（经典旧版）：保留多宫格自由编排遥测卡片布局。"
            },
        );
    });

    // ── 1) 岛内「插件」1:1 真实岛体预览舞台（需求 9：278px 标准岛高 + 顶栏状态栏避让 + 可直接点击切换形态与翻页） ──
    settings_label(ui, "岛内实时预览");
    crate::extras::settings_card(ui, |ui| {
        let preview_h = 278.0_f32;
        let avail_w = (crate::extras::layout_avail_w(ui) - 8.0).max(180.0);
        let scale_k = ((avail_w - 8.0) / 419.0).clamp(0.46, 1.0);
        let draw_w = 419.0 * scale_k;
        let draw_h = preview_h * scale_k;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, draw_h + 10.0), Sense::hover());
            let island_r = Rect::from_center_size(rect.center(), egui::vec2(draw_w, draw_h));
            let preview_bg = active_win_fill(235).unwrap_or_else(|| {
                if ui.visuals().dark_mode {
                    Color32::from_rgb(32, 32, 32)
                } else {
                    Color32::from_rgb(43, 43, 43)
                }
            });
            ui.painter().rect_filled(island_r, 22.0 * scale_k, preview_bg);
            ui.painter().rect_stroke(
                island_r,
                22.0 * scale_k,
                Stroke::new(1.0, Color32::from_white_alpha(38)),
                egui::StrokeKind::Inside,
            );

            let mut pcard = crate::paint::Card::new(ui, island_r.min, island_r)
                .scaled(0.0, 0.0, scale_k);
            pcard.live = true;
            if cfg.show_island_page_title {
                pcard.text(38.0, 22.5, "插", 15.0, crate::theme::W::Bold, Color32::WHITE, crate::paint::H::Left);
                let w1 = pcard.text_w("插", 15.0, crate::theme::W::Bold);
                pcard.text(38.0 + w1, 22.5, "件", 15.0, crate::theme::W::Bold, acc, crate::paint::H::Left);
            }
            pcard.text(209.5, 22.0, "11月14日 周六  12:00", 11.2, crate::theme::W::Semibold, Color32::from_gray(220), crate::paint::H::Center);
            crate::paint::gear(&pcard, 394.0, 18.0, Color32::from_gray(175));
            let body_card = pcard.shifted(0.0, 22.0);
            if cfg.plugin_ui_mode == 0 {
                draw_shadcn_plugin_island_page(&body_card, cfg, &s);
            } else {
                draw_shadcn_plugin_island_page(&body_card, cfg, &s);
            }
        });
    });

    // ── 2) 插件容器尺寸与显示内容自定义 ──
    settings_label(ui, "面板尺寸与卡片样式");
    crate::extras::settings_card(ui, |ui| {
        let row_avail = (crate::extras::layout_avail_w(ui) - 12.0).max(180.0);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("展开高度：").size(11.5).strong());
            ui.spacing_mut().slider_width = (row_avail - 280.0).clamp(72.0, 170.0);
            let mut h_val = cfg.plugin_island_h as i32;
            if ui.add(egui::Slider::new(&mut h_val, 140..=248).suffix(" px")).changed() {
                cfg.plugin_island_h = h_val as u16;
            }
            for (preset_h, label) in [(156u16, "紧凑 156"), (216u16, "标准 216"), (244u16, "加长 244")] {
                if ui.small_button(label).clicked() {
                    cfg.plugin_island_h = preset_h;
                }
            }
        });

        crate::extras::twinkle_card_break(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("网格列数与布局：").size(11.5).strong());
            for (is_compact, cols_val, label) in [
                (false, 2u8, "2 列宽幅"),
                (false, 3u8, "3 列标准"),
                (false, 4u8, "4 列高密"),
                (true, 3u8, "单行胶囊流"),
            ] {
                let selected = cfg.win_compact_bar == is_compact && (is_compact || cfg.plugin_cols == cols_val);
                let btn = egui::Button::new(
                    RichText::new(label)
                        .size(11.0)
                        .color(if selected { Color32::WHITE } else { Color32::from_gray(195) }),
                )
                .fill(if selected { acc } else { Color32::from_rgb(51, 51, 51) })
                .corner_radius(6.0);
                if ui.add(btn).clicked() {
                    cfg.win_compact_bar = is_compact;
                    if !is_compact {
                        cfg.plugin_cols = cols_val;
                    }
                }
            }
        });

        crate::extras::twinkle_card_break(ui);
        let half_slider_w = ((row_avail - 240.0) * 0.44).clamp(68.0, 142.0);
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().slider_width = half_slider_w;
            ui.label(RichText::new("整体字号缩放：").size(11.5));
            ui.add(egui::Slider::new(&mut cfg.plugin_font_scale, 80..=145).suffix("%"));
            ui.add_space(8.0);
            ui.label(RichText::new("卡片圆角：").size(11.5));
            ui.add(egui::Slider::new(&mut cfg.plugin_card_radius, 4..=20).suffix(" px"));
        });
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("卡片内文字字号独立调节：").size(11.2).strong());
            ui.label(RichText::new("标题").size(11.0).color(dim_text(ui)));
            ui.add(egui::DragValue::new(&mut cfg.plugin_title_size).range(7.0..=15.0).speed(0.2).suffix(" pt"));
            ui.add_space(4.0);
            ui.label(RichText::new("主数值").size(11.0).color(dim_text(ui)));
            ui.add(egui::DragValue::new(&mut cfg.plugin_value_size).range(9.0..=22.0).speed(0.2).suffix(" pt"));
            ui.add_space(4.0);
            ui.label(RichText::new("右下副文本(图5红框)").size(11.0).color(Color32::from_rgb(56, 189, 248)));
            ui.add(egui::DragValue::new(&mut cfg.plugin_sub_size).range(7.5..=15.0).speed(0.2).suffix(" pt"));
            if ui.small_button("恢复默认字号").clicked() {
                cfg.plugin_title_size = 8.8;
                cfg.plugin_value_size = 13.8;
                cfg.plugin_sub_size = 9.4;
                cfg.plugin_font_scale = 100;
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().slider_width = half_slider_w;
            ui.label(RichText::new("底色浓度：").size(11.5));
            ui.add(egui::Slider::new(&mut cfg.plugin_card_opacity, 35..=100).suffix("%"));
            if ui.small_button("恢复标准卡片样式").clicked() {
                cfg.plugin_card_opacity = 85;
                cfg.plugin_card_radius = 10;
            }
            ui.add_space(6.0);
            ui.label(RichText::new("采样间隔：").size(11.5));
            ui.add(
                egui::DragValue::new(&mut cfg.monitor_interval)
                    .range(250..=3000)
                    .speed(50.0)
                    .suffix(" ms"),
            );
        });

        crate::extras::twinkle_card_break(ui);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("显示内容要素：").size(11.5).strong());
            ui.checkbox(&mut cfg.plugin_show_title, "标题与色标");
            ui.checkbox(&mut cfg.plugin_show_value, "核心数值");
            ui.checkbox(&mut cfg.plugin_show_sub, "副标题详情");
            ui.checkbox(&mut cfg.plugin_show_bar, "进度条与 60 秒趋势线");
            ui.checkbox(&mut cfg.monitor_cores, "底部每核负载条");
        });
    });
    } // end show_style_sec

    if show_grid_sec {
    // ── 3) 已启用插件排序与独立尺寸定制（彻底修复图二右侧卡片被切掉的问题）──
    settings_label(ui, "组件库开关与尺寸编排");
    crate::extras::settings_hint(ui, "拖拽顶部标签排序 · 点击下方卡片启用或停用");
    crate::extras::settings_card(ui, |ui| {
        let card_avail_w = crate::extras::layout_avail_w(ui).max(180.0);
        ui.set_width(card_avail_w);
        ui.set_max_width(card_avail_w);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "已启用 {} / {} 项",
                    cfg.win_widgets_order.len(),
                    WIN_WIDGETS.len()
                ))
                .size(11.5)
                .strong(),
            );
            if ui.small_button("恢复默认").clicked() {
                cfg.win_widgets_order = vec![12, 15, 17, 19, 16, 22, 29, 32];
                cfg.plugin_sizes = vec![0; 34];
            }
            if ui.small_button("全部设为 1×1").clicked() {
                cfg.plugin_sizes = vec![0; 34];
            }
            if ui.small_button("全部设为 2×1 宽卡").clicked() {
                cfg.plugin_sizes = vec![1; 34];
            }
        });

        #[derive(Clone, Copy)]
        struct WidgetDrag(usize);
        let mut swap_pair: Option<(usize, usize)> = None;
        let mut remove_idx: Option<usize> = None;
        let mut cycle_size_wid: Option<usize> = None;

        // 固定最小高度的已启用组件拖拽排序区，防止下方 16 宫格卡片因标签折行而发生上下位移跳动（修复图三）
        ui.allocate_ui_with_layout(
            egui::vec2(card_avail_w, 64.0),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_width(card_avail_w);
                ui.set_max_width(card_avail_w);
                ui.set_min_height(64.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(6.0, 6.0);
                    for (idx, &wid) in cfg.win_widgets_order.iter().enumerate() {
                        let (k, v, _, wcol, _) = eval_win_widget(wid, &s, cfg, ppp);
                        let sz = cfg.plugin_sizes.get(wid as usize).copied().unwrap_or(0);
                        let sz_tag = match sz { 1 => "2×1", 2 => "全宽", _ => "1×1" };
                        let chip_text = format!(":: {k} {v}");
                        let tw = ui.fonts(|f| {
                            f.layout_no_wrap(chip_text.clone(), egui::FontId::proportional(11.0), Color32::WHITE)
                                .size()
                                .x
                        });
                        let max_chip_w = (card_avail_w - 8.0).max(110.0);
                        let chip_w = (tw + 66.0).clamp(104.0, max_chip_w);
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(chip_w, 28.0), Sense::click_and_drag());
                        resp.clone().on_hover_cursor(egui::CursorIcon::Grab).dnd_set_drag_payload(WidgetDrag(idx));
                        if let Some(dragged) = resp.dnd_hover_payload::<WidgetDrag>() {
                            if dragged.0 != idx {
                                ui.painter().rect_stroke(rect, 8.0, Stroke::new(1.8, acc), egui::StrokeKind::Inside);
                            }
                        }
                        if let Some(dragged) = resp.dnd_release_payload::<WidgetDrag>() {
                            if dragged.0 != idx {
                                swap_pair = Some((dragged.0, idx));
                            }
                        }
                        ui.painter().rect_filled(rect, 8.0, Color32::from_rgb(43, 43, 43));
                        ui.painter().rect_stroke(
                            rect,
                            8.0,
                            Stroke::new(1.0, wcol.gamma_multiply(0.55)),
                            egui::StrokeKind::Inside,
                        );
                        ui.painter().text(
                            Pos2::new(rect.left() + 8.0, rect.center().y),
                            egui::Align2::LEFT_CENTER,
                            chip_text,
                            egui::FontId::proportional(11.0),
                            Color32::WHITE,
                        );
                        let sz_rect = Rect::from_center_size(Pos2::new(rect.right() - 36.0, rect.center().y), Vec2::new(28.0, 18.0));
                        let sz_hit = ui.interact(sz_rect, egui::Id::new(("win-w-sz", wid, idx)), Sense::click());
                        ui.painter().rect_filled(
                            sz_rect,
                            4.5,
                            if sz > 0 { acc.gamma_multiply(0.85) } else { Color32::from_rgb(50, 54, 66) },
                        );
                        ui.painter().text(
                            sz_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            sz_tag,
                            egui::FontId::proportional(9.2),
                            Color32::WHITE,
                        );
                        if sz_hit.on_hover_text("点击切换该模块在岛内的尺寸：1×1 小卡 → 2×1 宽卡 → 全宽横幅").clicked() {
                            cycle_size_wid = Some(wid as usize);
                        }

                        let close_rect = Rect::from_center_size(Pos2::new(rect.right() - 11.0, rect.center().y), Vec2::splat(16.0));
                        let close_hit = ui.interact(close_rect, egui::Id::new(("win-w-rm", wid, idx)), Sense::click());
                        ui.painter().text(
                            close_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "×",
                            egui::FontId::proportional(12.0),
                            if close_hit.hovered() { Color32::from_rgb(255, 90, 90) } else { Color32::from_gray(155) },
                        );
                        if close_hit.clicked() {
                            remove_idx = Some(idx);
                        }
                    }
                });
            },
        );

        if let Some(w_idx) = cycle_size_wid {
            if let Some(slot) = cfg.plugin_sizes.get_mut(w_idx) {
                *slot = (*slot + 1) % 3;
            }
        }
        if let Some((from, to)) = swap_pair {
            if from < cfg.win_widgets_order.len() && to < cfg.win_widgets_order.len() {
                let item = cfg.win_widgets_order.remove(from);
                cfg.win_widgets_order.insert(to, item);
            }
        }
        if let Some(rm) = remove_idx {
            if rm < cfg.win_widgets_order.len() && cfg.win_widgets_order.len() > 1 {
                cfg.win_widgets_order.remove(rm);
            }
        }

        ui.add_space(4.0);

        // 分类筛选标签
        let cat_id = egui::Id::new("win_widgets_cat_filter_v2");
        let mut sel_cat = ui.data(|d| d.get_temp::<usize>(cat_id)).unwrap_or(0).min(WidgetCat::ALL.len());
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (ci, cname) in std::iter::once("全部 (16)")
                .chain(WidgetCat::ALL.iter().map(|c| c.label()))
                .enumerate()
            {
                let selected = sel_cat == ci;
                let btn = egui::Button::new(
                    RichText::new(cname)
                        .size(11.0)
                        .color(if selected { Color32::WHITE } else { Color32::from_gray(200) }),
                )
                .fill(if selected { acc } else { Color32::from_rgb(51, 51, 51) })
                .corner_radius(6.0);
                if ui.add(btn).clicked() {
                    sel_cat = ci;
                    ui.data_mut(|d| d.insert_temp(cat_id, sel_cat));
                }
            }
        });

        ui.add_space(4.0);

        // 使用卡片真实可用宽度计算网格列宽，彻底杜绝窄窗下右侧卡片被裁切
        let safe_avail = (card_avail_w - 4.0).max(150.0);
        let cols = if safe_avail >= 560.0 {
            3
        } else if safe_avail >= 300.0 {
            2
        } else {
            1
        };
        let gap = 6.0_f32;
        let card_w = ((safe_avail - gap * (cols as f32 - 1.0)) / cols as f32).floor().max(100.0);

        let filtered: Vec<&WinWidgetDef> = WIN_WIDGETS
            .iter()
            .filter(|w| sel_cat == 0 || w.cat == WidgetCat::ALL[sel_cat - 1])
            .collect();

        for chunk in filtered.chunks(cols) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for w in chunk {
                    let enabled = cfg.win_widgets_order.contains(&w.id);
                    let cur_sz = cfg.plugin_sizes.get(w.id as usize).copied().unwrap_or(0);
                    let (_, val, sub, wcol, _) = eval_win_widget(w.id, &s, cfg, ppp);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(card_w, 56.0), Sense::click());

                    let sz_btn_r = Rect::from_min_size(Pos2::new(rect.right() - 80.0, rect.top() + 6.0), Vec2::new(40.0, 17.0));
                    let sz_btn_hit = if enabled && card_w >= 148.0 {
                        Some(ui.interact(sz_btn_r, egui::Id::new(("grid-w-sz", w.id)), Sense::click()))
                    } else {
                        None
                    };
                    if let Some(ref sh) = sz_btn_hit {
                        if sh.clone().on_hover_text("切换岛内卡片尺寸：1×1 小卡 / 2×1 宽卡 / 全宽横幅").clicked() {
                            if let Some(slot) = cfg.plugin_sizes.get_mut(w.id as usize) {
                                *slot = (*slot + 1) % 3;
                            }
                        }
                    }
                    if resp.clicked() && !sz_btn_hit.as_ref().is_some_and(|h| h.clicked()) {
                        if enabled {
                            if cfg.win_widgets_order.len() > 1 {
                                cfg.win_widgets_order.retain(|&x| x != w.id);
                            }
                        } else {
                            cfg.win_widgets_order.push(w.id);
                        }
                    }
                    let bg = if enabled {
                        Color32::from_rgb(51, 51, 51)
                    } else if resp.hovered() {
                        Color32::from_rgb(51, 51, 51)
                    } else {
                        Color32::from_rgb(43, 43, 43)
                    };
                    ui.painter().rect_filled(rect, 8.0, bg);
                    ui.painter().rect_stroke(
                        rect,
                        8.0,
                        Stroke::new(
                            if enabled { 1.3 } else { 0.8 },
                            if enabled { wcol } else { Color32::from_white_alpha(18) },
                        ),
                        egui::StrokeKind::Inside,
                    );
                    let clipped = ui.painter().with_clip_rect(rect.shrink(4.0));
                    clipped.text(
                        Pos2::new(rect.left() + 10.0, rect.top() + 14.0),
                        egui::Align2::LEFT_CENTER,
                        w.name,
                        egui::FontId::proportional(11.2),
                        Color32::WHITE,
                    );
                    if enabled && card_w >= 148.0 {
                        clipped.rect_filled(
                            sz_btn_r,
                            4.0,
                            if cur_sz > 0 { acc.gamma_multiply(0.85) } else { Color32::from_rgb(52, 56, 68) },
                        );
                        clipped.text(
                            sz_btn_r.center(),
                            egui::Align2::CENTER_CENTER,
                            widget_size_label(cur_sz),
                            egui::FontId::proportional(8.5),
                            Color32::WHITE,
                        );
                    }
                    clipped.text(
                        Pos2::new(rect.right() - 8.0, rect.top() + 14.0),
                        egui::Align2::RIGHT_CENTER,
                        if enabled { "已启用" } else { "停用" },
                        egui::FontId::proportional(9.5),
                        if enabled { wcol } else { Color32::from_gray(130) },
                    );
                    let sub_line = format!("{val} · {sub}");
                    clipped.text(
                        Pos2::new(rect.left() + 10.0, rect.bottom() - 15.0),
                        egui::Align2::LEFT_CENTER,
                        sub_line,
                        egui::FontId::proportional(9.8),
                        Color32::from_gray(180),
                    );
                }
            });
        }

    });
    } // end show_grid_sec

    if show_cards_sec {
        // ── 4) 单卡个性化与自定义内容编辑（独立标准卡片容器 + 三行响应式排版，彻底解决图三红框文字拆行与右侧跨度裁切）──
        settings_label(ui, "单卡个性化与自定义内容编辑");
        crate::extras::settings_card(ui, |ui| {
            let avail_row_w = crate::extras::layout_avail_w(ui).max(180.0);
            ui.set_width(avail_row_w);
            ui.set_max_width(avail_row_w);
            crate::extras::settings_hint(ui, "留空自动显示实时硬件数据；输入文字可自定义卡片上的标题、主内容与副文本。");
            ui.add_space(4.0);

            while cfg.plugin_card_styles.len() < 34 {
                cfg.plugin_card_styles.push(0);
            }
            while cfg.plugin_custom_titles.len() < 34 {
                cfg.plugin_custom_titles.push(String::new());
            }
            while cfg.plugin_custom_values.len() < 34 {
                cfg.plugin_custom_values.push(String::new());
            }
            while cfg.plugin_custom_subs.len() < 34 {
                cfg.plugin_custom_subs.push(String::new());
            }
            while cfg.plugin_sizes.len() < 34 {
                cfg.plugin_sizes.push(0);
            }

            let enabled_list = cfg.win_widgets_order.clone();
            let row_bg = if ui.visuals().dark_mode {
                Color32::from_rgb(43, 43, 43)
            } else {
                Color32::from_rgb(242, 244, 248)
            };
            let border_col = if ui.visuals().dark_mode {
                Color32::from_white_alpha(18)
            } else {
                Color32::from_black_alpha(14)
            };

            for (pos_i, &wid) in enabled_list.iter().enumerate() {
                let (def_name, val, def_sub, wcol, _) = eval_win_widget(wid, &s, cfg, ppp);
                let idx_u = wid as usize;
                if idx_u >= 34 {
                    continue;
                }
                if pos_i > 0 {
                    ui.add_space(4.0);
                }
                ui.allocate_ui_with_layout(egui::vec2(avail_row_w, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_min_width(avail_row_w);
                    ui.set_max_width(avail_row_w);
                    let mut frame_clip = ui.clip_rect();
                    frame_clip.max.x = frame_clip.max.x.min(ui.cursor().left() + avail_row_w);
                    ui.set_clip_rect(frame_clip);
                    egui::Frame::new()
                        .fill(row_bg)
                        .stroke(Stroke::new(1.0, border_col))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::symmetric(10, 8))
                        .show(ui, |ui| {
                            // 10*2 inner_margin + 1*2 stroke = 22px
                            let inner_w = (avail_row_w - 22.0).max(140.0);
                            ui.set_width(inner_w);
                            ui.set_max_width(inner_w);
                            let mut inner_clip = ui.clip_rect();
                            inner_clip.max.x = inner_clip.max.x.min(ui.cursor().left() + inner_w);
                            ui.set_clip_rect(inner_clip);

                            // 第一行：左侧「● 硬件名 (实时数值)」| 右侧「跨度 [1×1 | 2×1 | 全宽]」（窄窗下自动折行，绝不裁切右侧按钮）
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                                let (dot_r, _) = ui.allocate_exact_size(egui::vec2(8.0, 20.0), Sense::hover());
                                ui.painter().circle_filled(dot_r.center(), 3.5, wcol);
                                ui.label(RichText::new(format!("{def_name} ({val})")).size(11.6).strong());
                                ui.add_space(4.0);
                                ui.label(RichText::new("跨度").size(10.2).color(dim_text(ui)));
                                for (sz_val, sz_lbl) in [(0u8, "1×1"), (1u8, "2×1"), (2u8, "全宽")] {
                                    let sel = cfg.plugin_sizes[idx_u] == sz_val;
                                    let btn = egui::Button::new(
                                        RichText::new(sz_lbl)
                                            .size(10.2)
                                            .color(if sel { Color32::WHITE } else { dim_text(ui) }),
                                    )
                                    .fill(if sel {
                                        acc
                                    } else if ui.visuals().dark_mode {
                                        Color32::from_rgb(51, 51, 51)
                                    } else {
                                        Color32::from_rgb(226, 230, 238)
                                    })
                                    .corner_radius(5.0);
                                    if ui.add_sized([34.0, 21.0], btn).clicked() {
                                        cfg.plugin_sizes[idx_u] = sz_val;
                                    }
                                }
                            });

                            ui.add_space(4.0);

                            // 第二行：标题 / 主内容 / 副文本 按实际字体测宽扣减，杜绝输入框撑出卡片右缘
                            let lbl_font = egui::FontId::proportional(10.5);
                            let measure_lbl = |t: &str| -> f32 {
                                ui.fonts(|f| f.layout_no_wrap(t.to_owned(), lbl_font.clone(), Color32::WHITE).size().x).ceil()
                            };
                            let lw_t = measure_lbl("标题");
                            let lw_v = measure_lbl("主内容");
                            let lw_s = measure_lbl("副文本");
                            let lw_sty = measure_lbl("样式");
                            let gap_x = 5.0_f32;
                            if inner_w >= 400.0 {
                                let rem = (inner_w - lw_t - lw_v - lw_s - gap_x * 5.0 - 4.0).max(120.0);
                                let w_title = (rem * 0.25).clamp(38.0, 110.0);
                                let w_val = (rem * 0.27).clamp(40.0, 115.0);
                                let w_sub = (rem - w_title - w_val).max(42.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap_x;
                                    ui.label(RichText::new("标题").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_title, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_titles[idx_u])
                                            .hint_text(def_name)
                                            .vertical_align(egui::Align::Center),
                                    );
                                    ui.label(RichText::new("主内容").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_val, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_values[idx_u])
                                            .hint_text(&val)
                                            .vertical_align(egui::Align::Center),
                                    );
                                    ui.label(RichText::new("副文本").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_sub, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_subs[idx_u])
                                            .hint_text(&def_sub)
                                            .vertical_align(egui::Align::Center),
                                    );
                                });
                            } else {
                                let rem_top = (inner_w - lw_t - lw_v - gap_x * 3.0 - 4.0).max(80.0);
                                let w_title = (rem_top * 0.46).max(36.0);
                                let w_val = (rem_top - w_title).max(36.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap_x;
                                    ui.label(RichText::new("标题").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_title, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_titles[idx_u])
                                            .hint_text(def_name)
                                            .vertical_align(egui::Align::Center),
                                    );
                                    ui.label(RichText::new("主内容").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_val, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_values[idx_u])
                                            .hint_text(&val)
                                            .vertical_align(egui::Align::Center),
                                    );
                                });
                                ui.add_space(3.0);
                                let w_sub = (inner_w - lw_s - gap_x - 4.0).max(72.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = gap_x;
                                    ui.label(RichText::new("副文本").size(10.5).color(dim_text(ui)));
                                    ui.add_sized(
                                        [w_sub, 22.0],
                                        egui::TextEdit::singleline(&mut cfg.plugin_custom_subs[idx_u])
                                            .hint_text(&def_sub)
                                            .vertical_align(egui::Align::Center),
                                    );
                                });
                            }

                            ui.add_space(4.0);

                            // 第三行：卡片样式分段选择器（自适应铺满剩余宽度，不再与跨度挤在同一行导致被裁切）
                            let seg_avail = (inner_w - lw_sty - 10.0).max(120.0);
                            let item_w = ((seg_avail - 18.0) / 4.0).clamp(40.0, 82.0);
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 6.0;
                                ui.label(RichText::new("样式").size(10.5).color(dim_text(ui)));
                                crate::extras::settings_segmented(
                                    ui,
                                    &mut cfg.plugin_card_styles[idx_u],
                                    &[
                                        (0u8, "智能专属"),
                                        (1u8, "圆环仪表"),
                                        (2u8, "极简大字"),
                                        (3u8, "趋势波形"),
                                    ],
                                    item_w,
                                );
                            });
                        });
                });
            }
        });
    } // end show_cards_sec
}

#[allow(dead_code)]
fn plugin_accent(index: usize) -> Color32 {
    [
        Color32::from_rgb(86, 156, 214),
        Color32::from_rgb(106, 190, 144),
        Color32::from_rgb(214, 158, 86),
    ][index % 3]
}

#[allow(dead_code)]
fn card(
    ui: &mut egui::Ui,
    id: &str,
    accent: Color32,
    add: impl FnOnce(&mut egui::Ui),
) -> bool {
    let id = egui::Id::new(id);
    let hover_key = id.with("hovered");
    let was_hovered = ui.data(|d| d.get_temp::<bool>(hover_key).unwrap_or(false));
    let ctx = ui.ctx().clone();
    let t = ctx.animate_bool_with_time(id.with("hover"), was_hovered, 0.12);
    let pressed = was_hovered && ui.input(|i| i.pointer.primary_down());
    let width = ui.available_width().max(120.0);
    let bg = mix(
        card_fill(ui),
        card_hover(ui),
        if pressed { (t * 0.7 + 0.3).min(1.0) } else { t },
    );

    let stroke = if t > 0.01 {
        Stroke::new(1.0_f32, accent.gamma_multiply(0.30 * t))
    } else {
        Stroke::NONE
    };
    let inner = egui::Frame::new()
        .fill(bg)
        .stroke(stroke)
        .corner_radius(10.0)
        .inner_margin(egui::Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(width - 24.0);
            add(ui);
        });
    let rect = inner.response.rect;
    let hovered = ui.rect_contains_pointer(rect);
    if hovered != was_hovered {
        ui.data_mut(|d| d.insert_temp(hover_key, hovered));
        ctx.request_repaint();
    }

    ui.painter().rect_filled(
        Rect::from_min_size(
            rect.min + Vec2::new(0.0, 10.0),
            Vec2::new(3.0, rect.height() - 20.0),
        ),
        2.0,
        accent.gamma_multiply(0.35 + 0.65 * t),
    );

    let clicked = hovered && ui.input(|i| i.pointer.primary_clicked());
    if t > 0.01 || pressed {
        ctx.request_repaint();
    }
    clicked
}

fn mix(from: Color32, to: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let l = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color32::from_rgb(
        l(from.r(), to.r()),
        l(from.g(), to.g()),
        l(from.b(), to.b()),
    )
}

pub fn monitor_preview(ui: &mut egui::Ui, on: bool) {
    if !on {
        ui.label(RichText::new("组件已停用，不采样系统数据").size(10.5).color(dim_text(ui)));
        return;
    }
    let s = sysmon::sample();
    let mem = if s.mem_total > 0 {
        s.mem_used as f32 / s.mem_total as f32 * 100.0
    } else {
        0.0
    };
    ui.horizontal(|ui| {
        for (label, value) in [
            ("CPU", format!("{:.0}%", s.cpu)),
            ("内存", format!("{mem:.0}%")),
            ("网络", format!("↓{} ↑{}", sysmon::rate(s.net_rx), sysmon::rate(s.net_tx))),
        ] {
            ui.label(RichText::new(label).size(10.0).color(dim_text(ui)));
            ui.label(RichText::new(value).size(13.0).strong());
            ui.add_space(10.0);
        }
    });
    ui.ctx()
        .request_repaint_after(std::time::Duration::from_millis(1000));
}

pub fn monitor_options(ui: &mut egui::Ui, cfg: &mut config::Config) {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut cfg.monitor_cpu, "CPU");
        ui.checkbox(&mut cfg.monitor_gpu, "GPU");
        ui.checkbox(&mut cfg.monitor_mem, "内存");
        ui.checkbox(&mut cfg.monitor_disk, "磁盘");
        ui.checkbox(&mut cfg.monitor_net, "网络");
        ui.checkbox(&mut cfg.monitor_battery, "电池");
    });
}

// ---------------------------------------------------------------
// 1. 岛内「定制组件」页 (`page == 9`)
// ---------------------------------------------------------------

static PLUGIN_PAGE_IDX: AtomicUsize = AtomicUsize::new(0);

pub fn draw_island_page(c: &crate::paint::Card) {
    let cfg = config::get();
    let s = sysmon::sample();
    if !cfg!(test) && cfg.plugin_ui_mode == 0 && !cfg.win_compact_bar {
        draw_shadcn_plugin_island_page(c, &cfg, &s);
    } else {
        draw_shadcn_plugin_island_page(c, &cfg, &s);
    }
}

/// [Shadcn UI 新版插件岛 (`plugin_ui_mode == 0`, 需求 14)]：
/// 遵循 https://ui.shadcn.com/ Dashboard & Bento Telemetry Card 规范：
/// - 零顶栏文字冲突：顶栏不叠加任何进程/开机时长散字，翻页胶囊严格置于 `x = 304..342` 安全区；
/// - 卡片内采用三层式正交排版：顶栏 (Zinc-400 标题 + 右上独立胶囊 Badge) + 中部 (大字号主指标 + 右侧 6 柱实时微波形/圆环) + 底栏 (Shadcn 连续发光进度轨)；
/// - 底部收束于统一的 Shadcn 核心负载与系统运行信息胶囊舱 (`y = 204..222`)，彻底杜绝任何元素重叠。
/// [修复需求 1]：Glance 新版插件岛完全响应「设置 → 插件」中的组件顺序 (`win_widgets_order`)、自定义标题 (`plugin_custom_titles`)、独立卡片视觉形态 (`plugin_card_styles`)、卡片圆角 (`plugin_card_radius`)、背景不透明度 (`plugin_card_opacity`)、文字缩放 (`plugin_font_scale`) 与各项显隐开关！
pub fn draw_shadcn_plugin_island_page(
    c: &crate::paint::Card,
    cfg: &config::Config,
    s: &sysmon::Sample,
) {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    if !cfg.monitor_plugin {
        c.fill(38.0, 36.0, 343.0, 182.0, 12.0, Color32::from_rgba_unmultiplied(16, 17, 22, 185));
        c.stroke(38.0, 36.0, 343.0, 182.0, 12.0, 1.0, Color32::from_white_alpha(22));
        c.text(209.5, 120.0, "插件面板已停用", 13.0, W::Bold, theme::WHITE, H::Center);
        c.text(209.5, 140.0, "前往「设置 → 插件」开启硬件监控面板", 9.5, W::Regular, theme::GRAY, H::Center);
        return;
    }

    let left_x = 38.0_f32;
    let total_w = 343.0_f32;
    let form = cfg.glance_widget_form % 3;
    let ppp = c.ui.ctx().pixels_per_point();
    let active_wids = sanitized_widget_order(&cfg.win_widgets_order);
    let per_page = 4usize;
    let total_pages = ((active_wids.len().max(1) + per_page - 1) / per_page).max(1);
    let cur_pg = PLUGIN_PAGE_IDX.load(Ordering::Relaxed).min(total_pages - 1);
    let page_slice: Vec<u8> = active_wids
        .iter()
        .copied()
        .skip(cur_pg * per_page)
        .take(per_page)
        .collect();

    // 次顶栏左端：已启用插件数与实时分页切换器（与右侧形态切换胶囊同高对齐）
    c.fill(left_x, 9.0, 186.0, 18.0, 9.0, Color32::from_rgba_unmultiplied(24, 26, 34, 195));
    c.stroke(left_x, 9.0, 186.0, 18.0, 9.0, 0.7, Color32::from_white_alpha(20));
    c.circle(left_x + 9.5, 18.0, 2.5, Color32::from_rgb(52, 211, 153));
    c.text(
        left_x + 16.5,
        21.2,
        &format!("已启用 {} 项插件 · 实时同步", active_wids.len()),
        7.9,
        W::Semibold,
        Color32::from_rgb(212, 212, 216),
        H::Left,
    );
    if total_pages > 1 {
        let pg_x = left_x + 132.0;
        let prev_hit = c.hit(pg_x, 9.0, 16.0, 18.0, "gl-plugin-pg-prev");
        let next_hit = c.hit(pg_x + 36.0, 9.0, 16.0, 18.0, "gl-plugin-pg-next");
        c.text(pg_x + 7.5, 21.2, "◀", 7.4, W::Bold, if prev_hit.hovered() { theme::WHITE } else { theme::GRAY }, H::Center);
        c.text(pg_x + 25.5, 21.2, &format!("{}/{}", cur_pg + 1, total_pages), 7.6, W::Bold, Color32::from_rgb(56, 189, 248), H::Center);
        c.text(pg_x + 43.5, 21.2, "▶", 7.4, W::Bold, if next_hit.hovered() { theme::WHITE } else { theme::GRAY }, H::Center);
        if prev_hit.clicked() {
            let np = if cur_pg == 0 { total_pages - 1 } else { cur_pg - 1 };
            PLUGIN_PAGE_IDX.store(np, Ordering::Relaxed);
        }
        if next_hit.clicked() {
            let np = (cur_pg + 1) % total_pages;
            PLUGIN_PAGE_IDX.store(np, Ordering::Relaxed);
        }
    }

    // 次顶栏右端 (x = 301..379)：Glance 形态切换胶囊 ([曲线 | 圆环 | 分栏]) + 平滑滑块动画
    let forms = [(0u8, "曲线"), (1u8, "圆环"), (2u8, "分栏")];
    c.fill(301.0, 9.0, 78.0, 18.0, 9.0, Color32::from_rgba_unmultiplied(24, 26, 34, 210));
    c.stroke(301.0, 9.0, 78.0, 18.0, 9.0, 0.7, Color32::from_white_alpha(24));
    let slide_fx = c.ui.ctx().animate_value_with_time(
        egui::Id::new("glance_plugin_form_pill"),
        302.5 + (form as f32) * 25.2,
        0.18,
    );
    c.fill(slide_fx, 10.5, 24.5, 15.0, 7.5, Color32::from_rgb(56, 189, 248).gamma_multiply(0.32));
    c.stroke(slide_fx, 10.5, 24.5, 15.0, 7.5, 0.7, Color32::from_rgb(56, 189, 248));
    for (idx, (f_id, f_lbl)) in forms.iter().enumerate() {
        let fx = 302.5 + idx as f32 * 25.2;
        let sel = form == *f_id;
        let hit = c.hit(fx, 9.0, 25.0, 18.0, ("glance-w-form", *f_id));
        c.text(
            fx + 12.2,
            21.2,
            f_lbl,
            7.6,
            if sel { W::Bold } else { W::Regular },
            if sel { theme::WHITE } else { Color32::from_rgb(161, 161, 170) },
            H::Center,
        );
        if hit.clicked() && !sel {
            let nf = *f_id;
            config::update(|c_mut| c_mut.glance_widget_form = nf);
        }
    }

    let samples = sysmon::history();
    let cpu_col = Color32::from_rgb(56, 189, 248);
    let gpu_col = Color32::from_rgb(168, 85, 247);
    let muted = Color32::from_rgb(161, 161, 170);
    let gpu_pct = s.gpu.unwrap_or(18.0).clamp(0.0, 100.0);

    let f_scale = (cfg.plugin_font_scale.clamp(75, 130) as f32) / 100.0;
    let card_rad = (cfg.plugin_card_radius.clamp(4, 22) as f32).max(6.0);
    let card_alpha = ((cfg.plugin_card_opacity.clamp(20, 100) as f32 / 100.0) * 225.0).round() as u8;
    let card_bg = Color32::from_rgba_unmultiplied(17, 19, 26, card_alpha);

    // 解析当前页每个组件在设置中的实时数据、自定义标题、自定义主内容、自定义副文本与自定义卡片形态（需求 11）
    let title_sz = (cfg.plugin_title_size.clamp(7.0, 15.0)) * f_scale;
    let value_sz = (cfg.plugin_value_size.clamp(9.0, 22.0)) * f_scale;
    let sub_sz = (cfg.plugin_sub_size.clamp(7.5, 15.0)) * f_scale;
    let resolved_items: Vec<(u8, String, String, String, Color32, f32, u8)> = page_slice
        .iter()
        .map(|&wid| {
            let (def_title, val_str, sub_str, col, bar_opt) = eval_win_widget(wid, s, cfg, ppp);
            let custom_title = cfg
                .plugin_custom_titles
                .get(wid as usize)
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .unwrap_or(def_title)
                .to_owned();
            let custom_val = cfg
                .plugin_custom_values
                .get(wid as usize)
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .unwrap_or(val_str);
            let custom_sub = cfg
                .plugin_custom_subs
                .get(wid as usize)
                .map(|t| t.trim())
                .filter(|t| !t.is_empty())
                .map(str::to_owned)
                .unwrap_or(sub_str);
            let cstyle = cfg
                .plugin_card_styles
                .get(wid as usize)
                .copied()
                .unwrap_or(0)
                .min(3);
            let ratio = bar_opt.unwrap_or_else(|| ((wid as f32 * 0.19 + 0.35).fract()).clamp(0.18, 0.86)).clamp(0.04, 1.0);
            (wid, custom_title, custom_val, custom_sub, col, ratio, cstyle)
        })
        .collect();

    if form == 0 {
        // ── 形态 0：Glance「上面是曲线/仪表、下面是读数的磨砂玻璃卡片」2×2 矩阵（完全同步设置中的排序、自定义标题、卡片形态与样式参数）──
        let card_w = (total_w - 7.0) * 0.5;
        let card_h = if cfg.monitor_cores { 74.0_f32 } else { 88.0_f32 };
        let y0 = 36.0_f32;
        let y1 = y0 + card_h + 6.5;

        for (idx, (wid, title, main_val, sub_str, col, ratio, cstyle)) in resolved_items.iter().enumerate() {
            let cx = if idx % 2 == 0 { left_x } else { left_x + card_w + 7.0 };
            let cy = if idx < 2 { y0 } else { y1 };

            c.fill(cx, cy, card_w, card_h, card_rad, card_bg);
            c.stroke(cx, cy, card_w, card_h, card_rad, 0.9, Color32::from_white_alpha(26));
            c.seg((cx + 10.0, cy + 0.8), (cx + card_w - 10.0, cy + 0.8), 0.8, col.gamma_multiply(0.55));

            if cfg.plugin_show_bar {
                if *cstyle == 1 {
                    // 独立指定「圆环仪表」形态
                    let r_cx = cx + card_w - 28.0;
                    let r_cy = cy + 22.0;
                    c.ring(r_cx, r_cy, 12.5, 3.0, Color32::from_white_alpha(22));
                    c.arc(r_cx, r_cy, 12.5, -90.0, -90.0 + 360.0 * (*ratio), 3.0, *col);
                } else if *cstyle == 2 {
                    // 独立指定「极简大字 + 进度轨」形态
                    let gx0 = cx + 8.0;
                    let gw = card_w - 16.0;
                    c.fill(gx0, cy + 16.0, gw, 7.0, 3.5, Color32::from_white_alpha(18));
                    c.fill(gx0, cy + 16.0, (gw * (*ratio)).max(4.0), 7.0, 3.5, *col);
                } else {
                    // 默认/曲线波形形态：顶部 60s 纯净实时面积曲线舞台（文字位于曲线正下方，零遮挡）
                    let gx0 = cx + 8.0;
                    let gw = card_w - 16.0;
                    let gy_top = cy + 6.5;
                    let gh = 27.5_f32;
                    let gy_bot = gy_top + gh;
                    c.fill(gx0, gy_top, gw, gh, 6.0, Color32::from_rgba_unmultiplied(8, 10, 15, 145));

                    let n_pts = 20usize;
                    let mut pts = Vec::with_capacity(n_pts);
                    let hist_len = samples.len();
                    for i in 0..n_pts {
                        let px = gx0 + 2.0 + (i as f32 / (n_pts - 1) as f32) * (gw - 4.0);
                        let pt_ratio = if hist_len > 0 {
                            let h_idx = hist_len.saturating_sub(n_pts - i);
                            let sm = &samples[h_idx.min(hist_len - 1)];
                            match *wid {
                                12 | 13 | 14 => (sm.cpu / 100.0).clamp(0.06, 0.96),
                                16 | 17 | 18 => (sm.gpu.unwrap_or(gpu_pct) / 100.0).clamp(0.06, 0.96),
                                15 => if sm.mem_total > 0 { (sm.mem_used as f32 / sm.mem_total as f32).clamp(0.08, 0.95) } else { 0.45 },
                                19 | 20 => (((sm.net_rx + sm.net_tx) as f32 / (2.0 * 1024.0 * 1024.0)).clamp(0.08, 0.95)),
                                _ => (*ratio * 0.82 + 0.08 * ((i as f32 * 0.5 + *wid as f32).sin())).clamp(0.08, 0.95),
                            }
                        } else {
                            (*ratio * 0.85 + 0.10 * ((i as f32 * 0.55 + *wid as f32).sin())).clamp(0.08, 0.95)
                        };
                        let py = gy_bot - 2.0 - pt_ratio * (gh - 5.0);
                        pts.push((px, py));
                    }
                    let mut mesh = egui::Mesh::default();
                    for (i, &(px, py)) in pts.iter().enumerate() {
                        mesh.colored_vertex(c.pos(px, py), Color32::from_rgba_unmultiplied(col.r(), col.g(), col.b(), 68));
                        mesh.colored_vertex(c.pos(px, gy_bot), Color32::TRANSPARENT);
                        if i > 0 {
                            let b = (i as u32 - 1) * 2;
                            mesh.add_triangle(b, b + 1, b + 2);
                            mesh.add_triangle(b + 1, b + 3, b + 2);
                        }
                    }
                    c.painter.add(egui::Shape::mesh(mesh));
                    c.path(&pts, 1.5, *col);
                    if let Some(&(lx, ly)) = pts.last() {
                        c.circle(lx, ly, 2.2, theme::WHITE);
                        c.circle(lx, ly, 1.3, *col);
                    }
                }
            }

            if cfg.plugin_show_title {
                c.circle(cx + 11.5, cy + 42.8, 2.0, *col);
                c.text(
                    cx + 16.5,
                    cy + 46.0,
                    &c.fit(title, title_sz, W::Bold, card_w - 26.0),
                    title_sz,
                    W::Bold,
                    theme::WHITE,
                    H::Left,
                );
            }
            if cfg.plugin_show_value {
                c.text(
                    cx + 9.0,
                    cy + card_h - 7.5,
                    &c.fit(main_val, value_sz, W::Bold, card_w * 0.44),
                    value_sz,
                    W::Bold,
                    theme::WHITE,
                    H::Left,
                );
            }
            if cfg.plugin_show_sub {
                // [修复需求 11 图5 `image_3a3307.jpg` 红框处文字过小与截断]：
                // 增大副文本字号至 `sub_sz`（默认 9.4pt，支持自由调节），加宽右侧显示宽度至 58% 卡片宽并使用高清晰度浅灰白字色！
                c.text(
                    cx + card_w - 7.5,
                    cy + card_h - 8.0,
                    &c.fit(sub_str, sub_sz, W::Semibold, card_w * 0.58),
                    sub_sz,
                    W::Semibold,
                    Color32::from_rgb(212, 214, 222),
                    H::Right,
                );
            }
        }
    } else if form == 1 {
        // ── 形态 1：Glance「磨砂玻璃圆环矩阵 (Rings)」：动态展示当前页 4 项插件 ──
        let ring_w = (total_w - 6.0 * 3.0) / 4.0;
        let ring_h = if cfg.monitor_cores { 154.0_f32 } else { 178.0_f32 };
        let ry = 36.0_f32;
        for (idx, (_wid, lbl, main_v, sub1, col, ratio, _cstyle)) in resolved_items.iter().enumerate() {
            let rx = left_x + idx as f32 * (ring_w + 6.0);
            c.fill(rx, ry, ring_w, ring_h, card_rad, card_bg);
            c.stroke(rx, ry, ring_w, ring_h, card_rad, 0.9, Color32::from_white_alpha(24));
            let cx_r = rx + ring_w * 0.5;
            let cy_r = ry + 56.0;
            let rad = 28.0_f32;
            if cfg.plugin_show_bar {
                c.ring(cx_r, cy_r, rad, 4.2, Color32::from_white_alpha(20));
                let sweep = 360.0 * ratio.clamp(0.04, 1.0);
                c.arc(cx_r, cy_r, rad, -90.0, -90.0 + sweep, 4.2, *col);
            }
            if cfg.plugin_show_value {
                c.text(
                    cx_r,
                    cy_r + 4.5,
                    &c.fit(main_v, 10.8 * f_scale, W::Bold, ring_w - 12.0),
                    10.8 * f_scale,
                    W::Bold,
                    theme::WHITE,
                    H::Center,
                );
            }
            if cfg.plugin_show_title {
                c.text(
                    cx_r,
                    ry + 16.0,
                    &c.fit(lbl, 8.6 * f_scale, W::Bold, ring_w - 8.0),
                    8.6 * f_scale,
                    W::Bold,
                    *col,
                    H::Center,
                );
            }
            if cfg.plugin_show_sub {
                c.text(
                    cx_r,
                    ry + 116.0,
                    &c.fit(sub1, sub_sz * 0.92, W::Semibold, ring_w - 6.0),
                    sub_sz * 0.92,
                    W::Semibold,
                    Color32::from_rgb(228, 228, 231),
                    H::Center,
                );
            }
        }
    } else {
        // ── 形态 2：Glance「高密硬件监控分栏横条 (Lanes)」：动态展示当前页 4 项插件 ──
        let lane_h = if cfg.monitor_cores { 34.5_f32 } else { 40.0_f32 };
        let gap = 5.0_f32;
        for (i, (_wid, title, val, detail, col, ratio, _cstyle)) in resolved_items.iter().enumerate() {
            let ly = 36.0 + i as f32 * (lane_h + gap);
            c.fill(left_x, ly, total_w, lane_h, card_rad.min(11.0), card_bg);
            c.stroke(left_x, ly, total_w, lane_h, card_rad.min(11.0), 0.8, Color32::from_white_alpha(22));
            c.fill(left_x + 3.0, ly + 7.0, 2.6, lane_h - 14.0, 1.3, *col);
            if cfg.plugin_show_title {
                c.text(left_x + 11.0, ly + 14.5, &c.fit(title, 8.8 * f_scale, W::Bold, 175.0), 8.8 * f_scale, W::Bold, theme::WHITE, H::Left);
            }
            if cfg.plugin_show_sub {
                c.text(left_x + 11.0, ly + 26.8, &c.fit(detail, sub_sz * 0.92, W::Semibold, 182.0), sub_sz * 0.92, W::Semibold, Color32::from_rgb(212, 214, 222), H::Left);
            }
            if cfg.plugin_show_value {
                c.text(left_x + total_w - 10.0, ly + 15.0, &c.fit(val, 11.2 * f_scale, W::Bold, 130.0), 11.2 * f_scale, W::Bold, *col, H::Right);
            }
            if cfg.plugin_show_bar {
                let bx = left_x + 190.0;
                let bw = total_w - 200.0;
                let by = ly + 23.5;
                c.fill(bx, by, bw, 3.6, 1.8, Color32::from_white_alpha(20));
                c.fill(bx, by, bw * (*ratio), 3.6, 1.8, *col);
            }
        }
    }

    // 底部 Glance 线程脉冲条与系统运行状态栏 (`y = 197..219`，受 `cfg.monitor_cores` 开关控制)
    if cfg.monitor_cores {
        let fy = 197.0_f32;
        let fh = 21.0_f32;
        c.fill(left_x, fy, total_w, fh, 7.5, Color32::from_rgba_unmultiplied(15, 16, 22, 188));
        c.stroke(left_x, fy, total_w, fh, 7.5, 0.8, Color32::from_white_alpha(20));
        c.text(left_x + 8.0, fy + 14.0, "THREADS", 7.2, W::Bold, cpu_col, H::Left);
        let bars_x = left_x + 54.0;
        let bars_w = 156.0_f32;
        let cores = if s.cores.is_empty() { &[s.cpu; 8][..] } else { &s.cores[..] };
        let n = cores.len().max(1) as f32;
        let gap = 2.0_f32;
        let bw = ((bars_w - gap * (n - 1.0)) / n).max(2.0);
        for (i, &val) in cores.iter().enumerate() {
            let bh = (12.0 * (val / 100.0).clamp(0.12, 1.0)).clamp(2.0, 12.0);
            let bx = bars_x + i as f32 * (bw + gap);
            c.fill(bx, fy + 16.5 - bh, bw, bh, 1.0, crate::paint::mix(cpu_col, gpu_col, i as f32 / n));
        }
        let info_str = format!("进程 {} · 运行 {}", s.processes, sysmon::uptime_text(s.uptime));
        c.text(left_x + total_w - 8.0, fy + 14.0, &c.fit(&info_str, 7.8, W::Semibold, 122.0), 7.8, W::Semibold, Color32::from_rgb(212, 212, 216), H::Right);
    }
}

/// 将 `self.weather.snapshot` (0.6.64 经典天气城市/实况) 实时同步至 Breezy / Shadcn 天气状态，实现三种天气界面方案 100% 功能与数据同质化（需求 5）。
pub fn sync_breezy_from_snapshot(snap: &crate::weather::Snapshot) {
    let cfg = config::get();
    let Ok(mut g) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() else { return };
    let city_clean = snap.place.trim().to_owned();
    let city_changed = !city_clean.is_empty() && g.city.trim_end_matches('市') != city_clean.trim_end_matches('市');
    if !city_clean.is_empty() {
        g.city = city_clean.clone();
    }
    if let Some(pos) = &snap.position {
        g.lat = pos.lat as f64;
        g.lon = pos.lon as f64;
        if is_valid_street_label(&pos.region) {
            g.street = pos.region.trim().to_owned();
        } else if !city_changed && is_valid_street_label(&g.street) {
            // 保留当前已选街道，不因 Open-Meteo 的 "湖南 · 中国" 覆盖掉具体街道
        } else if !city_changed && is_valid_street_label(&cfg.weather_street) {
            g.street = cfg.weather_street.trim().to_owned();
        } else {
            g.street = default_street_for_city(&g.city, g.lat, g.lon);
        }
    } else if !is_valid_street_label(&g.street) || city_changed {
        g.street = default_street_for_city(&g.city, g.lat, g.lon);
    }
    let new_temp = snap.temp.round() as i32;
    let temp_changed = g.temp != new_temp || !g.synced_from_live || city_changed;
    g.temp = new_temp;
    g.feels_like = snap.feels.map(|f| f.round() as i32).unwrap_or(new_temp);
    if let Some(hum) = snap.humidity {
        g.humidity = hum.min(100) as u8;
    }
    if let Some(w) = snap.wind {
        g.wind = format!("实时风速 · {:.0}km/h", w);
    }
    g.desc = snap.description().to_owned();
    if let (Some(sr), Some(ss)) = (&snap.sunrise, &snap.sunset) {
        g.sun_rise_set = format!("{sr} 日出 · {ss} 日落");
    }
    if snap.at > 0 {
        if let Some(dt) = chrono::DateTime::from_timestamp(snap.at, 0) {
            g.updated_at = dt.with_timezone(&Local).format("%H:%M 更新").to_string();
        }
    }
    if !snap.days.is_empty() {
        g.forecast = snap
            .days
            .iter()
            .take(5)
            .enumerate()
            .map(|(idx, d)| DailyForecast {
                day_label: if idx == 0 { "今天".into() } else { d.label.clone() },
                desc: crate::weather::describe(d.code).to_owned(),
                t_min: d.low.round() as i32,
                t_max: d.high.round() as i32,
            })
            .collect();
    }
    let t_min = snap
        .low
        .map(|v| v.round() as i32)
        .or_else(|| g.forecast.first().map(|d| d.t_min))
        .unwrap_or(new_temp - 4);
    let t_max = snap
        .high
        .map(|v| v.round() as i32)
        .or_else(|| g.forecast.first().map(|d| d.t_max))
        .unwrap_or(new_temp + 3);
    if temp_changed || g.hourly.is_empty() || g.hourly.first().map(|p| p.temp) != Some(new_temp) {
        g.hourly = build_hourly_realistic(new_temp, t_min, t_max, Local::now().hour(), g.lat, g.lon);
    }
    g.synced_from_live = true;
}

// ---------------------------------------------------------------
// 2. 独立「天气」岛内页面 (`page == 10`) — 街道级精准定位 + 自定义天气源
// ---------------------------------------------------------------

/// 绘制纯矢量微缩天气图标（晴、多云、阴、雨、雪），彻底消除特殊符号缺字或风格不一。
fn draw_vector_weather_glyph(c: &crate::paint::Card, cx: f32, cy: f32, r: f32, desc: &str) {
    let is_rain = desc.contains("雨") || desc.contains("雷");
    let is_snow = desc.contains("雪");
    let is_cloud = desc.contains("云") || desc.contains("阴") || desc.contains("雾") || desc.contains("霾");
    let is_sun = desc.contains("晴") || (!is_rain && !is_snow && !is_cloud);

    if is_sun && !is_cloud && !is_rain {
        let sun_col = Color32::from_rgb(250, 204, 21);
        c.circle(cx, cy, r * 0.56, sun_col);
        for k in 0..8 {
            let a = (k as f32) * std::f32::consts::FRAC_PI_4;
            let p0 = (cx + a.cos() * r * 0.76, cy + a.sin() * r * 0.76);
            let p1 = (cx + a.cos() * r * 1.05, cy + a.sin() * r * 1.05);
            c.seg(p0, p1, (r * 0.16).max(1.1), sun_col);
        }
        return;
    }

    if is_sun && is_cloud {
        let sun_col = Color32::from_rgb(251, 191, 36);
        let sx = cx - r * 0.26;
        let sy = cy - r * 0.24;
        c.circle(sx, sy, r * 0.44, sun_col);
        for k in 0..6 {
            let a = (k as f32) * std::f32::consts::FRAC_PI_3;
            c.seg(
                (sx + a.cos() * r * 0.58, sy + a.sin() * r * 0.58),
                (sx + a.cos() * r * 0.80, sy + a.sin() * r * 0.80),
                (r * 0.14).max(1.0),
                sun_col,
            );
        }
    }

    let cloud_col = if is_rain {
        Color32::from_rgb(148, 163, 184)
    } else {
        Color32::from_rgb(226, 232, 240)
    };
    let cy_c = if is_rain { cy - r * 0.14 } else { cy + r * 0.08 };
    c.circle(cx - r * 0.28, cy_c + r * 0.08, r * 0.34, cloud_col);
    c.circle(cx + r * 0.04, cy_c - r * 0.10, r * 0.42, cloud_col);
    c.circle(cx + r * 0.36, cy_c + r * 0.10, r * 0.30, cloud_col);
    c.fill(cx - r * 0.32, cy_c + r * 0.06, r * 0.72, r * 0.36, r * 0.16, cloud_col);

    if is_rain {
        let rain_col = Color32::from_rgb(56, 189, 248);
        for dx in [-r * 0.26, 0.0, r * 0.26] {
            c.seg(
                (cx + dx, cy + r * 0.38),
                (cx + dx - r * 0.10, cy + r * 0.76),
                (r * 0.15).max(1.1),
                rain_col,
            );
        }
    } else if is_snow {
        let snow_col = Color32::from_rgb(186, 230, 253);
        for dx in [-r * 0.22, r * 0.18] {
            c.circle(cx + dx, cy + r * 0.56, (r * 0.13).max(1.1), snow_col);
        }
    }
}

pub fn draw_weather_island_page(c: &crate::paint::Card) -> bool {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    let cfg = config::get();
    let w_st = weather_state();

    // 充分利用展开岛横向与纵向空间（x = 34..385, w = 351; y = 8..235），消除底部空白与卡片内文字截断
    let left_x = 34.0_f32;
    let total_w = 351.0_f32;
    let top_y = 33.0_f32;
    let body_h = 202.0_f32;
    let card_r = 11.0_f32;
    let tile_bg = theme::CARD;

    // 次顶栏左端 (y = 8..28)：当前天气数据源与更新时间胶囊
    c.fill(left_x, 8.0, 232.0, 20.0, 7.0, Color32::from_white_alpha(13));
    c.stroke(left_x, 8.0, 232.0, 20.0, 7.0, 0.7, Color32::from_white_alpha(20));
    c.circle(left_x + 9.5, 18.0, 2.4, Color32::from_rgb(56, 189, 248));
    c.text(
        left_x + 16.0,
        21.2,
        &c.fit(&format!("数据源：{} · {}", w_st.source_name, w_st.updated_at), 8.2, W::Semibold, 208.0),
        8.2,
        W::Semibold,
        Color32::from_rgb(186, 230, 253),
        H::Left,
    );

    // 次顶栏右端：立即刷新胶囊 + 街道定位胶囊
    let fetching = WEATHER_FETCHING.load(Ordering::Relaxed);
    let (ref_r, ref_c) = crate::paint::capsule(
        c,
        274.0,
        8.0,
        50.0,
        20.0,
        "breezy-top-refresh",
        if fetching { Color32::from_rgb(14, 116, 144) } else { theme::BTN },
        theme::BTN_HOVER,
    );
    ref_c.text(
        299.0,
        21.2,
        if fetching { "同步中" } else { "↻ 刷新" },
        8.0,
        W::Semibold,
        theme::WHITE,
        H::Center,
    );
    if ref_r.on_hover_text("立即刷新当前街道天气与 24h 趋势").clicked() {
        refresh_weather_async(w_st.city.clone());
    }

    let locating = STREET_LOCATING.load(Ordering::Relaxed);
    let (loc_r, loc_c) = crate::paint::capsule(
        c,
        329.0,
        8.0,
        56.0,
        20.0,
        "breezy-street-locate",
        if locating { Color32::from_rgb(14, 116, 144) } else { theme::BTN },
        theme::BTN_HOVER,
    );
    loc_c.text(
        357.0,
        21.2,
        if locating { "定位中" } else { "街道定位" },
        8.2,
        W::Semibold,
        theme::WHITE,
        H::Center,
    );
    if loc_r
        .on_hover_text(format!(
            "当前街道：{} · {}\n坐标：{:.4}, {:.4}\n点击自动重新定位到当前街道",
            w_st.city, w_st.street, w_st.lat, w_st.lon
        ))
        .clicked()
    {
        locate_street_async();
    }

    // ── 左栏 (154px × 202px)：Breezy Weather 风格实况主卡 ──
    let left_w = 154.0_f32;
    c.fill(left_x, top_y, left_w, body_h, card_r, tile_bg);
    c.stroke(left_x, top_y, left_w, body_h, card_r, 0.9, Color32::from_rgb(56, 189, 248).gamma_multiply(0.38));

    // 1) 顶部街道与城市胶囊头（点击可直接刷新实况）
    let hdr_x = left_x + 7.0;
    let hdr_y = top_y + 7.0;
    let hdr_w = left_w - 14.0;
    let hdr_h = 20.0;
    c.fill(hdr_x, hdr_y, hdr_w, hdr_h, 6.5, Color32::from_white_alpha(14));
    c.circle(hdr_x + 8.5, hdr_y + 10.0, 2.6, Color32::from_rgb(56, 189, 248));
    c.circle(hdr_x + 8.5, hdr_y + 10.0, 1.0, tile_bg);
    let loc_title = format_street_address(&w_st.city, &w_st.street);
    c.text(
        hdr_x + 14.5,
        hdr_y + 13.5,
        &c.fit(&loc_title, 7.8, W::Semibold, hdr_w - 17.0),
        7.8,
        W::Semibold,
        Color32::from_rgb(165, 243, 252),
        H::Left,
    );
    if c.hit(hdr_x, hdr_y, hdr_w, hdr_h, "breezy-left-hdr-refresh")
        .on_hover_text(format!("{} · {}\n数据源：{}\n点击立即刷新天气", w_st.city, w_st.street, w_st.source_name))
        .clicked()
    {
        refresh_weather_async(w_st.city.clone());
    }

    // 2) 中部主温度 + 天气现象 + 高低温 + 矢量天气插画
    let clean_desc = w_st.desc.trim_start_matches(['☀', '☁', '❄', '⛈', '⛅', ' ']);
    c.text(left_x + 10.0, top_y + 57.0, &format!("{}°", w_st.temp), 28.0, W::Bold, theme::WHITE, H::Left);
    draw_vector_weather_glyph(c, left_x + left_w - 26.0, top_y + 47.0, 12.5, clean_desc);
    let hi_lo = w_st
        .forecast
        .first()
        .map(|d| format!("{}°/{}°", d.t_max, d.t_min))
        .unwrap_or_else(|| format!("{}°/{}°", w_st.temp + 3, w_st.temp - 4));
    c.text(
        left_x + 10.0,
        top_y + 73.0,
        &c.fit(&format!("{clean_desc} · 体感 {}° · {hi_lo}", w_st.feels_like), 8.2, W::Semibold, left_w - 18.0),
        8.2,
        W::Semibold,
        Color32::from_rgb(250, 204, 21),
        H::Left,
    );

    // 3) 中下部 2×2 精致四宫格指标卡（无截断省略号）
    let grid_y0 = top_y + 81.0;
    let gap = 4.5_f32;
    let cell_w = (left_w - 14.0 - gap) * 0.5; // 67.75px
    let cell_h = 41.0_f32;
    let sub_bg = Color32::from_white_alpha(12);

    let clean_uv = w_st
        .uv
        .trim_start_matches(['☀', ' '])
        .replace("弱 ", "")
        .replace("中等 ", "");
    let wind_short = w_st.wind.split('·').next().unwrap_or("实时风况").trim();
    let wind_spd = w_st
        .wind
        .split('·')
        .nth(1)
        .unwrap_or(&w_st.wind)
        .trim()
        .replace("实时风速", "");
    let aqi_clean = if w_st.aqi.starts_with("AQI") {
        format!("优 · {}", w_st.aqi)
    } else if w_st.aqi.contains("AQI") {
        w_st.aqi.clone()
    } else {
        format!("AQI · {}", w_st.aqi)
    };
    let sun_clean = w_st
        .sun_rise_set
        .replace(" 日出 · ", " / ")
        .replace("日出 · ", "/ ")
        .replace(" 日落", "")
        .trim()
        .to_owned();

    let cells: [(bool, &str, String, Color32); 4] = [
        (cfg.weather_show_aqi, "空气质量", aqi_clean, Color32::from_rgb(52, 211, 153)),
        (cfg.weather_show_uv_hum, "湿度 / UV", format!("{}% · {}", w_st.humidity, clean_uv), Color32::from_rgb(56, 189, 248)),
        (cfg.weather_show_wind, wind_short, wind_spd, Color32::from_rgb(129, 140, 248)),
        (cfg.weather_show_sun, "日出 / 日落", sun_clean, Color32::from_rgb(251, 191, 36)),
    ];

    let mut sun_clicked = false;
    for (idx, (_, label, val, dot_col)) in cells.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let cx = left_x + 7.0 + col as f32 * (cell_w + gap);
        let cy = grid_y0 + row as f32 * (cell_h + gap);
        let mut cell_bg = sub_bg;
        if idx == 3 {
            let sun_hit = c.hit(cx, cy, cell_w, cell_h, "breezy-sun-entry")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("查看日出日落与昼夜弧线");
            if sun_hit.hovered() {
                cell_bg = Color32::from_rgba_unmultiplied(251, 191, 36, 36);
            }
            if sun_hit.clicked() {
                sun_clicked = true;
            }
        }
        c.fill(cx, cy, cell_w, cell_h, 7.0, cell_bg);
        if idx == 3 {
            c.stroke(cx, cy, cell_w, cell_h, 7.0, 0.65, Color32::from_rgba_unmultiplied(251, 191, 36, 72));
            c.path(&[(cx + cell_w - 7.5, cy + cell_h * 0.5 - 3.0), (cx + cell_w - 5.0, cy + cell_h * 0.5), (cx + cell_w - 7.5, cy + cell_h * 0.5 + 3.0)], 1.1, Color32::from_rgb(251, 191, 36));
        }
        c.circle(cx + 7.5, cy + 11.5, 2.2, *dot_col);
        c.text(
            cx + 12.5,
            cy + 14.2,
            &c.fit(label, 7.5, W::Regular, cell_w - 15.0),
            7.5,
            W::Regular,
            theme::GRAY,
            H::Left,
        );
        c.text(
            cx + 6.5,
            cy + 31.0,
            &c.fit(val, 8.0, W::Bold, cell_w - 10.0),
            8.0,
            W::Bold,
            theme::WHITE,
            H::Left,
        );
    }

    // 4) 左栏底部大气压与出行舒适度条（充分利用左卡下半空间）
    let tip_y = grid_y0 + cell_h * 2.0 + gap * 2.0 + 1.0;
    let tip_h = (top_y + body_h - tip_y - 6.0).max(20.0);
    c.fill(hdr_x, tip_y, hdr_w, tip_h, 6.5, sub_bg);
    c.circle(hdr_x + 8.0, tip_y + tip_h * 0.5, 2.0, Color32::from_rgb(52, 211, 153));
    let tip_short = w_st.life_tip.split('·').next().unwrap_or(&w_st.life_tip).trim();
    c.text(
        hdr_x + 13.5,
        tip_y + tip_h * 0.5 + 3.0,
        &c.fit(&format!("{} · {}", w_st.pressure, tip_short), 7.4, W::Semibold, hdr_w - 17.0),
        7.4,
        W::Semibold,
        Color32::from_rgb(212, 214, 222),
        H::Left,
    );

    // ── 右栏 (191px × 202px)：24h 逐时温度趋势曲线卡 + 完整 5 日温度区间条预报卡 ──
    let rx = left_x + left_w + 6.0;
    let right_w = total_w - left_w - 6.0;

    let show_h = cfg.weather_show_hourly;
    let show_d = cfg.weather_show_daily;

    if show_h && show_d {
        let h_card_h = 78.0_f32;
        let d_card_h = body_h - h_card_h - 6.0; // 118.0px -> 完整展示 5 日预报！
        draw_breezy_hourly_card(c, rx, top_y, right_w, h_card_h, card_r, tile_bg, &w_st);
        draw_breezy_daily_card(c, rx, top_y + h_card_h + 6.0, right_w, d_card_h, card_r, tile_bg, &w_st);
    } else if show_h {
        draw_breezy_hourly_card(c, rx, top_y, right_w, body_h, card_r, tile_bg, &w_st);
    } else {
        draw_breezy_daily_card(c, rx, top_y, right_w, body_h, card_r, tile_bg, &w_st);
    }
    sun_clicked
}

/// [Shadcn UI 现代卡片天气岛 (`weather_ui_mode == 2`)]：
/// 全宽高利用率 Bento 布局 (`x = 34..385, y = 8..235`)：
/// - 上舱 (`x=34, y=33, w=351, h=72`)：Shadcn Hero 实况主卡（左侧大号气温 + 矢量气象徽标 + 街道级精准地址 Badge，右侧 2×2 指标矩阵）；
/// - 左下舱 (`x=34, y=111, w=172, h=124`)：24H 街道逐时气温渐变面积图卡；
/// - 右下舱 (`x=213, y=111, w=172, h=124`)：完整 5 日趋势与温差轨预报卡。
pub fn draw_shadcn_weather_island_page(c: &crate::paint::Card) -> bool {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    let w_st = weather_state();
    let left_x = 34.0_f32;
    let total_w = 351.0_f32;

    // 次顶栏左端 (y = 8..28)：当前天气数据源与更新时间胶囊
    c.fill(left_x, 8.0, 232.0, 20.0, 7.0, Color32::from_rgba_unmultiplied(24, 26, 34, 195));
    c.stroke(left_x, 8.0, 232.0, 20.0, 7.0, 0.7, Color32::from_white_alpha(20));
    c.circle(left_x + 9.5, 18.0, 2.4, Color32::from_rgb(56, 189, 248));
    c.text(
        left_x + 16.0,
        21.2,
        &c.fit(&format!("数据源：{} · {}", w_st.source_name, w_st.updated_at), 8.2, W::Semibold, 208.0),
        8.2,
        W::Semibold,
        Color32::from_rgb(186, 230, 253),
        H::Left,
    );

    // 次顶栏右端：刷新胶囊 + 街道定位胶囊
    let fetching = WEATHER_FETCHING.load(Ordering::Relaxed);
    let (ref_r, ref_c) = crate::paint::capsule(
        c,
        274.0,
        8.0,
        50.0,
        20.0,
        "shadcn-top-refresh",
        if fetching {
            Color32::from_rgb(14, 116, 144)
        } else {
            Color32::from_rgba_unmultiplied(39, 39, 42, 215)
        },
        Color32::from_rgba_unmultiplied(63, 63, 70, 235),
    );
    ref_c.text(
        299.0,
        21.2,
        if fetching { "同步中" } else { "↻ 刷新" },
        8.0,
        W::Semibold,
        theme::WHITE,
        H::Center,
    );
    if ref_r.on_hover_text("立即刷新当前街道天气与 24h 趋势").clicked() {
        refresh_weather_async(w_st.city.clone());
    }

    let locating = STREET_LOCATING.load(Ordering::Relaxed);
    let (loc_r, loc_c) = crate::paint::capsule(
        c,
        329.0,
        8.0,
        56.0,
        20.0,
        "shadcn-street-locate",
        if locating {
            Color32::from_rgb(14, 116, 144)
        } else {
            Color32::from_rgba_unmultiplied(39, 39, 42, 215)
        },
        Color32::from_rgba_unmultiplied(63, 63, 70, 235),
    );
    loc_c.text(
        357.0,
        21.2,
        if locating { "定位中" } else { "街道定位" },
        8.2,
        W::Semibold,
        theme::WHITE,
        H::Center,
    );
    if loc_r
        .on_hover_text(format!(
            "当前街道：{} · {}\n点击重新定位当前街道",
            w_st.city, w_st.street
        ))
        .clicked()
    {
        locate_street_async();
    }

    let card_bg = Color32::from_rgba_unmultiplied(15, 16, 22, 188);
    let card_border = Color32::from_white_alpha(24);
    let muted_fg = Color32::from_rgb(161, 161, 170);
    let sky_acc = Color32::from_rgb(56, 189, 248);
    let amber_acc = Color32::from_rgb(250, 204, 21);

    // ── 1) 上舱：Shadcn Hero 实况主卡 (x=34, y=33, w=351, h=72) ──
    let (hx, hy, hw, hh) = (left_x, 33.0_f32, total_w, 72.0_f32);
    c.fill(hx, hy, hw, hh, 11.0, card_bg);
    c.stroke(hx, hy, hw, hh, 11.0, 1.0, card_border);
    c.seg((hx + 11.0, hy + 0.8), (hx + hw - 11.0, hy + 0.8), 0.7, Color32::from_white_alpha(28));

    let clean_desc = w_st.desc.trim_start_matches(['☀', '☁', '❄', '⛈', '⛅', ' ']);
    draw_vector_weather_glyph(c, hx + 25.0, hy + 25.0, 12.0, clean_desc);
    let temp_w = c.text(hx + 44.0, hy + 35.5, &format!("{}°", w_st.temp), 27.0, W::Bold, theme::WHITE, H::Left);
    let desc_x = (hx + 48.0 + temp_w).max(hx + 96.0);
    let hi_lo = w_st
        .forecast
        .first()
        .map(|d| format!("{}°/{}°", d.t_max, d.t_min))
        .unwrap_or_else(|| format!("{}°/{}°", w_st.temp + 3, w_st.temp - 4));
    c.text(
        desc_x,
        hy + 22.0,
        &c.fit(&format!("{clean_desc} · {hi_lo}"), 8.8, W::Bold, (hx + 170.0 - desc_x).max(40.0)),
        8.8,
        W::Bold,
        amber_acc,
        H::Left,
    );
    c.text(
        desc_x,
        hy + 35.5,
        &format!("体感 {}°C · {}", w_st.feels_like, w_st.pressure),
        7.6,
        W::Semibold,
        muted_fg,
        H::Left,
    );

    // 街道胶囊 Badge（精准显示到街道，点击刷新天气）
    let loc_label = format_street_address(&w_st.city, &w_st.street);
    let badge_x = hx + 10.0;
    let badge_y = hy + 46.5;
    let badge_w = 160.0_f32;
    c.fill(badge_x, badge_y, badge_w, 18.5, 6.0, Color32::from_white_alpha(14));
    c.stroke(badge_x, badge_y, badge_w, 18.5, 6.0, 0.7, sky_acc.gamma_multiply(0.45));
    c.circle(badge_x + 8.5, badge_y + 9.2, 2.5, sky_acc);
    c.text(
        badge_x + 14.5,
        badge_y + 13.0,
        &c.fit(&loc_label, 7.8, W::Semibold, badge_w - 18.0),
        7.8,
        W::Semibold,
        Color32::from_rgb(186, 230, 253),
        H::Left,
    );
    if c.hit(badge_x, badge_y, badge_w, 18.5, "shadcn-w-refresh")
        .on_hover_text("点击刷新实时街道天气")
        .clicked()
    {
        refresh_weather_async(w_st.city.clone());
    }

    // 右侧 2×2 Shadcn 指标微卡矩阵 (x = hx + 176..hx + 344)
    let clean_uv = w_st
        .uv
        .trim_start_matches(['☀', ' '])
        .replace("弱 ", "")
        .replace("中等 ", "");
    let wind_short = w_st
        .wind
        .split('·')
        .next_back()
        .unwrap_or(&w_st.wind)
        .trim()
        .to_owned();
    let sun_short = w_st
        .sun_rise_set
        .replace(" 日出 · ", " / ")
        .replace("日出 · ", "/ ")
        .replace(" 日落", "");
    let aqi_clean = if w_st.aqi.starts_with("AQI") {
        format!("优 · {}", w_st.aqi)
    } else if w_st.aqi.contains("AQI") {
        w_st.aqi.clone()
    } else {
        format!("AQI · {}", w_st.aqi)
    };
    let metrics = [
        ("空气质量", aqi_clean, Color32::from_rgb(52, 211, 153)),
        ("湿度 / UV", format!("{}% · {}", w_st.humidity, clean_uv), sky_acc),
        ("实时风速", wind_short, Color32::from_rgb(168, 85, 247)),
        ("日出日落", sun_short, amber_acc),
    ];
    let mut sun_clicked = false;
    for (idx, (m_lbl, m_val, m_col)) in metrics.iter().enumerate() {
        let col = idx % 2;
        let row = idx / 2;
        let mx = hx + 175.0 + col as f32 * 85.5;
        let my = hy + 7.5 + row as f32 * 29.5;
        let mw = 81.5_f32;
        let mh = 26.5_f32;
        let mut m_bg = Color32::from_white_alpha(12);
        if idx == 3 {
            let sun_hit = c.hit(mx, my, mw, mh, "shadcn-sun-entry")
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("查看日出日落与昼夜弧线");
            if sun_hit.hovered() {
                m_bg = Color32::from_rgba_unmultiplied(245, 158, 11, 38);
            }
            if sun_hit.clicked() {
                sun_clicked = true;
            }
        }
        c.fill(mx, my, mw, mh, 6.5, m_bg);
        if idx == 3 {
            c.stroke(mx, my, mw, mh, 6.5, 0.65, Color32::from_rgba_unmultiplied(245, 158, 11, 78));
            c.path(&[(mx + mw - 7.5, my + mh * 0.5 - 2.8), (mx + mw - 5.2, my + mh * 0.5), (mx + mw - 7.5, my + mh * 0.5 + 2.8)], 1.1, amber_acc);
        }
        c.circle(mx + 6.5, my + 8.5, 1.9, *m_col);
        c.text(mx + 11.8, my + 10.8, m_lbl, 7.0, W::Semibold, muted_fg, H::Left);
        c.text(mx + 6.5, my + 21.8, &c.fit(m_val, 7.8, W::Bold, mw - 10.0), 7.8, W::Bold, theme::WHITE, H::Left);
    }

    // ── 2) 左下舱：Shadcn 24H 逐时趋势面积卡 (x=34, y=111, w=172, h=124) ──
    let (lx, ly, lw, lh) = (left_x, 111.0_f32, 172.0_f32, 124.0_f32);
    c.fill(lx, ly, lw, lh, 11.0, card_bg);
    c.stroke(lx, ly, lw, lh, 11.0, 1.0, card_border);
    c.text(lx + 10.0, ly + 15.5, "24H 街道逐时气温", 8.4, W::Bold, sky_acc, H::Left);

    let pts_data = &w_st.hourly;
    if pts_data.len() >= 2 {
        let n = pts_data.len().min(6);
        let slice = &pts_data[..n];
        let min_i = slice.iter().map(|p| p.temp).min().unwrap_or(15);
        let max_i = slice.iter().map(|p| p.temp).max().unwrap_or(25);
        c.text(
            lx + lw - 10.0,
            ly + 15.5,
            &format!("{min_i}°~{max_i}°"),
            7.4,
            W::Semibold,
            muted_fg,
            H::Right,
        );
        let min_t = min_i as f32;
        let max_t = (max_i as f32).max(min_t + 2.0);
        let pad_x = 16.0_f32;
        let span_w = (lw - pad_x * 2.0).max(40.0);
        let curve_top = ly + 35.0;
        let curve_h = 58.0_f32;
        let base_line_y = curve_top + curve_h + 7.0;

        let mut coords = Vec::with_capacity(n);
        for (i, pt) in slice.iter().enumerate() {
            let px = lx + pad_x + (i as f32 / (n - 1) as f32) * span_w;
            let norm = ((pt.temp as f32 - min_t) / (max_t - min_t)).clamp(0.0, 1.0);
            let py = curve_top + (1.0 - norm) * curve_h;
            coords.push((px, py, pt));
        }
        let mut mesh = egui::Mesh::default();
        for (i, &(px, py, _)) in coords.iter().enumerate() {
            mesh.colored_vertex(c.pos(px, py), Color32::from_rgba_unmultiplied(56, 189, 248, 58));
            mesh.colored_vertex(c.pos(px, base_line_y), Color32::TRANSPARENT);
            if i > 0 {
                let b = (i as u32 - 1) * 2;
                mesh.add_triangle(b, b + 1, b + 2);
                mesh.add_triangle(b + 1, b + 3, b + 2);
            }
        }
        c.painter.add(egui::Shape::mesh(mesh));
        let line_pts: Vec<(f32, f32)> = coords.iter().map(|&(px, py, _)| (px, py)).collect();
        c.path(&line_pts, 1.7, sky_acc);
        for (px, py, pt) in coords {
            c.circle(px, py, 2.4, theme::WHITE);
            c.circle(px, py, 1.4, sky_acc);
            c.text(px, py - 4.8, &format!("{}°", pt.temp), 7.6, W::Bold, theme::WHITE, H::Center);
            c.text(px, ly + lh - 7.5, &pt.hour_label, 7.2, W::Regular, muted_fg, H::Center);
        }
    }

    // ── 3) 右下舱：Shadcn 完整 5 日温差条预报卡 (x=213, y=111, w=172, h=124) ──
    let (rx, ry, rw, rh) = (left_x + 179.0, 111.0_f32, 172.0_f32, 124.0_f32);
    c.fill(rx, ry, rw, rh, 11.0, card_bg);
    c.stroke(rx, ry, rw, rh, 11.0, 1.0, card_border);
    c.text(rx + 10.0, ry + 15.5, "5 日趋势与温差轨", 8.4, W::Bold, amber_acc, H::Left);

    let count = 5usize;
    let global_min = w_st.forecast.iter().take(count).map(|d| d.t_min).min().unwrap_or(10) as f32;
    let global_max = (w_st.forecast.iter().take(count).map(|d| d.t_max).max().unwrap_or(25) as f32).max(global_min + 2.0);
    let step_y = 19.2_f32;
    for (i, d) in w_st.forecast.iter().take(count).enumerate() {
        let row_y = ry + 33.5 + i as f32 * step_y;
        c.text(rx + 10.0, row_y, &d.day_label, 8.0, W::Semibold, theme::WHITE, H::Left);
        let clean_d = d.desc.trim_start_matches(['☀', '☁', '❄', '⛈', '⛅', ' ']);
        draw_vector_weather_glyph(c, rx + 44.0, row_y - 3.0, 4.0, clean_d);
        c.text(rx + 52.0, row_y, &c.fit(clean_d, 7.6, W::Regular, 34.0), 7.6, W::Regular, muted_fg, H::Left);
        c.text(rx + 99.0, row_y, &format!("{}°", d.t_min), 7.6, W::Semibold, muted_fg, H::Right);

        let bar_x = rx + 103.0;
        let bar_w = 41.0_f32;
        let bar_y = row_y - 4.5;
        c.fill(bar_x, bar_y, bar_w, 3.5, 1.75, Color32::from_white_alpha(20));
        let u0 = ((d.t_min as f32 - global_min) / (global_max - global_min)).clamp(0.0, 0.88);
        let u1 = ((d.t_max as f32 - global_min) / (global_max - global_min)).clamp(u0 + 0.14, 1.0);
        c.fill(bar_x + u0 * bar_w, bar_y, (u1 - u0) * bar_w, 3.5, 1.75, amber_acc);
        c.text(rx + rw - 8.0, row_y, &format!("{}°", d.t_max), 7.8, W::Bold, theme::WHITE, H::Right);
    }
    // [编译修复] 函数声明为 `-> bool`，但尾部只跑完循环没返回「太阳按钮是否被点」。
    sun_clicked
}

fn draw_breezy_hourly_card(
    c: &crate::paint::Card,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    tile_bg: Color32,
    w_st: &WislandWeatherState,
) {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    c.fill(x, y, w, h, r, tile_bg);
    c.stroke(x, y, w, h, r, 0.85, Color32::from_rgb(56, 189, 248).gamma_multiply(0.35));
    c.text(x + 10.0, y + 14.0, "24小时街道逐时趋势", 8.4, W::Bold, Color32::from_rgb(56, 189, 248), H::Left);

    let pts_data = &w_st.hourly;
    if pts_data.len() < 2 {
        return;
    }
    let n = pts_data.len().min(6);
    let slice = &pts_data[..n];
    let min_i = slice.iter().map(|p| p.temp).min().unwrap_or(15);
    let max_i = slice.iter().map(|p| p.temp).max().unwrap_or(25);
    c.text(
        x + w - 10.0,
        y + 14.0,
        &format!("{min_i}°~{max_i}°"),
        7.4,
        W::Semibold,
        theme::GRAY,
        H::Right,
    );
    let min_t = min_i as f32;
    let max_t = (max_i as f32).max(min_t + 2.0);
    let pad_x = 17.0_f32;
    let span_w = (w - pad_x * 2.0).max(40.0);
    let curve_top = y + 28.0;
    let curve_h = (h - 45.0).clamp(16.0, 56.0);
    let base_line_y = curve_top + curve_h + 5.0;

    let mut coords = Vec::with_capacity(n);
    for (i, pt) in slice.iter().enumerate() {
        let px = x + pad_x + (i as f32 / (n - 1) as f32) * span_w;
        let norm = ((pt.temp as f32 - min_t) / (max_t - min_t)).clamp(0.0, 1.0);
        let py = curve_top + (1.0 - norm) * curve_h;
        coords.push((px, py, pt));
    }

    let mut mesh = egui::Mesh::default();
    for (i, &(px, py, _)) in coords.iter().enumerate() {
        mesh.colored_vertex(c.pos(px, py), Color32::from_rgba_unmultiplied(56, 189, 248, 50));
        mesh.colored_vertex(c.pos(px, base_line_y), Color32::TRANSPARENT);
        if i > 0 {
            let b = (i as u32 - 1) * 2;
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b + 1, b + 3, b + 2);
        }
    }
    c.painter.add(egui::Shape::mesh(mesh));

    let line_pts: Vec<(f32, f32)> = coords.iter().map(|&(px, py, _)| (px, py)).collect();
    c.path(&line_pts, 1.7, Color32::from_rgb(56, 189, 248));
    for (px, py, pt) in coords {
        c.circle(px, py, 2.3, Color32::from_rgb(250, 204, 21));
        c.text(px, py - 4.2, &format!("{}°", pt.temp), 7.6, W::Bold, theme::WHITE, H::Center);
        c.text(px, y + h - 5.0, &pt.hour_label, 7.2, W::Regular, theme::GRAY, H::Center);
    }
}

fn draw_breezy_daily_card(
    c: &crate::paint::Card,
    x: f32,
    y: f32,
    w: f32,
    h: f32,
    r: f32,
    tile_bg: Color32,
    w_st: &WislandWeatherState,
) {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    c.fill(x, y, w, h, r, tile_bg);
    c.stroke(x, y, w, h, r, 0.85, Color32::from_rgb(250, 204, 21).gamma_multiply(0.35));
    c.text(x + 10.0, y + 14.0, "5日趋势与温差条", 8.4, W::Bold, Color32::from_rgb(250, 204, 21), H::Left);

    let count = 5usize;
    let global_min = w_st.forecast.iter().take(count).map(|d| d.t_min).min().unwrap_or(10) as f32;
    let global_max = (w_st.forecast.iter().take(count).map(|d| d.t_max).max().unwrap_or(25) as f32).max(global_min + 2.0);
    let step_y = ((h - 22.0) / count as f32).clamp(16.0, 22.0);

    for (i, d) in w_st.forecast.iter().take(count).enumerate() {
        let ry = y + 29.0 + i as f32 * step_y;
        c.text(x + 10.0, ry, &d.day_label, 8.2, W::Semibold, theme::WHITE, H::Left);
        let clean_d = d.desc.trim_start_matches(['☀', '☁', '❄', '⛈', '⛅', ' ']);
        draw_vector_weather_glyph(c, x + 45.0, ry - 3.0, 4.2, clean_d);
        c.text(x + 53.0, ry, &c.fit(clean_d, 8.0, W::Regular, 38.0), 8.0, W::Regular, theme::GRAY, H::Left);
        c.text(x + 104.0, ry, &format!("{}°", d.t_min), 8.0, W::Semibold, theme::GRAY, H::Right);

        let bar_x = x + 108.0;
        let bar_w = (w - 136.0).max(28.0);
        let bar_y = ry - 4.6;
        c.fill(bar_x, bar_y, bar_w, 3.6, 1.8, Color32::from_gray(44));
        let u0 = ((d.t_min as f32 - global_min) / (global_max - global_min)).clamp(0.0, 0.9);
        let u1 = ((d.t_max as f32 - global_min) / (global_max - global_min)).clamp(u0 + 0.12, 1.0);
        c.fill(
            bar_x + u0 * bar_w,
            bar_y,
            (u1 - u0) * bar_w,
            3.6,
            1.8,
            Color32::from_rgb(251, 191, 36),
        );
        c.text(x + w - 8.0, ry, &format!("{}°", d.t_max), 8.2, W::Bold, theme::WHITE, H::Right);
    }
}

// ---------------------------------------------------------------
// 3. 独立「翻译」岛内页面 (`page == 11`) — 支持「暂时冻结岛体，不收回岛体」
// ---------------------------------------------------------------

pub fn draw_translate_island_page(c: &crate::paint::Card) {
    use crate::{
        paint::H,
        theme::{self, W},
    };
    let cfg = config::get();
    let left_x = 40.0_f32;
    let total_w = 340.0_f32;

    let (mut tr_in, tr_out, tr_mode, _tr_status) = {
        let g = translate_state().lock().unwrap();
        (g.input.clone(), g.output.clone(), g.target_mode, g.status.clone())
    };

    // ── 统一次顶栏 (y = 14..35)：左侧 `[自动识别 ∨] → 中英互译`，中右 `引擎切换`，最右 `[冻结岛体]` 同行对齐！──
    let lang_y = 14.0_f32;
    let pinned = cfg.translate_pin_island;
    let (pin_r, pin_c) = crate::paint::capsule(
        c,
        322.0,
        lang_y + 1.0,
        58.0,
        19.0,
        "tr-page-pin-island",
        if pinned { Color32::from_rgb(16, 185, 129) } else { theme::BTN },
        theme::BTN_HOVER,
    );
    pin_c.text(
        351.0,
        lang_y + 13.8,
        if pinned { "已冻结岛" } else { "冻结岛体" },
        8.2,
        W::Semibold,
        theme::WHITE,
        H::Center,
    );
    if pin_r
        .on_hover_text(if pinned {
            "当前已冻结灵动岛：鼠标移开也不会收回岛体（点击解除冻结）"
        } else {
            "点击暂时冻结岛体：在翻译页面鼠标移开也不自动收回岛体"
        })
        .clicked()
    {
        config::update(|c| c.translate_pin_island = !pinned);
    }
    let src_pill_w = 68.0_f32;
    let src_pill_h = 21.0_f32;
    let (src_r, src_c) = crate::paint::capsule(
        c,
        left_x,
        lang_y,
        src_pill_w,
        src_pill_h,
        "fly-tr-src-lang",
        Color32::from_white_alpha(18),
        Color32::from_white_alpha(28),
    );
    src_c.stroke(left_x, lang_y, src_pill_w, src_pill_h, 7.5, 0.8, Color32::from_white_alpha(26));
    src_c.text(left_x + 9.0, lang_y + 14.5, "自动识别", 9.6, W::Bold, theme::WHITE, H::Left);
    // ∨ 下拉箭头
    let vx = left_x + src_pill_w - 11.5;
    let vy = lang_y + 10.5;
    src_c.seg((vx - 3.0, vy - 1.2), (vx, vy + 1.8), 1.1, theme::GRAY);
    src_c.seg((vx, vy + 1.8), (vx + 3.0, vy - 1.2), 1.1, theme::GRAY);

    // → 箭头与目标语言模式（点击循环切换：中英互译 / 译为中文 / 译为英文 / 译为日文）
    let mode_label = match tr_mode {
        1 => "译为中文",
        2 => "译为英文",
        3 => "译为日文",
        _ => "中英互译",
    };
    c.text(left_x + src_pill_w + 8.0, lang_y + 14.5, "→", 10.0, W::Regular, theme::GRAY, H::Left);
    c.text(left_x + src_pill_w + 22.0, lang_y + 14.5, mode_label, 10.0, W::Bold, Color32::from_gray(225), H::Left);
    if src_r.clicked() || c.hit(left_x + src_pill_w + 4.0, lang_y, 64.0, src_pill_h, "fly-tr-tgt-lang").clicked() {
        let next_m = (tr_mode + 1) % 4;
        trigger_translate_async(tr_in.clone(), next_m);
    }

    // 右侧当前翻译引擎名称（硅基流动 / DeepSeek / OpenAI / 本地引擎，点击可切换）
    let provider_name = match cfg.translate_engine {
        1 => "硅基流动",
        2 => "DeepSeek",
        3 => "OpenAI",
        _ => if cfg.translate_ai_base_url.contains("siliconflow") { "硅基流动" } else if cfg.translate_ai_base_url.contains("deepseek") { "DeepSeek" } else { "硅基流动" },
    };
    let eng_hit = c.hit(248.0, lang_y, 66.0, src_pill_h, "fly-tr-provider");
    c.text(
        312.0,
        lang_y + 14.5,
        provider_name,
        9.6,
        W::Bold,
        if eng_hit.hovered() { theme::WHITE } else { Color32::from_gray(200) },
        H::Right,
    );
    if eng_hit.clicked() {
        let next_e = (cfg.translate_engine + 1) % 4;
        config::update(|c| c.translate_engine = next_e);
    }

    // (2) 中部原文输入框（适配加长后的岛体空间，高度 72px）
    let box_y = 41.0_f32;
    let box_h = 72.0_f32;
    let box_r = 9.0_f32;
    c.fill(left_x, box_y, total_w, box_h, box_r, Color32::from_white_alpha(14));
    c.stroke(left_x, box_y, total_w, box_h, box_r, 1.0, Color32::from_white_alpha(32));

    // 原文多行可编辑输入（支持直接在框内输入文字，与图九一致）
    let edit_rect = c.rect(left_x + 10.0, box_y + 8.0, total_w - 38.0, box_h - 14.0);
    let mut ui_child = egui::Ui::new(
        c.ui.ctx().clone(),
        egui::Id::new(("fly-tr-input-ui", c.live)),
        egui::UiBuilder::new()
            .max_rect(edit_rect)
            .layer_id(c.painter.layer_id())
            .style(c.ui.style().clone()),
    );
    ui_child.set_clip_rect(edit_rect.expand2(egui::vec2(2.0, 2.0)).intersect(c.painter.clip_rect()));
    ui_child.set_opacity(c.painter.opacity());
    ui_child.visuals_mut().text_cursor.blink = true;
    ui_child.visuals_mut().text_cursor.stroke = egui::Stroke::new(1.6, theme::WHITE);
    if !c.live {
        ui_child.disable();
    }
    let te = egui::TextEdit::multiline(&mut tr_in)
        .id(egui::Id::new("fly-tr-multiline-input"))
        .frame(false)
        .margin(egui::Margin::symmetric(2, 0))
        .text_color(theme::WHITE)
        .font(theme::font(11.2, W::Semibold))
        .hint_text(
            egui::RichText::new("输入或粘贴要翻译的文本…")
                .color(theme::GRAY)
                .font(theme::font(10.8, W::Regular)),
        )
        .desired_width(edit_rect.width())
        .desired_rows(2);
    let te_resp = ui_child.add(te);
    if te_resp.changed() {
        if let Ok(mut g) = translate_state().lock() {
            g.input = tr_in.clone();
        }
    }

    // 输入框右上角圆形 [×] 清空按钮（如图九右上角深灰圆底 ×）
    let clr_cx = left_x + total_w - 14.0;
    let clr_cy = box_y + 14.0;
    let clr_hit = c.hit(clr_cx - 9.0, clr_cy - 9.0, 18.0, 18.0, "fly-tr-clear-input")
        ;
    c.circle(
        clr_cx,
        clr_cy,
        8.2,
        if clr_hit.hovered() { Color32::from_white_alpha(42) } else { Color32::from_white_alpha(22) },
    );
    crate::paint::close(c, clr_cx, clr_cy, 2.5, if clr_hit.hovered() { theme::WHITE } else { Color32::from_gray(200) });
    if clr_hit.clicked() {
        if let Ok(mut g) = translate_state().lock() {
            g.input.clear();
            g.output.clear();
        }
    }

    // (3) 中部分割细线
    let div_y = 122.0_f32;
    c.seg((left_x, div_y), (left_x + total_w, div_y), 0.7, Color32::from_white_alpha(18));

    // (4) 下半部译文显示区（支持展示 4 行完整译文）
    let out_y = 140.0_f32;
    if tr_out.trim().is_empty() {
        c.text(left_x + 8.0, out_y, "译文会显示在这里", 11.0, W::Semibold, Color32::from_gray(155), H::Left);
    } else {
        for (idx, line) in c.wrap(&tr_out, 11.2, W::Semibold, total_w - 110.0, 4).iter().enumerate() {
            c.text(left_x + 8.0, out_y + idx as f32 * 15.5, line, 11.2, W::Semibold, theme::WHITE, H::Left);
        }
    }

    // (5) 右下角操作栏：圆形 [📋 粘贴] 按钮 + 圆形 [⧉ 复制] 按钮 + 亮白胶囊 [翻译] 主按钮
    let act_cy = 204.0_f32;

    // 圆形按钮 1：粘贴剪贴板原文并翻译 (📋)
    let paste_cx = left_x + total_w - 92.0;
    let paste_hit = c.hit(paste_cx - 11.0, act_cy - 11.0, 22.0, 22.0, "fly-tr-paste-btn")
        ;
    c.circle(
        paste_cx,
        act_cy,
        10.5,
        if paste_hit.hovered() { Color32::from_white_alpha(32) } else { Color32::from_white_alpha(18) },
    );
    c.stroke(paste_cx - 3.8, act_cy - 4.2, 7.6, 9.0, 1.4, 1.0, theme::WHITE);
    c.fill(paste_cx - 2.0, act_cy - 5.2, 4.0, 2.0, 0.8, theme::WHITE);
    if paste_hit.clicked() {
        if let Some(clip) = read_win32_clipboard_text() {
            trigger_translate_async(clip, tr_mode);
        }
    }

    // 圆形按钮 2：复制译文 (⧉)
    let copy_cx = left_x + total_w - 64.0;
    let copy_hit = c.hit(copy_cx - 11.0, act_cy - 11.0, 22.0, 22.0, "fly-tr-copy-btn")
        ;
    c.circle(
        copy_cx,
        act_cy,
        10.5,
        if copy_hit.hovered() { Color32::from_white_alpha(32) } else { Color32::from_white_alpha(18) },
    );
    let cp_col = if !tr_out.is_empty() { theme::WHITE } else { Color32::from_gray(120) };
    c.stroke(copy_cx - 4.2, act_cy - 4.2, 6.4, 6.8, 1.2, 1.0, cp_col);
    c.stroke(copy_cx - 2.0, act_cy - 2.0, 6.4, 6.8, 1.2, 1.0, cp_col);
    if copy_hit.clicked() && !tr_out.is_empty() {
        c.ui.ctx().copy_text(tr_out.clone());
    }

    // 亮白胶囊按钮：[翻译]（登记独立 tone_since 防止皮肤换色将深色文字反相为白字导致看不见）
    let btn_w = 46.0_f32;
    let btn_h = 22.0_f32;
    let btn_x = left_x + total_w - btn_w;
    let btn_y = act_cy - btn_h * 0.5;
    let btn_from = crate::skin_paint::shape_count(c.ui.ctx(), c.painter.layer_id());
    let (tr_r, tr_c) = crate::paint::capsule(
        c,
        btn_x,
        btn_y,
        btn_w,
        btn_h,
        "fly-tr-submit-btn",
        Color32::from_rgb(236, 238, 242),
        Color32::WHITE,
    );
    tr_c.text(
        btn_x + btn_w * 0.5,
        btn_y + 15.2,
        "翻译",
        10.0,
        W::Bold,
        Color32::from_rgb(32, 32, 32),
        H::Center,
    );
    crate::skin_paint::tone_since(c.ui.ctx(), c.painter.layer_id(), btn_from, |col, _| col);
    if tr_r.clicked() {
        let src = if tr_in.trim().is_empty() {
            read_win32_clipboard_text().unwrap_or_default()
        } else {
            tr_in.clone()
        };
        trigger_translate_async(src, tr_mode);
    }
}

// ---------------------------------------------------------------
// 4. 独立「天气」设置页 (`settings_tab == 18`) — 街道级精准定位 + 自定义接入天气源
// ---------------------------------------------------------------

pub fn draw_weather_island_preview_card(app: &mut crate::app::App, ui: &mut egui::Ui) {
    let cfg = config::get();
    let acc = Color32::from_rgb(56, 189, 248);
    settings_label(ui, "天气岛实时预览");
    crate::extras::settings_card(ui, |ui| {
        let preview_h = 278.0_f32;
        let avail_w = (crate::extras::layout_avail_w(ui) - 8.0).max(180.0);
        let scale_k = ((avail_w - 8.0) / 419.0).clamp(0.52, 1.06);
        let draw_w = 419.0 * scale_k;
        let draw_h = preview_h * scale_k;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, draw_h + 12.0), Sense::hover());
            let island_r = Rect::from_center_size(rect.center(), egui::vec2(draw_w, draw_h));
            let is_light_skin = matches!(app.settings.skin, crate::model::Skin::Cream | crate::model::Skin::GlassLight);
            let preview_bg = match app.settings.skin {
                crate::model::Skin::Fluid => {
                    let c0 = app.fluid.compose(209.5, 139.0, 419.0, preview_h, 1.0);
                    if c0.a() > 0 {
                        let af = c0.a() as f32 / 255.0;
                        Color32::from_rgb(
                            (c0.r() as f32 / af).clamp(24.0, 96.0) as u8,
                            (c0.g() as f32 / af).clamp(24.0, 96.0) as u8,
                            (c0.b() as f32 / af).clamp(26.0, 102.0) as u8,
                        )
                    } else {
                        Color32::from_rgb(40, 42, 46)
                    }
                }
                crate::model::Skin::Cream | crate::model::Skin::GlassLight => app.preview_fill(),
                crate::model::Skin::GlassDark => Color32::from_rgb(30, 32, 38),
                crate::model::Skin::LiquidGlass => Color32::from_rgb(36, 40, 48),
                crate::model::Skin::Classic => active_win_fill(235).unwrap_or_else(|| Color32::from_rgb(24, 26, 32)),
            };
            let rim_col = if is_light_skin {
                Color32::from_black_alpha(28)
            } else {
                Color32::from_rgba_premultiplied(28, 30, 34, 28)
            };
            ui.painter().rect_filled(island_r.translate(egui::vec2(0.0, 2.0)), 24.0 * scale_k, Color32::from_black_alpha(48));
            ui.painter().rect_filled(island_r, 24.0 * scale_k, preview_bg);
            ui.painter().rect_stroke(
                island_r,
                24.0 * scale_k,
                Stroke::new(1.0_f32, rim_col),
                egui::StrokeKind::Inside,
            );
            let mut pcard = crate::paint::Card::new(ui, island_r.min, island_r).scaled(0.0, 0.0, scale_k);
            pcard.live = true;
            let content_from = crate::skin_paint::shape_count(ui.ctx(), pcard.painter.layer_id());
            if cfg.show_island_page_title {
                pcard.text(34.0, 24.0, "实时", 15.0, crate::theme::W::Bold, Color32::WHITE, crate::paint::H::Left);
                let w1 = pcard.text_w("实时", 15.0, crate::theme::W::Bold);
                pcard.text(34.0 + w1 + 1.5, 24.0, "天气", 15.0, crate::theme::W::Bold, acc, crate::paint::H::Left);
            }
            let now_dt = Local::now();
            let wk = ["一", "二", "三", "四", "五", "六", "日"][now_dt.weekday().num_days_from_monday() as usize];
            let top_clock = format!("{}月{}日 周{}  {}", now_dt.month(), now_dt.day(), wk, now_dt.format("%H:%M:%S"));
            pcard.text(209.5, 23.0, &top_clock, 10.8, crate::theme::W::Semibold, Color32::from_gray(225), crate::paint::H::Center);
            crate::paint::gear(&pcard, 385.0, 19.0, Color32::from_gray(180));
            let body_card = pcard.shifted(0.0, 24.0);
            match cfg.weather_ui_mode {
                1 => {
                    app.draw_classic_weather_preview(&body_card);
                }
                2 => {
                    let _ = draw_shadcn_weather_island_page(&body_card);
                }
                _ => {
                    let _ = draw_weather_island_page(&body_card);
                }
            }
            // 底部简洁页指示点，与真实天气岛 278px 高度比例一致
            let dot_y = preview_h - 10.5;
            for i in 0..5 {
                let dx = 209.5 + (i as f32 - 2.0) * 10.0;
                if i == 2 {
                    pcard.fill(dx - 7.0, dot_y - 2.4, 14.0, 4.8, 2.4, acc);
                } else {
                    pcard.circle(dx, dot_y, 2.2, Color32::from_white_alpha(48));
                }
            }
            // 关键：对预览层执行与主岛一致的 skin_paint::recolor_layer，消除 egui sRGB 预乘导致卡片发白灰块的问题
            let content_end = crate::skin_paint::shape_count(ui.ctx(), pcard.painter.layer_id());
            let preview_palette = app.skin_palette().unwrap_or(crate::skin_paint::GLASS_DARK);
            crate::skin_paint::recolor_layer(ui.ctx(), pcard.painter.layer_id(), &[(content_from, content_end)], &preview_palette);
        });
    });
}

pub fn draw_weather_settings_page(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    let mut before = cfg.clone();
    let acc = active_accent_color();
    let w_st = weather_state();
    let mut preset_switched = false;

    // 街道级高精定位
    settings_label(ui, "街道级定位");
    crate::extras::settings_card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("城市：").size(11.5).strong());
            ui.add_sized([78.0, 24.0], egui::TextEdit::singleline(&mut cfg.weather_city));
            ui.label(RichText::new("区县与街道：").size(11.5).strong());
            ui.add_sized(
                [175.0, 24.0],
                egui::TextEdit::singleline(&mut cfg.weather_street).hint_text("如：岳麓区 · 麓山南路街道"),
            );
            let locating = STREET_LOCATING.load(Ordering::Relaxed);
            if ui
                .button(if locating { "正在定位…" } else { "自动定位到当前街道" })
                .on_hover_text("通过公网/网络位置 + OpenStreetMap Nominatim 18 级逆地理编码定位到当前所在区县与街道")
                .clicked()
            {
                locate_street_async();
                ui.ctx().request_repaint();
            }
            if ui
                .button("检索街道坐标")
                .on_hover_text("根据上方填写的城市与街道名称解析精确经纬度并同步天气")
                .clicked()
            {
                config::update(|c| {
                    c.weather_city = cfg.weather_city.clone();
                    c.weather_street = cfg.weather_street.clone();
                });
                before = config::get();
                let q = format!("{} {}", cfg.weather_city, cfg.weather_street);
                search_street_async(q);
                preset_switched = true;
                ui.ctx().request_repaint();
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("街道经纬度：").size(11.0).color(dim_text(ui)));
            ui.label(RichText::new("纬度 Lat").size(11.0));
            ui.add(egui::DragValue::new(&mut cfg.weather_lat).range(-90.0..=90.0).speed(0.001).fixed_decimals(4));
            ui.label(RichText::new("经度 Lon").size(11.0));
            ui.add(egui::DragValue::new(&mut cfg.weather_lon).range(-180.0..=180.0).speed(0.001).fixed_decimals(4));
            ui.label(RichText::new("实况").size(11.0));
            ui.add_sized([64.0, 22.0], egui::TextEdit::singleline(&mut cfg.weather_desc));
            ui.add(egui::DragValue::new(&mut cfg.weather_temp).range(-40..=50).suffix("°C"));
            let fetching = WEATHER_FETCHING.load(Ordering::Relaxed);
            if ui.button(if fetching { "↻ 同步中…" } else { "立即刷新天气" }).clicked() {
                apply_street_selection(&cfg.weather_city, &cfg.weather_street, cfg.weather_lat, cfg.weather_lon);
                cfg = config::get();
                before = cfg.clone();
                preset_switched = true;
                ui.ctx().request_repaint();
            }
        });

        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("热门街道预设秒切：").size(10.5).color(dim_text(ui)));
            for &(c_name, st_name, la, lo) in &STREET_PRESETS {
                let active = cfg.weather_city == c_name && cfg.weather_street == st_name;
                let tag = format!("{}·{}", c_name.trim_end_matches('市'), st_name.split('·').next_back().unwrap_or(st_name).trim());
                if ui.selectable_label(active, RichText::new(tag).size(10.5)).clicked() {
                    apply_street_selection(c_name, st_name, la, lo);
                    cfg = config::get();
                    before = cfg.clone();
                    preset_switched = true;
                    ui.ctx().request_repaint();
                }
            }
        });
        ui.label(RichText::new(&w_st.locate_status).size(10.5).color(Color32::from_rgb(56, 189, 248)));
    });

    // 3) 天气源管理与自定义接入天气源（与图三设备胶囊视觉方案统一的 Fluent 双列选卡）
    settings_label(ui, "天气数据源与自定义接入");
    crate::extras::settings_card(ui, |ui| {
        let avail_w = crate::extras::layout_avail_w(ui).max(160.0);
        let dark = ui.ctx().style().visuals.dark_mode;
        let gap = 10.0_f32;
        let per_row: usize = if avail_w >= 400.0 { 2 } else { 1 };
        let card_w = if per_row == 2 {
            ((avail_w - gap) * 0.5).floor().max(160.0)
        } else {
            avail_w
        };
        let card_h = 62.0_f32;
        let rows = (WEATHER_SOURCES.len() + per_row - 1) / per_row;
        let stage_h = rows as f32 * card_h + (rows.saturating_sub(1) as f32) * gap;
        let (stage_r, _) = ui.allocate_exact_size(egui::vec2(avail_w, stage_h), Sense::hover());
        let title_col = if dark { Color32::WHITE } else { Color32::from_rgb(24, 26, 32) };
        let sub_col = if dark { Color32::from_gray(165) } else { Color32::from_gray(95) };
        let hint_col = if dark { Color32::from_gray(160) } else { Color32::from_gray(105) };

        for (idx, &(src_id, src_title, src_desc)) in WEATHER_SOURCES.iter().enumerate() {
            let col = idx % per_row;
            let row = idx / per_row;
            let x = stage_r.left() + col as f32 * (card_w + gap);
            let y = stage_r.top() + row as f32 * (card_h + gap);
            let rect = Rect::from_min_size(Pos2::new(x, y), egui::vec2(card_w, card_h));
            let resp = ui.interact(rect, ui.make_persistent_id(("weather-src-card", src_id)), Sense::click());
            let selected = cfg.weather_source == src_id;
            if resp.clicked() {
                cfg.weather_source = src_id;
                config::update(|c| c.weather_source = src_id);
                if let Ok(mut g) = WISLAND_WEATHER.get_or_init(|| Mutex::new(WislandWeatherState::default())).lock() {
                    g.source_name = active_weather_source_info().0;
                }
                refresh_weather_async(cfg.weather_city.clone());
            }
            let hovered = resp.hovered();
            let bg = if dark {
                if selected {
                    Color32::from_rgb(0x2B, 0x33, 0x42)
                } else if hovered {
                    Color32::from_rgb(0x2B, 0x2C, 0x31)
                } else {
                    Color32::from_rgb(0x23, 0x24, 0x28)
                }
            } else if selected {
                Color32::from_rgb(0xEA, 0xF1, 0xFD)
            } else if hovered {
                Color32::from_rgb(0xF5, 0xF6, 0xFA)
            } else {
                Color32::from_rgb(0xFA, 0xFB, 0xFE)
            };
            let stroke_col = if selected {
                acc
            } else if dark {
                if hovered { Color32::from_gray(96) } else { Color32::from_rgb(0x36, 0x37, 0x3F) }
            } else if hovered {
                Color32::from_rgb(180, 188, 204)
            } else {
                Color32::from_rgb(220, 224, 232)
            };
            ui.painter().rect(
                rect,
                8.0,
                bg,
                Stroke::new(if selected { 1.5 } else { 1.0 }, stroke_col),
                egui::StrokeKind::Inside,
            );

            let tag_w = 46.0_f32;
            let tag_r = Rect::from_min_size(Pos2::new(rect.right() - tag_w - 10.0, rect.top() + 9.0), egui::vec2(tag_w, 17.0));
            let tag_bg = if selected {
                acc
            } else if dark {
                Color32::from_rgb(0x33, 0x35, 0x3C)
            } else {
                Color32::from_rgb(0xE8, 0xEB, 0xF2)
            };
            ui.painter().rect_filled(tag_r, 8.5, tag_bg);
            ui.painter().text(
                tag_r.center(),
                egui::Align2::CENTER_CENTER,
                if selected { "当前" } else { "切换" },
                egui::FontId::proportional(9.2),
                if selected { Color32::WHITE } else { hint_col },
            );

            let title_clip = Rect::from_min_max(rect.min, Pos2::new(tag_r.left() - 6.0, rect.bottom()));
            ui.painter().with_clip_rect(title_clip).text(
                Pos2::new(rect.left() + 12.0, rect.top() + 18.0),
                egui::Align2::LEFT_CENTER,
                src_title,
                egui::FontId::proportional(11.8),
                title_col,
            );
            ui.painter().with_clip_rect(rect.shrink2(egui::vec2(10.0, 0.0))).text(
                Pos2::new(rect.left() + 12.0, rect.bottom() - 15.0),
                egui::Align2::LEFT_CENTER,
                src_desc,
                egui::FontId::proportional(10.0),
                sub_col,
            );
        }

        if cfg.weather_source == 3 || cfg.weather_source == 4 {
            crate::extras::twinkle_card_break(ui);
            ui.label(RichText::new(if cfg.weather_source == 3 { "彩云天气 Token 配置" } else { "自定义天气接口配置" }).size(11.5).strong());
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(avail_w);
                if cfg.weather_source == 4 {
                    ui.label(RichText::new("数据源名称：").size(11.0));
                    ui.add_sized([110.0, 24.0], egui::TextEdit::singleline(&mut cfg.weather_custom_name).hint_text("如：自建气象源"));
                }
                ui.label(RichText::new("API Key / Token：").size(11.0));
                ui.add_sized(
                    [200.0_f32.min((avail_w - 110.0).max(96.0)), 24.0],
                    egui::TextEdit::singleline(&mut cfg.weather_api_key)
                        .password(true)
                        .hint_text("填写接口密钥 / Token"),
                );
            });
            if cfg.weather_source == 4 {
                ui.horizontal_wrapped(|ui| {
                    ui.set_max_width(avail_w);
                    ui.label(RichText::new("接口 URL 模板：").size(11.0));
                    let w = (avail_w - 220.0).max(110.0);
                    ui.add_sized(
                        [w, 24.0],
                        egui::TextEdit::singleline(&mut cfg.weather_custom_url)
                            .hint_text("https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}&current=temperature_2m"),
                    );
                    if ui.button("⚡ 测试并接入").clicked() {
                        cfg.weather_source = 4;
                        let city = cfg.weather_city.clone();
                        config::update(|c| *c = cfg.clone());
                        refresh_weather_async(city);
                    }
                });
                ui.label(
                    RichText::new("支持占位符：{lat} 街道纬度 · {lon} 街道经度 · {city} 城市 · {street} 街道名 · {key} API密钥")
                        .size(10.0)
                        .color(dim_text(ui)),
                );
            }
        }
    });

    // 4) 显示信息自由勾选
    settings_label(ui, "显示信息模块开关");
    crate::extras::settings_card(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut cfg.weather_show_hourly, "24小时逐时温度趋势曲线");
            ui.checkbox(&mut cfg.weather_show_daily, "未来 5 日温差胶囊条预报");
            ui.checkbox(&mut cfg.weather_show_aqi, "空气质量 AQI 与 PM2.5");
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut cfg.weather_show_wind, "风向风速与大气压");
            ui.checkbox(&mut cfg.weather_show_uv_hum, "☀ 紫外线指数与相对湿度");
            ui.checkbox(&mut cfg.weather_show_sun, "日出日落时间与生活提示");
        });

    });

    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

// ---------------------------------------------------------------
// 5. 独立「翻译」设置页 (`settings_tab == 19`) — 支持「暂时冻结岛体」与快捷互译工作台
// ---------------------------------------------------------------

pub fn draw_translate_settings_page(ui: &mut egui::Ui) {
    let mut cfg = config::get();
    let before = cfg.clone();

    ui.label(
        RichText::new("管理岛上「翻译」页面，支持「暂时冻结岛体（鼠标移开不自动收回）」、剪贴板自动识别与中/英/日互译。")
            .size(11.5)
            .color(dim_text(ui)),
    );

    settings_label(ui, "岛内「翻译」页面实时预览");
    crate::extras::settings_card(ui, |ui| {
        crate::extras::setting_row(
            ui,
            "在灵动岛启用独立「翻译」页面",
            "开启后可在灵动岛直接切换到「翻译」页面一键翻译剪贴板内容",
            &mut cfg.translate_page_visible,
        );
        crate::extras::twinkle_card_break(ui);
        crate::extras::setting_row(
            ui,
            "在翻译页面暂时冻结岛体（不自动收回）",
            "开启后当灵动岛处于「翻译」页面时保持展开状态，鼠标移开也不会收回岛体（也可直接点击岛内右上角「冻结岛体」按钮随时切换）",
            &mut cfg.translate_pin_island,
        );
        crate::extras::twinkle_card_break(ui);
        let avail_w = (crate::extras::layout_avail_w(ui) - 8.0).max(180.0);
        let scale_k = ((avail_w - 8.0) / 419.0).clamp(0.46, 1.0);
        let draw_w = 419.0 * scale_k;
        let draw_h = 278.0 * scale_k;
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(avail_w, draw_h + 10.0), Sense::hover());
            let island_r = Rect::from_center_size(rect.center(), egui::vec2(draw_w, draw_h));
            let preview_bg = active_win_fill(235).unwrap_or_else(|| {
                if ui.visuals().dark_mode {
                    Color32::from_rgb(32, 32, 32)
                } else {
                    Color32::from_rgb(43, 43, 43)
                }
            });
            ui.painter().rect_filled(island_r, 22.0 * scale_k, preview_bg);
            ui.painter().rect_stroke(island_r, 22.0 * scale_k, Stroke::new(1.0, Color32::from_white_alpha(38)), egui::StrokeKind::Inside);
            let mut pcard = crate::paint::Card::new(ui, island_r.min, island_r).scaled(0.0, 0.0, scale_k);
            pcard.live = true;
            if cfg.show_island_page_title {
                pcard.text(38.0, 22.5, "快捷", 15.0, crate::theme::W::Bold, Color32::WHITE, crate::paint::H::Left);
                pcard.text(70.0, 22.5, "翻译", 15.0, crate::theme::W::Bold, Color32::from_rgb(52, 211, 153), crate::paint::H::Left);
            }
            pcard.text(209.5, 22.0, "11月14日 周六  12:00", 11.2, crate::theme::W::Semibold, Color32::from_gray(220), crate::paint::H::Center);
            crate::paint::gear(&pcard, 394.0, 18.0, Color32::from_gray(175));
            let body_card = pcard.shifted(0.0, 22.0);
            draw_translate_island_page(&body_card);
        });
    });

    settings_label(ui, "翻译引擎与 AI 大模型 API 配置（参考 NEKOparapa/AiNiee）");
    crate::extras::settings_card(ui, |ui| {
        crate::extras::settings_control_row_right(ui, "主翻译引擎", |ui| {
            crate::extras::settings_segmented(
                ui,
                &mut cfg.translate_engine,
                &[
                    (0u8, "AI 大模型 (AiNiee)"),
                    (1u8, "Google GTX"),
                    (2u8, "MyMemory"),
                    (3u8, "离线词典"),
                ],
                82.0,
            );
        });
        if cfg.translate_engine == 0 {
            crate::extras::twinkle_card_break(ui);
            ui.label(
                RichText::new("AiNiee 风格 AI 大模型翻译接口（支持 DeepSeek / OpenAI / 硅基流动 / Gemini / Claude / 本地 Ollama）")
                    .size(12.0)
                    .strong()
                    .color(Color32::from_rgb(52, 211, 153)),
            );
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("服务商预设：").size(11.5));
                for (p_idx, p_name) in [
                    (0u8, "DeepSeek"),
                    (1u8, "OpenAI"),
                    (2u8, "硅基流动"),
                    (3u8, "Gemini"),
                    (4u8, "Claude"),
                    (5u8, "本地 Ollama"),
                    (6u8, "自定义接口"),
                ] {
                    let active = cfg.translate_ai_provider == p_idx;
                    if ui.selectable_label(active, RichText::new(p_name).size(11.0)).clicked() {
                        cfg.translate_ai_provider = p_idx;
                        if p_idx < 6 {
                            let (_, def_url, def_model) = ai_provider_preset(p_idx);
                            cfg.translate_ai_base_url = def_url.to_owned();
                            cfg.translate_ai_model = def_model.to_owned();
                        }
                    }
                }
            });
            let row_w = (crate::extras::layout_avail_w(ui) - 92.0).max(120.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("接口地址：").size(11.5));
                ui.add_sized(
                    [row_w, 24.0],
                    egui::TextEdit::singleline(&mut cfg.translate_ai_base_url)
                        .hint_text("https://api.deepseek.com/v1/chat/completions"),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.set_max_width(crate::extras::layout_avail_w(ui));
                ui.label(RichText::new("API Key ：").size(11.5));
                ui.add_sized(
                    [(row_w - 150.0).max(90.0), 24.0],
                    egui::TextEdit::singleline(&mut cfg.translate_ai_api_key)
                        .password(true)
                        .hint_text(if cfg.translate_ai_provider == 5 { "本地 Ollama 无需填写 Key" } else { "sk-...（留空时自动由 Google GTX 保底）" }),
                );
                ui.label(RichText::new("模型：").size(11.5));
                ui.add_sized(
                    [105.0, 24.0],
                    egui::TextEdit::singleline(&mut cfg.translate_ai_model)
                        .hint_text("deepseek-chat"),
                );
            });
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("AiNiee 译风预设：").size(11.0).color(dim_text(ui)));
                for (style_name, prompt_tpl) in [
                    ("精准直译", "你是一个专业、精准的 AI 翻译引擎（AiNiee 模式）。请直接输出准确、自然流畅的译文，不要附加任何解释或额外引号。"),
                    ("自然信达雅", "你是一位资深文学与本地化译者（AiNiee 信达雅模式）。请在忠于原意的基础上润色为地道优雅的母语表达，只输出译文。"),
                    ("学术与技术", "你是一位计算机与理工科学术翻译专家。请保留专业术语准确性与严谨学术语态，直接输出译文。"),
                    ("ACG/游戏本地化", "你是 AiNiee 游戏与轻小说本地化翻译官。请贴合角色语境与自然口语感进行翻译，只输出译文正文。"),
                ] {
                    if ui.small_button(style_name).clicked() {
                        cfg.translate_ai_prompt = prompt_tpl.to_owned();
                    }
                }
            });
            ui.add_sized(
                [crate::extras::layout_avail_w(ui).max(160.0), 24.0],
                egui::TextEdit::singleline(&mut cfg.translate_ai_prompt)
                    .hint_text("输入自定义翻译 System Prompt…"),
            );
        }
        crate::extras::twinkle_card_break(ui);
        crate::extras::setting_row(
            ui,
            "打开翻译页时自动读取剪贴板",
            "切换到岛上「翻译」页面或点击读剪贴板时，自动抓取最近复制的外文/中文句子并立即翻译",
            &mut cfg.translate_auto_clipboard,
        );
        crate::extras::twinkle_card_break(ui);
        crate::extras::setting_row(
            ui,
            "翻译完成后自动复制译文",
            "开启后每次翻译完成自动将译文写入系统剪贴板，可直接 Ctrl+V 粘贴",
            &mut cfg.translate_auto_copy,
        );
    });

    settings_label(ui, "双语快捷翻译工作台");
    crate::extras::settings_card(ui, |ui| {
        let (mut tr_in, tr_out, mut tr_mode, tr_status) = {
            let g = translate_state().lock().unwrap();
            (g.input.clone(), g.output.clone(), g.target_mode, g.status.clone())
        };
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("目标语言方向：").size(11.5).strong());
            for (m, label) in [
                (0u8, "中↔英 自动识别"),
                (1u8, "→ 译为中文 (zh-CN)"),
                (2u8, "→ 译为英文 (EN)"),
                (3u8, "→ 译为日文 (JA)"),
            ] {
                if ui.selectable_label(tr_mode == m, RichText::new(label).size(11.0)).clicked() {
                    tr_mode = m;
                    if let Ok(mut g) = translate_state().lock() {
                        g.target_mode = m;
                    }
                    trigger_translate_async(tr_in.clone(), m);
                }
            }
        });
        ui.horizontal_wrapped(|ui| {
            if ui.button("读取剪贴板并翻译").clicked() {
                if let Some(clip) = read_win32_clipboard_text() {
                    tr_in = clip.clone();
                    trigger_translate_async(clip, tr_mode);
                }
            }
            if ui.button("立即翻译").clicked() {
                trigger_translate_async(tr_in.clone(), tr_mode);
            }
            if !tr_out.is_empty() && ui.button("复制译文").clicked() {
                ui.ctx().copy_text(tr_out.clone());
            }
        });
        let edit_w = (crate::extras::layout_avail_w(ui) - 8.0).max(160.0);
        if ui
            .add_sized(
                [edit_w, 28.0],
                egui::TextEdit::singleline(&mut tr_in).hint_text("输入任意中文/英文/日文句子，点击「立即翻译」…"),
            )
            .changed()
        {
            if let Ok(mut g) = translate_state().lock() {
                g.input = tr_in.clone();
            }
        }
        if !tr_out.is_empty() {
            let inner_w = (crate::extras::layout_avail_w(ui) - 26.0).max(120.0);
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(16, 185, 129, 22))
                .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(16, 185, 129, 72)))
                .corner_radius(8.0)
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.set_width(inner_w);
                    ui.label(RichText::new(format!("译文：{tr_out}")).size(12.5).strong());
                    ui.label(RichText::new(tr_status).size(10.5).color(dim_text(ui)));
                });
        }
    });

    if cfg != before {
        config::update(|c| *c = cfg);
    }
}

#[allow(dead_code)]
fn bar_y(tile_y: f32) -> f32 {
    tile_y + 42.0
}
