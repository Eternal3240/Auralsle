//! 课程表：读取 `%APPDATA%\Auralsle\courses.json`，按周次展开成日程事件。
//!
//! 只读配置、只产数据：日程页把课程当成一种「课表」来源并入列表与月历圆点，
//! 课前提醒沿用上游本机日程那套（到点弹提示、按 key 去重），不改上游任何状态。
//!
//! 课表数据保存在 `courses.json`：可在设置页手动编辑、从 Excel 识别导入或粘贴文本批量添加。
//! 字段含义见 README「课程表」一节。

use crate::auralsle_ai_status::course_import;
use crate::{calendar_ui::Source, mail::Meeting};
use chrono::{Datelike, Duration as ChronoDuration, Local, NaiveDate, NaiveTime};
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration as StdDuration, Instant, SystemTime},
};

/// 一条上课记录：星期几、第几节到第几节、哪些周上。
struct Course {
    name: String,
    teacher: String,
    room: String,
    /// 1 = 周一 … 7 = 周日。
    weekday: u32,
    /// 起止节次（含）。
    from: u32,
    to: u32,
    /// 周次区间，含端点；可有多段。
    weeks: Vec<(u32, u32)>,
    /// 0 = 每周，1 = 单周，2 = 双周。
    parity: u8,
}

struct Plan {
    /// 第 1 周周一。
    start: NaiveDate,
    /// 第 1 节 … 第 13 节的起止时间。
    periods: Vec<(NaiveTime, NaiveTime)>,
    courses: Vec<Course>,
}

struct PlanCache {
    plan: Option<Arc<Plan>>,
    mtime: Option<SystemTime>,
    checked: Instant,
}

fn cache() -> &'static Mutex<PlanCache> {
    static CACHE: OnceLock<Mutex<PlanCache>> = OnceLock::new();
    CACHE.get_or_init(|| {
        Mutex::new(PlanCache { plan: None, mtime: None, checked: Instant::now() - StdDuration::from_secs(60) })
    })
}

/// 恢复备份后清空内存课表缓存，强制下次读取重新从磁盘加载。
pub fn reload_from_disk() {
    if let Ok(mut g) = cache().lock() {
        g.plan = None;
        g.mtime = None;
        g.checked = Instant::now() - StdDuration::from_secs(60);
    }
    if let Ok(mut g) = day_cache().lock() {
        g.clear();
    }
    if let Some(m) = DUE_CACHE.get() {
        if let Ok(mut g) = m.lock() {
            g.0 = i64::MIN;
        }
    }
    if let Some(m) = ALERT_CACHE.get() {
        if let Ok(mut g) = m.lock() {
            g.0 = i64::MIN;
        }
    }
    if let Ok(mut g) = rows_cache().lock() {
        g.0 = Instant::now() - StdDuration::from_secs(60);
        g.1.clear();
    }
}

/// 课表文件路径；配置里填了就用配置的。
pub fn file_path() -> PathBuf {
    let cfg = crate::auralsle_ai_status::config::get();
    if !cfg.course_file.trim().is_empty() {
        return PathBuf::from(cfg.course_file.trim());
    }
    crate::model::config_dir().join("courses.json")
}

fn parse_time(text: &str) -> Option<NaiveTime> {
    NaiveTime::parse_from_str(text.trim(), "%H:%M").ok()
}

fn parse_date(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

/// `1-16` / `1-16单` / `3,5-7` / `9` → 周次区间 + 单双周标记。
fn parse_weeks(text: &str) -> (Vec<(u32, u32)>, u8) {
    let mut out = Vec::new();
    let mut parity = 0u8;
    for part in text.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (body, par) = if let Some(rest) = part.strip_suffix('单') {
            (rest, 1)
        } else if let Some(rest) = part.strip_suffix('双') {
            (rest, 2)
        } else {
            (part, 0)
        };
        if par > 0 {
            parity = par;
        }
        match body.split_once('-').or_else(|| body.split_once('~')) {
            Some((a, b)) => {
                if let (Ok(a), Ok(b)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                    out.push((a.min(b), a.max(b)));
                }
            }
            None => {
                if let Ok(a) = body.trim().parse::<u32>() {
                    out.push((a, a));
                }
            }
        }
    }
    (out, parity)
}

fn parse_plan(text: &str) -> Option<Plan> {
    let v: Value = serde_json::from_str(text).ok()?;
    let raw_start_str = v.get("semester_start").and_then(Value::as_str).unwrap_or("2026-09-07");
    let effective_start_str = if !cfg!(test) {
        let cfg_s = crate::auralsle_ai_status::config::get().course_start;
        if raw_start_str == "2026-09-21" || raw_start_str >= "2026-11-01" {
            if !cfg_s.trim().is_empty() && cfg_s.trim() != "2026-09-21" {
                cfg_s
            } else {
                "2026-09-07".to_owned()
            }
        } else {
            raw_start_str.to_owned()
        }
    } else {
        raw_start_str.to_owned()
    };
    let start = parse_date(&effective_start_str)?;
    let mut periods = Vec::new();
    if let Some(list) = v.get("periods").and_then(Value::as_array) {
        // 按下标取用时要求从第 1 节排起，缺节补空占位。
        for item in list {
            let s = item.get("start").and_then(Value::as_str).and_then(parse_time);
            let e = item.get("end").and_then(Value::as_str).and_then(parse_time);
            periods.push(match (s, e) {
                (Some(s), Some(e)) => (s, e),
                _ => (NaiveTime::from_hms_opt(0, 0, 0)?, NaiveTime::from_hms_opt(0, 0, 0)?),
            });
        }
    }
    let mut courses = Vec::new();
    for item in v.get("courses").and_then(Value::as_array)? {
        let name = item.get("name").and_then(Value::as_str).unwrap_or("").trim();
        if name.is_empty() || course_import::is_bogus_course_name(name) {
            continue;
        }
        let (weeks, parity) = parse_weeks(item.get("weeks").and_then(Value::as_str).unwrap_or("1-20"));
        courses.push(Course {
            name: name.to_owned(),
            teacher: item.get("teacher").and_then(Value::as_str).unwrap_or("").trim().to_owned(),
            room: item.get("room").and_then(Value::as_str).unwrap_or("").trim().to_owned(),
            weekday: item.get("weekday").and_then(Value::as_u64).unwrap_or(1) as u32,
            from: item.get("from").and_then(Value::as_u64).unwrap_or(1) as u32,
            to: item.get("to").and_then(Value::as_u64).unwrap_or(2) as u32,
            weeks,
            parity,
        });
    }
    Some(Plan { start, periods, courses })
}

/// 取缓存的课表；文件改了（按 mtime）2 秒内重新加载。
fn plan() -> Option<Arc<Plan>> {
    // 单元测试不读本机课表：提示窗口随真实时间开关，会把像素对比测试搞成非确定。
    if cfg!(test) {
        return None;
    }
    if !crate::auralsle_ai_status::config::get().course_enabled {
        return None;
    }
    let path = file_path();
    let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
    let mut c = cache().lock().ok()?;
    if c.checked.elapsed() < StdDuration::from_secs(2) {
        return c.plan.clone();
    }
    c.checked = Instant::now();
    if c.plan.is_none() || mtime != c.mtime {
        c.plan = fs::read_to_string(&path).ok().and_then(|text| parse_plan(&text)).map(Arc::new);
        c.mtime = mtime;
        // 课表换了，按天缓存的事件一并作废。
        if let Ok(mut days) = day_cache().lock() {
            days.clear();
        }
    }
    c.plan.clone()
}

/// 按天生成的事件缓存：`&'static Meeting` 供日历列表引用。
fn day_cache() -> &'static Mutex<HashMap<i64, &'static [Meeting]>> {
    static DAYS: OnceLock<Mutex<HashMap<i64, &'static [Meeting]>>> = OnceLock::new();
    DAYS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn week_of(plan: &Plan, day: NaiveDate) -> Option<u32> {
    let days = (day - plan.start).num_days();
    if days < 0 {
        return None;
    }
    Some(days as u32 / 7 + 1)
}

fn hits(course: &Course, week: u32) -> bool {
    let inside = course.weeks.iter().any(|(a, b)| (*a..=*b).contains(&week));
    match course.parity {
        1 => inside && week % 2 == 1,
        2 => inside && week % 2 == 0,
        _ => inside,
    }
}

fn meeting(plan: &Plan, course: &Course, day: NaiveDate) -> Option<Meeting> {
    let (start_time, _) = *plan.periods.get(course.from.checked_sub(1)? as usize)?;
    let (_, end_time) = *plan.periods.get(course.to.checked_sub(1)? as usize)?;
    let start = day.and_time(start_time).and_local_timezone(Local).single()?;
    let end = day.and_time(end_time).and_local_timezone(Local).single()?;
    // 悬浮提示的第二行：时间 · 教室 · 老师，提醒条与日程行都直接显示它。
    let mut at = start.format("%m-%d %H:%M").to_string();
    for part in [course.room.as_str(), course.teacher.as_str()] {
        if !part.is_empty() {
            at.push_str(" · ");
            at.push_str(part);
        }
    }
    Some(Meeting {
        all_day: false,
        title: course.name.clone(),
        start: start.timestamp(),
        end: Some(end.timestamp()),
        at,
        uid: format!("course-{}-{}", course.name, day.format("%Y%m%d")),
    })
}

/// 纯函数：把某一天的课展开成事件（按开始时间排序），便于单测与计时使用。
fn expand(plan: &Plan, day: NaiveDate, week: u32) -> Vec<Meeting> {
    let weekday = day.weekday().num_days_from_monday() + 1;
    let mut list: Vec<Meeting> = plan
        .courses
        .iter()
        .filter(|c| c.weekday == weekday && hits(c, week))
        .filter_map(|c| meeting(plan, c, day))
        .collect();
    list.sort_by_key(|m| m.start);
    list
}

/// 某一天要上的课（已按开始时间排序）。返回 `&'static Meeting` 以便并进日历列表。
pub(crate) fn entries(day: NaiveDate) -> Vec<(&'static Meeting, Source<'static>)> {
    let Some(plan) = plan() else { return Vec::new() };
    let Some(week) = week_of(&plan, day) else { return Vec::new() };
    let key = day.num_days_from_ce() as i64;
    if let Ok(days) = day_cache().lock() {
        if let Some(cached) = days.get(&key) {
            return cached.iter().map(|m| (m, Source::Course)).collect();
        }
    }
    let list = expand(&plan, day, week);
    if list.is_empty() {
        return Vec::new();
    }
    let leaked: &'static [Meeting] = Box::leak(list.into_boxed_slice());
    if let Ok(mut days) = day_cache().lock() {
        days.insert(key, leaked);
    }
    leaked.iter().map(|m| (m, Source::Course)).collect()
}

/// 一段日期内的课（月历圆点用）：逐天取缓存，避免重复展开。
pub(crate) fn entries_between(first: NaiveDate, last: NaiveDate) -> Vec<(&'static Meeting, Source<'static>)> {
    let mut out = Vec::new();
    let mut day = first;
    while day <= last {
        out.extend(entries(day));
        day += chrono::Duration::days(1);
    }
    out
}

/// 到点该提醒的课：`(去重 key, 事件)`。提前量取设置里的「课前提醒」分钟数。
static DUE_CACHE: OnceLock<Mutex<(i64, i64, Vec<(String, Meeting)>)>> = OnceLock::new();
static ALERT_CACHE: OnceLock<Mutex<(i64, i64, u8, Option<Alert>)>> = OnceLock::new();

pub fn due(now: i64) -> Vec<(String, Meeting)> {
    let lead = crate::auralsle_ai_status::config::get().course_remind_minutes.max(0) * 60;
    if !cfg!(test) {
        if let Ok(g) = DUE_CACHE.get_or_init(|| Mutex::new((i64::MIN, -1, Vec::new()))).lock() {
            if g.0 == now && g.1 == lead {
                return g.2.clone();
            }
        }
    }
    let today = Local::now().date_naive();
    let mut out = Vec::new();
    for offset in 0..2 {
        let day = today + chrono::Duration::days(offset);
        for (event, _) in entries(day) {
            let left = event.start - now;
            // 提前 lead 分钟开始提醒，开始后 1 分钟内仍可补提醒。
            if left <= lead && left > -60 {
                out.push((crate::calendar::dedupe_key(event), event.clone()));
            }
        }
    }
    if !cfg!(test) {
        if let Ok(mut g) = DUE_CACHE.get_or_init(|| Mutex::new((i64::MIN, -1, Vec::new()))).lock() {
            *g = (now, lead, out.clone());
        }
    }
    out
}

/// 当前第几周（按学期起始算；读不到课表时返回 1）。
pub fn week_now() -> u32 {
    let today = Local::now().date_naive();
    if let Some(p) = plan() {
        if let Some(w) = week_of(&p, today) {
            return w;
        }
    }
    let cfg_start = crate::auralsle_ai_status::config::get().course_start;
    let start_str = if cfg_start.trim().is_empty() || cfg_start.trim() == "2026-09-21" {
        "2026-09-07"
    } else {
        cfg_start.trim()
    };
    if let Some(st) = parse_date(start_str) {
        let days = (today - st).num_days();
        if days >= 0 {
            return (days as u32 / 7 + 1).clamp(1, 30);
        }
    }
    10
}

/// 自由设置「当前是第 N 周」：根据今天所在周自动反推第 1 周周一 (`semester_start`) 并写回 `courses.json` 与配置。
pub fn set_current_week_now(target_week: u32) -> Result<String, String> {
    let w = target_week.clamp(1, 30);
    let today = Local::now().date_naive();
    let monday_this_week = today - ChronoDuration::days(today.weekday().num_days_from_monday() as i64);
    let sem_start = monday_this_week - ChronoDuration::days((w as i64 - 1) * 7);
    let start_str = sem_start.format("%Y-%m-%d").to_string();
    crate::auralsle_ai_status::config::update(|c| {
        c.course_start = start_str.clone();
    });
    let mut root = read_root();
    root["semester_start"] = Value::String(start_str.clone());
    let _ = write_root(&root);
    invalidate();
    Ok(format!("已将当前周次校准为第 {w} 周（第 1 周周一：{start_str}）"))
}

/// 某条课程的 `weeks` 是否覆盖指定周（含单双周）。
pub fn week_applies(weeks: &str, week: u32) -> bool {
    let (ranges, parity) = parse_weeks(weeks);
    let inside = ranges.iter().any(|(a, b)| (*a..=*b).contains(&week));
    match parity {
        1 => inside && week % 2 == 1,
        2 => inside && week % 2 == 0,
        _ => inside,
    }
}

/// 设置页展示用：`(课程数, 第几周, 第 1 周周一)`。
pub fn summary() -> Option<(usize, u32, NaiveDate)> {
    let plan = plan()?;
    let week = week_of(&plan, Local::now().date_naive()).unwrap_or(0);
    Some((plan.courses.len(), week, plan.start))
}

/// 今天还没结束的下一节课：`(课程名, "14:30")`。
pub fn next_today(now: i64) -> Option<(String, String)> {
    let today = Local::now().date_naive();
    entries(today)
        .into_iter()
        .find(|(m, _)| m.end.unwrap_or(m.start) > now)
        .map(|(m, _)| {
            let at = chrono::DateTime::from_timestamp(m.start, 0)
                .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
                .unwrap_or_default();
            (m.title.clone(), at)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与 `data/courses.json` 同格式的小样本：周二 1-2 节（5-16 周）+ 周二 3-4 节（1-10 周）。
    fn sample() -> Plan {
        let text = r#"{
            "semester_start": "2026-09-07",
            "periods": [
                {"no":1,"start":"08:00","end":"08:45"},
                {"no":2,"start":"08:55","end":"09:40"},
                {"no":3,"start":"10:00","end":"10:45"},
                {"no":4,"start":"10:55","end":"11:40"}
            ],
            "courses": [
                {"name":"中职电子","teacher":"唐智彬","room":"306","weekday":2,"from":1,"to":2,"weeks":"5-16"},
                {"name":"单片机","teacher":"杨进宝","room":"402","weekday":2,"from":3,"to":4,"weeks":"1-10"}
            ]
        }"#;
        parse_plan(text).expect("样本应能解析")
    }

    #[test]
    fn week_counts_from_semester_start() {
        let p = sample();
        assert_eq!(week_of(&p, NaiveDate::from_ymd_opt(2026, 9, 7).unwrap()), Some(1));
        assert_eq!(week_of(&p, NaiveDate::from_ymd_opt(2026, 9, 13).unwrap()), Some(1));
        assert_eq!(week_of(&p, NaiveDate::from_ymd_opt(2026, 9, 14).unwrap()), Some(2));
        // 2026-11-10 是第 10 周周二
        assert_eq!(week_of(&p, NaiveDate::from_ymd_opt(2026, 11, 10).unwrap()), Some(10));
        assert_eq!(week_of(&p, NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()), None);
    }

    #[test]
    fn courses_expand_with_period_times_and_weeks() {
        let p = sample();
        let day = NaiveDate::from_ymd_opt(2026, 11, 10).unwrap();
        let list = expand(&p, day, 10);
        assert_eq!(list.len(), 2, "第 10 周两门课都该上");
        assert_eq!(list[0].title, "中职电子");
        assert!(list[0].at.contains("08:00") && list[0].at.contains("306") && list[0].at.contains("唐智彬"));
        let start = chrono::DateTime::from_timestamp(list[0].start, 0).unwrap().with_timezone(&Local);
        assert_eq!(start.format("%H:%M").to_string(), "08:00");
        let end = chrono::DateTime::from_timestamp(list[0].end.unwrap(), 0).unwrap().with_timezone(&Local);
        assert_eq!(end.format("%H:%M").to_string(), "09:40", "1-2 节应到第二节下课");

        // 第 13 周：只剩 5-16 周那门
        let late = expand(&p, NaiveDate::from_ymd_opt(2026, 12, 1).unwrap(), 13);
        assert_eq!(late.len(), 1);
        assert_eq!(late[0].title, "中职电子");

        // 第 1 周：只剩 1-10 周那门
        let early = expand(&p, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap(), 1);
        assert_eq!(early.len(), 1);
        assert_eq!(early[0].title, "单片机");

        // 周三没课
        assert!(expand(&p, NaiveDate::from_ymd_opt(2026, 11, 11).unwrap(), 10).is_empty());
    }

    #[test]
    fn odd_even_weeks_are_respected() {
        assert_eq!(parse_weeks("1-15单"), (vec![(1, 15)], 1));
        assert_eq!(parse_weeks("2-16双"), (vec![(2, 16)], 2));
        assert_eq!(parse_weeks("3,5-7"), (vec![(3, 3), (5, 7)], 0));
        let course = Course {
            name: "单周课".into(),
            teacher: String::new(),
            room: String::new(),
            weekday: 2,
            from: 1,
            to: 2,
            weeks: vec![(1, 15)],
            parity: 1,
        };
        assert!(hits(&course, 3));
        assert!(!hits(&course, 2));
        assert!(!hits(&course, 10), "偶数周不该上单周课");
    }
}

/// 岛左侧「日程卡片」需要的全部信息。
#[derive(Clone, Debug)]
pub struct Alert {
    pub title: String,
    /// "10:00"。
    pub start: String,
    /// "11:40"。
    pub end: String,
    /// "天工楼中栋402教室 · 杨进宝"。
    pub info: String,
    /// "今天 · 11月10日"。
    pub date: String,
    /// "进行中" / "即将开始"。
    pub status: &'static str,
    /// 0..1，课程已过去的比例。
    pub progress: f32,
    /// "08:00-08:40"：文字卡片首行的时间段。
    pub time_range: String,
    /// 上课地点（"电教202"）。
    pub place: String,
    /// 教师（可能为空）。
    pub teacher: String,
    /// "20分钟后上课" / "上课中 · 剩 35 分钟"。
    pub countdown: String,
    /// 上课 / 下课时间戳，给「免打扰到课间」用。
    pub start_ts: i64,
    pub end_ts: i64,
}

/// 当前（或即将）要上的课；没有落在提醒窗口内就返回 `None`。
///
/// 与 `alert_course` 同源，只是把结束时间、日期、状态、进度一并给出来，
/// 供岛左侧那张「日程卡片」直接排版。
pub fn alert_detail(now: i64) -> Option<Alert> {
    let cfg = crate::auralsle_ai_status::config::get();
    let lead = cfg.course_remind_minutes.max(0) * 60;
    let style = cfg.course_countdown_style;
    if !cfg!(test) {
        if let Ok(g) = ALERT_CACHE.get_or_init(|| Mutex::new((i64::MIN, -1, 255, None))).lock() {
            if g.0 == now && g.1 == lead && g.2 == style {
                return g.3.clone();
            }
        }
    }
    if lead <= 0 {
        return None;
    }
    let today = Local::now().date_naive();
    let hhmm = |ts: i64| {
        chrono::DateTime::from_timestamp(ts, 0)
            .map(|t| t.with_timezone(&Local).format("%H:%M").to_string())
            .unwrap_or_default()
    };
    for (m, _) in entries(today) {
        let end = m.end.unwrap_or(m.start);
        if m.start - now > lead || end <= now {
            continue;
        }
        let parts: Vec<&str> = m.at.split(" · ").collect();
        let place = parts.get(1).copied().unwrap_or("").to_owned();
        let teacher = parts.get(2).copied().unwrap_or("").to_owned();
        let info = parts.iter().skip(1).copied().collect::<Vec<_>>().join(" · ");
        let start_s = hhmm(m.start);
        let end_s = hhmm(end);
        let clock = |d: i64| format!("{:02}:{:02}:{:02}", d / 3600, (d % 3600) / 60, d % 60);
        let style = crate::auralsle_ai_status::config::get().course_countdown_style;
        let countdown = if now < m.start {
            let d = (m.start - now).max(0);
            match style {
                1 => clock(d),
                2 => format!("还有 {} 分钟", (d + 59) / 60),
                _ if d >= 3600 => format!("{}小时{}分后上课", d / 3600, (d % 3600) / 60),
                _ if d >= 60 => format!("{}分钟后上课", (d + 59) / 60),
                _ => "马上上课".to_owned(),
            }
        } else {
            let left = (end - now).max(0);
            match style {
                1 => clock(left),
                2 => format!("还剩 {} 分钟", (left + 59) / 60),
                _ if left >= 60 => format!("上课中 · 剩 {} 分钟", (left + 59) / 60),
                _ => "即将下课".to_owned(),
            }
        };
        let res = Some(Alert {
            title: m.title.clone(),
            start: start_s.clone(),
            end: end_s.clone(),
            info,
            date: format!("今天 · {}月{}日", today.month(), today.day()),
            status: if now >= m.start { "进行中" } else { "即将开始" },
            progress: if end > m.start {
                ((now - m.start) as f32 / (end - m.start) as f32).clamp(0.0, 1.0)
            } else {
                0.0
            },
            time_range: format!("{start_s}-{end_s}"),
            place,
            teacher,
            countdown,
            start_ts: m.start,
            end_ts: end,
        });
        if !cfg!(test) {
            if let Ok(mut g) = ALERT_CACHE.get_or_init(|| Mutex::new((i64::MIN, -1, 255, None))).lock() {
                *g = (now, lead, style, res.clone());
            }
        }
        return res;
    }
    if !cfg!(test) {
        if let Ok(mut g) = ALERT_CACHE.get_or_init(|| Mutex::new((i64::MIN, -1, 255, None))).lock() {
            *g = (now, lead, style, None);
        }
    }
    None
}

/// 岛上提示用：当前（或即将）要上的课：`(标题, "10:00", "306 · 唐智彬")`。
pub fn alert_course(now: i64) -> Option<(String, String, String)> {
    alert_detail(now).map(|a| (a.title, a.start, a.info))
}

/// 课表文件被外部改动后立刻作废缓存，下一帧就生效。
pub fn invalidate() {
    if let Ok(mut c) = cache().lock() {
        c.plan = None;
        c.mtime = None;
        c.checked = Instant::now() - StdDuration::from_secs(60);
    }
    if let Ok(mut days) = day_cache().lock() {
        days.clear();
    }
    if let Some(m) = DUE_CACHE.get() {
        if let Ok(mut g) = m.lock() { g.0 = i64::MIN; }
    }
    if let Some(m) = ALERT_CACHE.get() {
        if let Ok(mut g) = m.lock() { g.0 = i64::MIN; }
    }
}

/// 设置页「自定义导入」的结果提示。
pub fn import_status() -> &'static Mutex<String> {
    static S: OnceLock<Mutex<String>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(String::new()))
}

/// Excel 识别 / 导入的结果提示（与文本导入分开，两条消息互不覆盖）。
pub fn import_note() -> &'static Mutex<String> {
    static N: OnceLock<Mutex<String>> = OnceLock::new();
    N.get_or_init(|| Mutex::new(String::new()))
}

/// 设置页「自定义导入」的文本框内容。
pub fn import_buf() -> &'static Mutex<String> {
    static BUF: OnceLock<Mutex<String>> = OnceLock::new();
    BUF.get_or_init(|| Mutex::new(String::new()))
}

/// 解析一行：`课程名,星期(1-7),开始节,结束节,周次,教室,老师`（老师可省）。
fn parse_import_line(line: &str) -> Option<Value> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let normalized = line.replace('，', ",");
    let parts: Vec<&str> = normalized.split(',').map(str::trim).collect();
    if parts.len() < 5 {
        return None;
    }
    let name = parts[0];
    let weekday: u64 = parts[1].parse().ok()?;
    let from: u64 = parts[2].parse().ok()?;
    let to: u64 = parts[3].parse().ok()?;
    if !(1..=7).contains(&weekday) || from == 0 || to < from {
        return None;
    }
    let mut v = serde_json::Map::new();
    v.insert("name".into(), Value::String(name.to_owned()));
    v.insert("weekday".into(), Value::from(weekday));
    v.insert("from".into(), Value::from(from));
    v.insert("to".into(), Value::from(to));
    v.insert("weeks".into(), Value::String(parts[4].to_owned()));
    v.insert("room".into(), Value::String(parts.get(5).copied().unwrap_or("").to_owned()));
    v.insert("teacher".into(), Value::String(parts.get(6).copied().unwrap_or("").to_owned()));
    Some(Value::Object(v))
}

/// 把文本框里的课程追加进 `courses.json`，返回新增条数。
pub fn append_courses(text: &str) -> Result<usize, String> {
    let path = file_path();
    let mut root: Value = fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_else(|| {
            serde_json::json!({
                "semester_start": "2026-09-07".to_string(),
                "periods": [],
                "courses": [],
            })
        });
    let mut added = 0usize;
    let list = root
        .as_object_mut()
        .ok_or("课表文件结构不正确")?
        .entry("courses")
        .or_insert_with(|| Value::Array(vec![]));
    let arr = list.as_array_mut().ok_or("courses 不是数组")?;
    for line in text.lines() {
        if let Some(item) = parse_import_line(line) {
            arr.push(item);
            added += 1;
        }
    }
    if added == 0 {
        return Err("没有解析到课程。每行格式：课程名,星期(1-7),开始节,结束节,周次,教室,老师".into());
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(&root).unwrap_or_default()).map_err(|e| e.to_string())?;
    fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
    invalidate();
    Ok(added)
}

// ---------------------------------------------------------------
// 手动编辑课表
// ---------------------------------------------------------------

/// 编辑表单；`index = None` 表示新增。
#[derive(Clone, Debug)]
pub struct Editor {
    pub index: Option<usize>,
    pub name: String,
    pub weekday: u32,
    pub from: u32,
    pub to: u32,
    pub weeks: String,
    pub room: String,
    pub teacher: String,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            index: None,
            name: String::new(),
            weekday: 1,
            from: 1,
            to: 2,
            weeks: "1-16".into(),
            room: String::new(),
            teacher: String::new(),
        }
    }
}

/// 原始课程行（不按周展开），给下拉列表与表单回填用。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CourseRow {
    pub name: String,
    pub weekday: u32,
    pub from: u32,
    pub to: u32,
    pub weeks: String,
    pub room: String,
    pub teacher: String,
}

impl Editor {
    /// 从已有行回填表单。
    pub fn from_row(index: usize, row: &CourseRow) -> Self {
        Self {
            index: Some(index),
            name: row.name.clone(),
            weekday: row.weekday.clamp(1, 7),
            from: row.from.max(1),
            to: row.to.max(row.from.max(1)),
            weeks: if row.weeks.is_empty() { "1-16".into() } else { row.weeks.clone() },
            room: row.room.clone(),
            teacher: row.teacher.clone(),
        }
    }
}

/// 设置页正在编辑的表单（跨帧保留，切换 tab 不丢）。
pub fn editor() -> &'static Mutex<Editor> {
    static EDITOR: OnceLock<Mutex<Editor>> = OnceLock::new();
    EDITOR.get_or_init(|| Mutex::new(Editor::default()))
}

fn rows_cache() -> &'static Mutex<(Instant, Vec<CourseRow>)> {
    static ROWS: OnceLock<Mutex<(Instant, Vec<CourseRow>)>> = OnceLock::new();
    ROWS.get_or_init(|| Mutex::new((Instant::now() - StdDuration::from_secs(60), Vec::new())))
}

/// 自动清理旧版导入残留的脏课程行（如 `(5-16,天工楼中栋306教室)`、`18160086`、`正常`、`已选`）。
fn sanitize_raw_rows(raw: Vec<CourseRow>) -> (Vec<CourseRow>, bool) {
    let mut cleaned: Vec<CourseRow> = Vec::with_capacity(raw.len());
    let mut changed = false;
    for r in raw {
        let name = r.name.trim();
        // 如果是 `(5-16,天工楼中栋306教室)` 这种被旧版拆断的括号行，把它的周次和教室合并回上一条同格课程！
        if (name.starts_with('(') || name.starts_with('（')) && name.ends_with([')', '）']) {
            changed = true;
            let inner = name.trim_matches(['(', ')', '（', '）']);
            if let Some((w_part, room_part)) = inner.split_once([',', '，']) {
                if let Some(prev) = cleaned
                    .iter_mut()
                    .rev()
                    .find(|p| p.weekday == r.weekday && p.from == r.from)
                {
                    let w_clean = w_part.trim().trim_end_matches('周').trim();
                    if !w_clean.is_empty() {
                        prev.weeks = w_clean.to_owned();
                    }
                    if prev.room.is_empty() && !room_part.trim().is_empty() {
                        prev.room = room_part.trim().to_owned();
                    }
                }
            }
            continue;
        }
        if course_import::is_bogus_course_name(name) {
            changed = true;
            continue;
        }
        cleaned.push(r);
    }
    (cleaned, changed)
}

fn read_rows() -> Vec<CourseRow> {
    let path = file_path();
    let Ok(text) = fs::read_to_string(&path) else { return Vec::new() };
    let Ok(mut v) = serde_json::from_str::<Value>(&text) else { return Vec::new() };
    let raw: Vec<CourseRow> = v
        .get("courses")
        .and_then(Value::as_array)
        .map(|list| {
            list.iter()
                .map(|item| CourseRow {
                    name: item.get("name").and_then(Value::as_str).unwrap_or("").to_owned(),
                    weekday: item.get("weekday").and_then(Value::as_u64).unwrap_or(1) as u32,
                    from: item.get("from").and_then(Value::as_u64).unwrap_or(1) as u32,
                    to: item.get("to").and_then(Value::as_u64).unwrap_or(2) as u32,
                    weeks: item.get("weeks").and_then(Value::as_str).unwrap_or("1-16").to_owned(),
                    room: item.get("room").and_then(Value::as_str).unwrap_or("").to_owned(),
                    teacher: item.get("teacher").and_then(Value::as_str).unwrap_or("").to_owned(),
                })
                .collect()
        })
        .unwrap_or_default();

    let (cleaned, changed) = sanitize_raw_rows(raw);
    if changed && !cfg!(test) {
        // 如果本地配置了原 .xlsx 且文件存在，直接用新版解析器重导一遍；否则将清洗后的干净列表写回盘。
        let xlsx_path = crate::auralsle_ai_status::config::get().course_xlsx.trim().to_owned();
        if !xlsx_path.is_empty() && Path::new(&xlsx_path).is_file() {
            if let Ok(sheet) = crate::auralsle_ai_status::xlsx::read_sheet(Path::new(&xlsx_path)) {
                if let Ok(parsed) = course_import::parse(&sheet) {
                    let rebuilt: Vec<CourseRow> = parsed
                        .rows
                        .into_iter()
                        .map(|r| CourseRow {
                            name: r.name,
                            weekday: r.weekday,
                            from: r.from,
                            to: r.to,
                            weeks: r.weeks,
                            room: r.room,
                            teacher: r.teacher,
                        })
                        .collect();
                    v["periods"] = Value::Array(parsed.periods);
                    v["courses"] = Value::Array(
                        rebuilt
                            .iter()
                            .map(|r| {
                                json!({
                                    "name": r.name,
                                    "teacher": r.teacher,
                                    "room": r.room,
                                    "weekday": r.weekday,
                                    "from": r.from,
                                    "to": r.to,
                                    "weeks": r.weeks,
                                })
                            })
                            .collect(),
                    );
                    let _ = fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default());
                    invalidate();
                    return rebuilt;
                }
            }
        }
        v["courses"] = Value::Array(
            cleaned
                .iter()
                .map(|r| {
                    json!({
                        "name": r.name,
                        "teacher": r.teacher,
                        "room": r.room,
                        "weekday": r.weekday,
                        "from": r.from,
                        "to": r.to,
                        "weeks": r.weeks,
                    })
                })
                .collect(),
        );
        let _ = fs::write(&path, serde_json::to_string_pretty(&v).unwrap_or_default());
        invalidate();
    }
    cleaned
}

/// 原始课程列表（1 秒缓存，设置页每帧调用也不会反复读盘）。
/// 下标与 `courses.json` 的 `courses` 数组严格一致，保存 / 删除据此定位。
pub fn rows() -> Vec<CourseRow> {
    if let Ok(mut c) = rows_cache().lock() {
        if c.0.elapsed() < StdDuration::from_secs(1) {
            return c.1.clone();
        }
        c.1 = read_rows();
        c.0 = Instant::now();
        return c.1.clone();
    }
    read_rows()
}

fn bump_rows() {
    if let Ok(mut c) = rows_cache().lock() {
        c.0 = Instant::now() - StdDuration::from_secs(60);
    }
}

/// 没有任何 periods 时（纯手工建课）的兜底：湖南师大 13 节作息。
fn default_periods() -> Value {
    json!([
        {"no":1,"start":"08:00","end":"08:45"}, {"no":2,"start":"08:55","end":"09:40"},
        {"no":3,"start":"10:00","end":"10:45"}, {"no":4,"start":"10:55","end":"11:40"},
        {"no":5,"start":"12:45","end":"13:30"}, {"no":6,"start":"13:30","end":"14:15"},
        {"no":7,"start":"14:30","end":"15:15"}, {"no":8,"start":"15:25","end":"16:10"},
        {"no":9,"start":"16:30","end":"17:15"}, {"no":10,"start":"17:25","end":"18:10"},
        {"no":11,"start":"19:00","end":"19:45"}, {"no":12,"start":"19:55","end":"20:40"},
        {"no":13,"start":"20:50","end":"21:35"}
    ])
}

/// 一周总览左侧节次列要显示的每节课上下课时间（1..13 节）。
///
/// 优先读 courses.json 的 `periods`（编辑作息时间表时写回），
/// 缺失或不足时用内置作息补齐，保证 13 行都有时间可显示。
pub fn period_labels() -> Vec<(String, String)> {
    const FALLBACK: [(&str, &str); 13] = [
        ("08:00", "08:45"), ("08:55", "09:40"), ("10:00", "10:45"),
        ("10:55", "11:40"), ("12:45", "13:30"), ("13:30", "14:15"),
        ("14:30", "15:15"), ("15:25", "16:10"), ("16:30", "17:15"),
        ("17:25", "18:10"), ("19:00", "19:45"), ("19:55", "20:40"),
        ("20:50", "21:35"),
    ];
    let root = read_root();
    let mut out: Vec<(String, String)> = root["periods"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|p| {
                    (
                        p["start"].as_str().unwrap_or("").to_owned(),
                        p["end"].as_str().unwrap_or("").to_owned(),
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    for i in out.len()..FALLBACK.len() {
        out.push((FALLBACK[i].0.to_owned(), FALLBACK[i].1.to_owned()));
    }
    out.truncate(FALLBACK.len());
    out
}

/// 设置页「课表岛编辑预览」用的固定示例课：不受当前时间与提醒窗口影响。
pub fn preview_alert() -> Alert {
    Alert {
        title: "高等数学".to_owned(),
        start: "10:00".to_owned(),
        end: "11:40".to_owned(),
        info: "天工楼中栋402教室 · 杨进宝".to_owned(),
        date: "今天 · 11月10日".to_owned(),
        status: "即将开始",
        progress: 0.32,
        time_range: "10:00-11:40".to_owned(),
        place: "天工楼中栋402".to_owned(),
        teacher: "杨进宝".to_owned(),
        countdown: "20分钟后上课".to_owned(),
        start_ts: 0,
        end_ts: 0,
    }
}

fn read_root() -> Value {
    fs::read_to_string(file_path())
        .ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .unwrap_or_else(|| {
            json!({
                "semester_start": "2026-09-07".to_string(),
                "periods": default_periods(),
                "courses": [],
            })
        })
}

fn write_root(root: &Value) -> Result<(), String> {
    let path = file_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_string_pretty(root).unwrap_or_default())
        .map_err(|e| format!("写入课表失败：{e}"))?;
    fs::rename(&tmp, &path).map_err(|e| format!("替换课表失败：{e}"))?;
    invalidate();
    bump_rows();
    Ok(())
}

/// 保存当前表单：有 `index` 就改那一条，否则追加。返回给用户看的结果文案。
pub fn save_editor() -> Result<String, String> {
    let e = editor().lock().map(|g| g.clone()).unwrap_or_default();
    let name = e.name.trim().to_owned();
    if name.is_empty() {
        return Err("课程名不能为空".into());
    }
    if !(1..=7).contains(&e.weekday) {
        return Err("星期要在 1-7 之间".into());
    }
    if e.from == 0 || e.to < e.from {
        return Err("节次不对：结束节要 ≥ 开始节".into());
    }
    let weeks = if e.weeks.trim().is_empty() { "1-16" } else { e.weeks.trim() }.to_owned();
    let mut root = read_root();
    if root
        .get("periods")
        .and_then(Value::as_array)
        .is_none_or(|a| a.is_empty())
    {
        root["periods"] = default_periods();
    }
    let obj = root.as_object_mut().ok_or("课表文件结构不正确")?;
    let arr = obj.entry("courses").or_insert_with(|| Value::Array(vec![]));
    let arr = arr.as_array_mut().ok_or("courses 不是数组")?;
    let item = json!({
        "name": name,
        "teacher": e.teacher.trim(),
        "room": e.room.trim(),
        "weekday": e.weekday,
        "from": e.from,
        "to": e.to,
        "weeks": weeks,
    });
    let updated = match e.index {
        Some(i) if i < arr.len() => {
            arr[i] = item;
            true
        }
        _ => {
            arr.push(item);
            false
        }
    };
    write_root(&root)?;
    Ok(if updated {
        "已保存修改，日程与岛上卡片立刻生效".into()
    } else {
        "已新增课程，日程里立刻可见".into()
    })
}

/// 只改学期第 1 周周一（`YYYY-MM-DD`），不动课程。
pub fn set_semester_start(text: &str) -> Result<(), String> {
    let text = text.trim();
    if parse_date(text).is_none() {
        return Err("日期格式应为 YYYY-MM-DD".to_owned());
    }
    let mut root = read_root();
    root["semester_start"] = Value::String(text.to_owned());
    write_root(&root)
}

/// 删除第 `index` 条课程。
pub fn delete_row(index: usize) -> Result<String, String> {
    let mut root = read_root();
    let Some(arr) = root.get_mut("courses").and_then(Value::as_array_mut) else {
        return Err("课表里没有课程".into());
    };
    if index >= arr.len() {
        return Err("课程已不存在，请重新选择".into());
    }
    let name = arr[index]
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("课程")
        .to_owned();
    arr.remove(index);
    write_root(&root)?;
    Ok(format!("已删除「{name}」"))
}

/// [ShiGuangSchedule] 保存自定义 13 节作息时间表（每节起止 HH:MM）。
pub fn save_periods(periods: &[(String, String)]) -> Result<String, String> {
    let mut root = read_root();
    let mut arr = Vec::with_capacity(periods.len());
    for (idx, (s, e)) in periods.iter().enumerate() {
        let s = s.trim();
        let e = e.trim();
        if parse_time(s).is_none() || parse_time(e).is_none() {
            return Err(format!("第 {} 节时间格式应为 HH:MM（如 08:00）", idx + 1));
        }
        arr.push(json!({ "no": idx + 1, "start": s, "end": e }));
    }
    root["periods"] = Value::Array(arr);
    write_root(&root)?;
    Ok("已保存作息时间表，周课表与提醒立即更新".into())
}

// ---------------------------------------------------------------
// Excel 课表导入：先识别、预览，确认后再写入
// ---------------------------------------------------------------

/// 识别结果预览（「识别」之后、「导入」之前显示在设置页）。
#[derive(Clone, Debug)]
pub struct ImportPreview {
    /// 来源文件名，让用户确认选对了表。
    pub file: String,
    /// 识别到的表结构（网格表 / 明细表）。
    pub kind: &'static str,
    pub rows: Vec<course_import::Row>,
    /// 像课程但缺字段、被跳过的格子数。
    pub skipped: usize,
    /// 从表里读到上下课时间的节数。
    pub times_found: usize,
    /// 已归一化成 13 项的节次时间表，导入时整体写入。
    periods: Vec<Value>,
}

fn preview_slot() -> &'static Mutex<Option<ImportPreview>> {
    static SLOT: OnceLock<Mutex<Option<ImportPreview>>> = OnceLock::new();
    SLOT.get_or_init(|| Mutex::new(None))
}

/// 只读访问预览（设置页每帧都要看，别克隆整张表）。
pub fn with_preview<R>(f: impl FnOnce(Option<&ImportPreview>) -> R) -> R {
    match preview_slot().lock() {
        Ok(slot) => f(slot.as_ref()),
        Err(poisoned) => f(poisoned.into_inner().as_ref()),
    }
}

pub fn clear_preview() {
    if let Ok(mut slot) = preview_slot().lock() {
        *slot = None;
    }
}

/// 识别一个 .xlsx：只解析、不动课表，结果放进预览。
///
/// 返回给用户看的结果文案（成功/失败原因都写清楚）。
pub fn recognize_xlsx(path: &Path) -> Result<String, String> {
    let sheet = crate::auralsle_ai_status::xlsx::read_sheet(path)?;
    let parsed = crate::auralsle_ai_status::course_import::parse(&sheet)?;
    let mut msg = format!(
        "已识别 {} 条课程（{}）",
        parsed.rows.len(),
        parsed.kind
    );
    if parsed.times_found > 0 {
        msg.push_str(&format!("，读到 {} 个节次时间", parsed.times_found));
    }
    if parsed.skipped > 0 {
        msg.push_str(&format!("，跳过 {} 个非课程格", parsed.skipped));
    }
    msg.push_str("。确认无误后点「导入到课表」。");
    let file = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("课表.xlsx")
        .to_owned();
    if let Ok(mut slot) = preview_slot().lock() {
        *slot = Some(ImportPreview {
            file,
            kind: parsed.kind,
            rows: parsed.rows,
            skipped: parsed.skipped,
            times_found: parsed.times_found,
            periods: parsed.periods,
        });
    }
    Ok(msg)
}

/// 一步识别并导入 .xlsx 课表文件（兼容直接导入调用）。
pub fn import_xlsx(path: &Path) -> Result<String, String> {
    let _ = recognize_xlsx(path)?;
    apply_preview()
}

/// 把预览里的课程写进 `courses.json`（覆盖 periods / courses，保留学期起始等字段）。
pub fn apply_preview() -> Result<String, String> {
    let Some(preview) = preview_slot().lock().ok().and_then(|mut slot| slot.take()) else {
        return Err("还没有识别结果：先选 .xlsx 再点「识别」".into());
    };
    if preview.rows.is_empty() {
        return Err("识别结果里没有课程，未写入课表".into());
    }
    let courses: Vec<Value> = preview
        .rows
        .iter()
        .map(|r| {
            json!({
                "name": r.name,
                "teacher": r.teacher,
                "room": r.room,
                "weekday": r.weekday,
                "from": r.from,
                "to": r.to,
                "weeks": r.weeks,
            })
        })
        .collect();
    let count = courses.len();
    let mut root = read_root();
    root["periods"] = Value::Array(preview.periods);
    root["courses"] = Value::Array(courses);
    write_root(&root)?;
    Ok(format!(
        "已导入 {count} 条课程，日程页与课表岛立即生效（来源：{}）",
        preview.file
    ))
}

/// [ShiGuangSchedule] 导出通用 iCalendar (.ics) 日历文件，支持导入系统日历、Outlook 与手机日历。
pub fn export_ics_to(dest: &Path) -> Result<usize, String> {
    let root = read_root();
    let start_str = root.get("semester_start").and_then(Value::as_str).unwrap_or("2026-09-07");
    let sem_start = parse_date(start_str).unwrap_or_else(|| Local::now().date_naive());
    let periods = period_labels();
    let list = rows();
    if list.is_empty() {
        return Err("当前课表为空，无法导出 .ics".into());
    }
    let mut ics = String::from("BEGIN:VCALENDAR\nVERSION:2.0\nPRODID:-//Auralsle//ShiGuangSchedule//CN\nCALSCALE:GREGORIAN\n");
    let mut count = 0usize;
    for (idx, r) in list.iter().enumerate() {
        let p_from = r.from.clamp(1, 13) as usize - 1;
        let p_to = r.to.clamp(r.from, 13) as usize - 1;
        let st = periods.get(p_from).map(|x| x.0.replace(':', "")).unwrap_or_else(|| "0800".into());
        let et = periods.get(p_to).map(|x| x.1.replace(':', "")).unwrap_or_else(|| "0940".into());
        for w in 1..=20u32 {
            if !week_applies(&r.weeks, w) {
                continue;
            }
            let day = sem_start + ChronoDuration::days((w as i64 - 1) * 7 + (r.weekday.clamp(1, 7) as i64 - 1));
            let d_str = day.format("%Y%m%d").to_string();
            ics.push_str("BEGIN:VEVENT\n");
            ics.push_str(&format!("UID:auralsle-sgs-{idx}-w{w}@local\n"));
            ics.push_str(&format!("DTSTART:{d_str}T{st}00\n"));
            ics.push_str(&format!("DTEND:{d_str}T{et}00\n"));
            ics.push_str(&format!("SUMMARY:{}\n", r.name));
            if !r.room.is_empty() {
                ics.push_str(&format!("LOCATION:{}\n", r.room));
            }
            if !r.teacher.is_empty() {
                ics.push_str(&format!("DESCRIPTION:教师：{} · 第{}周 第{}-{}节\n", r.teacher, w, r.from, r.to));
            }
            ics.push_str("END:VEVENT\n");
            count += 1;
        }
    }
    ics.push_str("END:VCALENDAR\n");
    fs::write(dest, ics).map_err(|e| format!("导出 .ics 失败：{e}"))?;
    Ok(count)
}
