//! Excel 课表识别：把 xlsx 工作表翻译成 `courses.json` 的 `periods` / `courses`。
//!
//! 教务系统的导出格式五花八门，这里不绑定某一家，按「先网格、后明细」两路识别：
//!
//! - **网格表**（最常见）：表头是「节次 | 星期一 … 星期日」，下面一行一节课，
//!   课程信息写在星期列里；连堂靠合并单元格或「第3-4节」这样的标签给出。
//! - **明细表**：一行一条记录，表头含「课程名 / 星期 / 节次 / 周次 / 教室 / 教师」。
//!
//! 单个格子里的课程文本再走一遍容错解析，兼容下面这些常见写法：
//!
//! ```text
//! 高等数学(张伟) 1-16周 A101
//! 高等数学
//! 张伟
//! 1-16周
//! A101
//! 高等数学(代码) (张伟)\n\n(1-16, A101)      ← 旧版教务导出
//! ```
//!
//! 识别结果先给用户预览，确认后才写入课表；解析失败时返回能照着改的中文原因。

use std::sync::OnceLock;

use regex::Regex;
use serde_json::{json, Value};

use crate::auralsle_ai_status::xlsx::Sheet;

/// 课表最多认到第 13 节（与内置作息一致）。
pub const MAX_PERIOD: u32 = 13;

/// 内置作息：xlsx 里没有上下课时间时用它兜底，保证 `periods` 一定是 13 项。
const FALLBACK: [(&str, &str); 13] = [
    ("08:00", "08:45"),
    ("08:55", "09:40"),
    ("10:00", "10:45"),
    ("10:55", "11:40"),
    ("12:45", "13:30"),
    ("13:30", "14:15"),
    ("14:30", "15:15"),
    ("15:25", "16:10"),
    ("16:30", "17:15"),
    ("17:25", "18:10"),
    ("19:00", "19:45"),
    ("19:55", "20:40"),
    ("20:50", "21:35"),
];

/// 一条识别出来的课程（不按周展开，和 `courses.json` 的一行对应）。
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub name: String,
    pub weekday: u32,
    pub from: u32,
    pub to: u32,
    pub weeks: String,
    pub room: String,
    pub teacher: String,
}

impl Row {
    fn key(&self) -> (String, u32, u32) {
        (self.name.clone(), self.weekday, self.from)
    }
}

/// 一次识别的结果。
pub struct Parsed {
    pub rows: Vec<Row>,
    pub periods: Vec<Value>,
    /// 识别到的格式，给用户确认用。
    pub kind: &'static str,
    /// 看起来像课程但缺字段、被跳过的行数。
    pub skipped: usize,
    /// 从表格里读到上下课时间的节数。
    pub times_found: usize,
}

// ---------------------------------------------------------------
// 基础归一化
// ---------------------------------------------------------------

/// 全角数字 / 冒号 / 逗号 / 括号转半角（不动中文名，避免把「至」这类字改掉）。
fn half(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '０'..='９' => char::from_u32(c as u32 - 0xFEE0).unwrap_or(c),
            '：' => ':',
            '，' => ',',
            '（' => '(',
            '）' => ')',
            '～' => '~',
            _ => c,
        })
        .collect()
}

#[allow(dead_code)]
fn re_weekday() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(星期[一二三四五六日天]|周[一二三四五六日天]|礼拜[一二三四五六日天])$").unwrap())
}

/// 「周一」/「星期一」/「礼拜一」/「周日」→ 1..7；纯数字 1..7 也算。
fn weekday_of(cell: &str) -> Option<u32> {
    let t = half(cell);
    let t = t.trim().trim_matches('*').trim();
    if t.is_empty() {
        return None;
    }
    const CN: [char; 7] = ['一', '二', '三', '四', '五', '六', '日'];
    for (i, ch) in CN.iter().enumerate() {
        if t.contains(&format!("星期{ch}"))
            || t.contains(&format!("周{ch}"))
            || t.contains(&format!("礼拜{ch}"))
        {
            return Some(i as u32 + 1);
        }
    }
    if t.contains("星期天") || t.contains("周天") || t.contains("礼拜天") {
        return Some(7);
    }
    // 有些教务直接写 1..7 或者 "周一(1)"
    if let Ok(n) = t.parse::<u32>() {
        if (1..=7).contains(&n) {
            return Some(n);
        }
    }
    None
}

fn re_period() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"第?\s*(\d{1,2})\s*(?:[-~一至到]\s*(\d{1,2}))?\s*节").unwrap()
    })
}

fn re_bare_period() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*(\d{1,2})\s*(?:[-~]\s*(\d{1,2}))?\s*$").unwrap())
}

fn re_cn_period() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"第([一二三四五六七八九十]{1,3})节").unwrap())
}

fn cn_digit(c: char) -> Option<u32> {
    Some(match c {
        '一' => 1,
        '二' => 2,
        '三' => 3,
        '四' => 4,
        '五' => 5,
        '六' => 6,
        '七' => 7,
        '八' => 8,
        '九' => 9,
        _ => return None,
    })
}

/// 「十三」「十一」「十」→ 13 / 11 / 10。
fn cn_number(s: &str) -> Option<u32> {
    let mut chars = s.chars();
    let first = chars.next()?;
    if first == '十' {
        return Some(10 + chars.next().and_then(cn_digit).unwrap_or(0));
    }
    let n = cn_digit(first)?;
    match chars.next() {
        None => Some(n),
        Some('十') => Some(n * 10 + chars.next().and_then(cn_digit).unwrap_or(0)),
        _ => None,
    }
}

/// 节次区间：`第3-4节` / `3-4节` / `3~4` / 纯 `3`。
fn period_span(cell: &str) -> Option<(u32, u32)> {
    let t = half(cell);
    // 「1-16周」是周次不是节次，别把它的范围当成连堂。
    if t.contains('周') {
        return None;
    }
    if let Some(c) = re_period().captures(&t) {
        let a = c[1].parse::<u32>().ok()?;
        let b = c
            .get(2)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(a);
        if a == 0 || a > MAX_PERIOD {
            return None;
        }
        return Some((a.min(b), a.max(b).min(MAX_PERIOD)));
    }
    if let Some(c) = re_cn_period().captures(&t) {
        let a = cn_number(&c[1])?;
        if (1..=MAX_PERIOD).contains(&a) {
            return Some((a, a));
        }
    }
    if let Some(c) = re_bare_period().captures(&t) {
        let a = c[1].parse::<u32>().ok()?;
        let b = c
            .get(2)
            .and_then(|m| m.as_str().parse::<u32>().ok())
            .unwrap_or(a);
        if (1..=MAX_PERIOD).contains(&a) {
            return Some((a.min(b), a.max(b).min(MAX_PERIOD)));
        }
    }
    None
}

fn re_clock() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d{1,2}):(\d{2})\s*[-~至到]\s*(\d{1,2}):(\d{2})").unwrap()
    })
}

/// （A 列里的）上下课时间：`08:00-08:45` / `8:00~8:45`。
fn clock_pair(cell: &str) -> Option<(String, String)> {
    let t = half(cell);
    let c = re_clock().captures(&t)?;
    let start = format!("{:0>2}:{}", &c[1], &c[2]);
    let end = format!("{:0>2}:{}", &c[3], &c[4]);
    let ok = |v: &str| {
        let (h, m) = v.split_once(':')?;
        let (h, m) = (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?);
        (h < 24 && m < 60).then_some(())
    };
    (ok(&start).is_some() && ok(&end).is_some()).then_some((start, end))
}

fn re_weeks() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(\d{1,2}(?:\s*[-~]\s*\d{1,2})?(?:\s*,\s*\d{1,2}(?:\s*[-~]\s*\d{1,2})?)*)\s*周",
        )
        .unwrap()
    })
}

#[allow(dead_code)]
fn re_weeks_loose() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(\d{1,2}(?:\s*[-~]\s*\d{1,2})?(?:\s*,\s*\d{1,2}(?:\s*[-~]\s*\d{1,2})?)*)").unwrap()
    })
}

fn re_bare_weeks() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"^(\d{1,2}(?:\s*[-~]\s*\d{1,2})?(?:\s*,\s*\d{1,2}(?:\s*[-~]\s*\d{1,2})?)*)\s*(单|双)?$")
            .unwrap()
    })
}

/// 一个片段整体就是周次（`2-14双`、`1-16`、`1,3,5`）时返回归一化结果。
///
/// 没有「周」字也没写单双、且上界很小（`1-2` 这种）时不认，避免把节次当周次。
fn bare_weeks(token: &str) -> Option<String> {
    let t = half(token);
    let t = t.trim();
    if t.is_empty() {
        return None;
    }
    let c = re_bare_weeks().captures(t)?;
    let body = c[1].replace(' ', "").replace('~', "-");
    let parity = c.get(2).map(|m| m.as_str()).unwrap_or("");
    let bound = body
        .split(['-', ','])
        .filter_map(|v| v.trim().parse::<u32>().ok())
        .max()
        .unwrap_or(0);
    if parity.is_empty() && bound < 6 {
        return None;
    }
    Some(format!("{body}{parity}"))
}

/// 周次归一化成 `1-16` / `1-16单` / `1,3,5` 这种 `parse_weeks` 认得的写法。
fn weeks_text(cell: &str) -> Option<String> {
    let t = half(cell);
    // 有「周」字：`1-16周` / `第1-16周(单)` / `1,3,5周`。
    if let Some(c) = re_weeks().captures(&t) {
        let body = c[1].replace(' ', "").replace('~', "-");
        let parity = if t.contains('单') {
            "单"
        } else if t.contains('双') {
            "双"
        } else {
            ""
        };
        return Some(format!("{body}{parity}"));
    }
    // 没有「周」字时按片段认：`2-14双` / `1-16` 这类纯数字区间。
    if t.contains('节') {
        return None;
    }
    t.split(|c: char| {
        c.is_whitespace() || matches!(c, '(' | ')' | '（' | '）' | ',' | '，' | ';' | '；')
    })
    .find_map(bare_weeks)
}

/// 标签取值：`教室：A101` / `地点 A101`。
fn labeled(text: &str, labels: &[&str]) -> Option<String> {
    for label in labels {
        let Some(start) = text.find(label) else { continue };
        let rest = &text[start + label.len()..];
        let rest = rest.trim_start_matches([':', '：', ' ', '\t']);
        let value: String = rest
            .chars()
            .take_while(|c| !matches!(c, '\n' | ',' | ';' | '；' | '(' | ')' | '，'))
            .collect();
        let value = value.trim();
        if !value.is_empty() && value.chars().count() <= 24 {
            return Some(value.to_owned());
        }
    }
    None
}

fn re_room_token() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"[A-Za-z]{0,4}\d{2,4}[A-Za-z]?|[一二三四五六七八九十\d]+(?:号)?(?:教学楼|实验楼|楼|馆|室)[A-Za-z]?\d{0,4}").unwrap()
    })
}

/// 没有「教室」标签时，从文本里猜一个像教室的片段（A101 / 综合楼203）。
fn guess_room(text: &str) -> Option<String> {
    let t = half(text);
    let mut best: Option<String> = None;
    for m in re_room_token().find_iter(&t) {
        let token = m.as_str().trim();
        // 纯数字（比如节次、周次）不算教室。
        if token.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let looks_room = token.chars().any(|c| c.is_ascii_alphabetic())
            || token.contains('楼')
            || token.contains('室')
            || token.contains('馆');
        if looks_room && token.chars().count() <= 16 {
            best = Some(token.to_owned());
            break;
        }
    }
    best
}

fn is_cjk_name(token: &str) -> bool {
    let n = token.chars().count();
    (2..=4).contains(&n) && token.chars().all(|c| ('\u{4e00}'..='\u{9fff}').contains(&c))
}

/// 判断一个课程名是否是旧版解析残留的脏数据（如 `(5-16,天工楼中栋306教室)`、`18160086`、`正常`、`已选`）。
pub fn is_bogus_course_name(name: &str) -> bool {
    let t = name.trim();
    if t.is_empty() || t.starts_with('(') || t.starts_with('（') || t.ends_with("教室)") || t.ends_with("教室）") {
        return true;
    }
    // 纯数字 / 小数（课程代码如 18160086、学分如 2.5）
    if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | '-' | '_' | ' ')) {
        return true;
    }
    // 选课状态 / 汇总表字段名
    if matches!(
        t,
        "正常"
            | "已选"
            | "退选"
            | "重修"
            | "必修"
            | "选修"
            | "课程列表"
            | "课程列表:"
            | "课程列表："
            | "课程代码"
            | "课程名称"
            | "序号"
            | "学分"
            | "总学分"
            | "选课状态"
            | "上课状态"
            | "备注"
    ) {
        return true;
    }
    false
}

// ---------------------------------------------------------------
// 单个格子 → 若干门课
// ---------------------------------------------------------------

/// 一个格子里可能叠了多门课，用空行 / 分隔线切开。
///
/// 注意：教务系统常见格式 `课程名(代码) (教师)\n\n\n(5-16,天工楼中栋306教室)` 中，
/// 空行后面的 `(5-16,天工楼中栋306教室)` 是上一门课的周次与教室，必须合并回上一块，
/// 绝不能拆成单独的一门课！
fn split_blocks(cell: &str) -> Vec<String> {
    let mut raw_blocks: Vec<String> = Vec::new();
    let mut buf = String::new();
    for line in cell.lines() {
        let trimmed = line.trim();
        let separator = trimmed.is_empty()
            || trimmed.chars().all(|c| matches!(c, '-' | '=' | '—' | '_' | '*' | '·'));
        if separator {
            if !buf.trim().is_empty() {
                raw_blocks.push(buf.clone());
            }
            buf.clear();
        } else {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(trimmed);
        }
    }
    if !buf.trim().is_empty() {
        raw_blocks.push(buf);
    }
    let mut blocks: Vec<String> = Vec::new();
    for b in raw_blocks {
        let t = b.trim();
        let first_line = t.lines().next().unwrap_or("").trim();
        if (first_line.starts_with('(') || first_line.starts_with('（')) && !blocks.is_empty() {
            if let Some(last) = blocks.last_mut() {
                last.push('\n');
                last.push_str(t);
                continue;
            }
        }
        blocks.push(b);
    }
    blocks
}

/// 解析一个课程格。返回 `None` 表示这格不是课程（表头、纯时间、空行等）。
fn course_from_block(block: &str) -> Option<Row> {
    let text = half(block);
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    // 表头 / 时间行 / 括号属性行不是独立课程。
    if lines.is_empty()
        || re_clock().is_match(lines[0])
        || weekday_of(lines[0]).is_some()
        || lines[0].starts_with('(')
        || lines[0].starts_with('（')
    {
        return None;
    }
    let mut weeks = weeks_text(&text);
    let mut room = labeled(&text, &["上课教室", "教室", "地点", "上课地点"]);
    let mut teacher = labeled(&text, &["任课教师", "授课教师", "教师", "老师"]);

    // 旧版教务格式：`课程名(代码) (老师)` + 单独一行 `(1-16,教室)`。
    // 扫描整块文本中的所有括号对，提取 `(周次,教室)` 与 `(教师)`。
    let paren = Regex::new(r"[（(]([^（）()]*)[）)]").unwrap();
    for cap in paren.captures_iter(&text) {
        let value = cap[1].trim().to_owned();
        if let Some((left, right)) = value.split_once(['，', ',']) {
            // 左半是周次、右半是教室（如 `5-16,天工楼中栋306教室` 或 `9-12,至善楼208教室`）。
            if let Some(w) = weeks_text(left).or_else(|| weeks_text(&format!("{}周", left.trim()))) {
                weeks = Some(w);
            }
            if room.is_none() && !right.trim().is_empty() {
                room = Some(right.trim().to_owned());
            }
        } else if teacher.is_none() && is_cjk_name(&value) {
            teacher = Some(value);
        }
    }

    let mut name = lines[0].to_owned();
    if let Some(first) = paren.find(&name) {
        let head = name[..first.start()].trim().to_owned();
        if !head.is_empty() {
            name = head;
        }
    }

    // 逐行补齐：剩下的行里认教室 / 教师。
    let mut rest_room: Option<String> = None;
    let mut rest_teacher: Option<String> = None;
    for line in lines.iter().skip(1) {
        if line.starts_with('(') || line.starts_with('（') {
            continue;
        }
        // 周次行、纯数字行不参与判断。
        if re_weeks().is_match(line) || line.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if room.is_none() && rest_room.is_none() {
            if let Some(r) = labeled(line, &["教室", "地点"]) {
                rest_room = Some(r);
                continue;
            }
        }
        if teacher.is_none() && rest_teacher.is_none() && is_cjk_name(line) {
            rest_teacher = Some(line.to_string());
            continue;
        }
        if room.is_none() && rest_room.is_none() {
            if let Some(r) = guess_room(line) {
                rest_room = Some(r);
                continue;
            }
        }
    }
    let room = room.or(rest_room).or_else(|| guess_room(&text));
    let teacher = teacher.or(rest_teacher);

    // 名字太长说明整块其实是说明文字，不当课程。
    let name = name.trim().trim_end_matches(['(', '（']).trim().to_owned();
    if name.is_empty() || name.chars().count() > 40 || is_bogus_course_name(&name) {
        return None;
    }
    // 名称里如果带着节次/时间，说明抓错了格。
    if re_clock().is_match(&name) || period_span(&name).is_some() {
        return None;
    }
    Some(Row {
        name,
        weekday: 1,
        from: 1,
        to: 1,
        weeks: weeks.unwrap_or_else(|| "1-16".to_owned()),
        room: room.unwrap_or_default(),
        teacher: teacher.unwrap_or_default(),
    })
}

// ---------------------------------------------------------------
// 两种表结构
// ---------------------------------------------------------------

fn header_row(grid: &[Vec<String>]) -> Option<usize> {
    grid.iter().position(|row| {
        let weekdays = row.iter().filter(|c| weekday_of(c).is_some()).count();
        weekdays >= 2
    })
}

/// 网格表：表头是星期，左侧是节次。
fn parse_grid(sheet: &Sheet) -> Option<Parsed> {
    let grid = &sheet.cells;
    let header = header_row(grid)?;
    let mut wd_cols: Vec<(usize, u32)> = Vec::new();
    for (col, cell) in grid[header].iter().enumerate() {
        if let Some(w) = weekday_of(cell) {
            if !wd_cols.iter().any(|(_, existing)| *existing == w) {
                wd_cols.push((col, w));
            }
        }
    }
    if wd_cols.len() < 2 {
        return None;
    }
    let label_cols = wd_cols.iter().map(|(c, _)| *c).min().unwrap_or(3).min(4).max(1);

    // 每行是不是「第 n 节」；顺手收集上下课时间。
    // 遇到课表下方的「课程列表：」「序号」「课程代码」汇总区域或节次倒退时立即停止！
    struct Line {
        row: usize,
        from: u32,
        to: u32,
        time: Option<(String, String)>,
    }
    let mut lines: Vec<Line> = Vec::new();
    let mut max_period_seen = 0u32;
    for (r, row) in grid.iter().enumerate().skip(header + 1) {
        let joined_head: String = row
            .iter()
            .take(label_cols + 2)
            .map(|s| s.trim())
            .collect::<Vec<_>>()
            .join(" ");
        if ["课程列表", "选课列表", "课程代码", "选课序号", "序号"]
            .iter()
            .any(|kw| joined_head.contains(kw))
        {
            break;
        }
        let mut span = None;
        let mut time = None;
        for cell in row.iter().take(label_cols) {
            if span.is_none() {
                span = period_span(cell);
            }
            if time.is_none() {
                time = clock_pair(cell);
            }
        }
        if let Some((from, to)) = span {
            // 如果已经扫描到第 8 节以后，又突然出现 1、2、3（下方附带的序号列表），立刻停止！
            if max_period_seen >= 8 && from < max_period_seen {
                break;
            }
            max_period_seen = max_period_seen.max(to);
            lines.push(Line { row: r, from, to, time });
        }
    }
    if lines.is_empty() {
        return None;
    }

    let period_at = |row: usize, lines: &[Line]| -> Option<u32> {
        lines
            .iter()
            .find(|l| l.row == row)
            .map(|l| l.from)
            .or_else(|| {
                lines
                    .iter()
                    .filter(|l| l.row <= row)
                    .max_by_key(|l| l.row)
                    .map(|l| l.from)
            })
    };

    // 检查该表是否采用双节合并单元格模式（如 C5:C6、D11:D12）；若是，部分漏合并的奇数节连堂课（下一节同列为空）也自动补齐为 2 节连堂。
    let has_pair_merges = sheet
        .merges
        .iter()
        .any(|(c0, r0, c1, r1)| c0 == c1 && *r0 > header && *r1 == *r0 + 1);

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped = 0usize;
    for (line_idx, line) in lines.iter().enumerate() {
        for (col, weekday) in &wd_cols {
            let Some(cell) = grid.get(line.row).and_then(|r| r.get(*col)) else {
                continue;
            };
            if cell.trim().is_empty() {
                continue;
            }
            // 连堂：合并单元格精确给出末行，没有就用标签里的「3-4节」。
            let mut to = line.to;
            if to <= line.from {
                if let Some((_, _, _, r1)) = sheet
                    .merges
                    .iter()
                    .find(|(c0, r0, c1, _)| c0 == col && c1 == col && *r0 == line.row)
                {
                    to = period_at(*r1, &lines).unwrap_or(line.from).max(line.from);
                } else if has_pair_merges && line.from % 2 == 1 {
                    if let Some(next_line) = lines.get(line_idx + 1) {
                        let next_empty = grid
                            .get(next_line.row)
                            .and_then(|r| r.get(*col))
                            .map_or(true, |s| s.trim().is_empty());
                        if next_line.from == line.from + 1 && next_empty {
                            to = next_line.from;
                        }
                    }
                }
            }
            for block in split_blocks(cell) {
                match course_from_block(&block) {
                    Some(mut course) => {
                        course.weekday = *weekday;
                        course.from = line.from;
                        course.to = to.max(line.from);
                        rows.push(course);
                    }
                    None => skipped += 1,
                }
            }
        }
    }
    if rows.is_empty() {
        return None;
    }
    normalize(&mut rows);

    let mut periods: Vec<Value> = Vec::with_capacity(MAX_PERIOD as usize);
    let mut times_found = 0usize;
    for no in 1..=MAX_PERIOD {
        let time = lines
            .iter()
            .find(|l| l.from == no)
            .and_then(|l| l.time.clone());
        let (start, end) = match time {
            Some(t) => {
                times_found += 1;
                t
            }
            None => (FALLBACK[(no - 1) as usize].0.to_owned(), FALLBACK[(no - 1) as usize].1.to_owned()),
        };
        periods.push(json!({"no": no, "start": start, "end": end}));
    }
    Some(Parsed { rows, periods, kind: "网格表", skipped, times_found })
}

/// 明细表：一行一条记录。
fn parse_list(sheet: &Sheet) -> Option<Parsed> {
    let grid = &sheet.cells;
    let mut found: Option<(usize, usize, usize, usize, Option<usize>, Option<usize>, Option<usize>, Option<usize>)> = None;
    for (r, row) in grid.iter().enumerate() {
        let has = |needles: &[&str]| row.iter().position(|c| needles.iter().any(|n| c.contains(n)));
        let name = has(&["课程名称", "课程名", "课程"]);
        let weekday = has(&["星期", "周几", "上课日"]);
        let period = has(&["节次", "节"]);
        if let (Some(name), Some(weekday), Some(period)) = (name, weekday, period) {
            let weeks = has(&["周次", "起止周", "上课周"]);
            let room = has(&["教室", "地点", "上课地点"]);
            let teacher = has(&["任课教师", "授课教师", "教师", "老师"]);
            let no = has(&["序号", "编号"]);
            found = Some((r, name, weekday, period, weeks, room, teacher, no));
            break;
        }
    }
    let (head, name_col, wd_col, period_col, weeks_col, room_col, teacher_col, _no_col) = found?;

    let cell = |r: usize, c: usize| -> String {
        grid.get(r)
            .and_then(|row| row.get(c))
            .map(|v| v.trim().to_owned())
            .unwrap_or_default()
    };

    let mut rows: Vec<Row> = Vec::new();
    let mut skipped = 0usize;
    for r in head + 1..grid.len() {
        let name = cell(r, name_col);
        if name.is_empty() || is_bogus_course_name(&name) {
            continue;
        }
        // 合计 / 备注行直接跳过。
        if ["合计", "总计", "备注", "说明", "以下"].iter().any(|k| name.contains(k)) {
            continue;
        }
        let weekday = match weekday_of(&cell(r, wd_col)) {
            Some(w) => w,
            None => {
                skipped += 1;
                continue;
            }
        };
        let (from, to) = match period_span(&cell(r, period_col)) {
            Some(p) => p,
            None => {
                skipped += 1;
                continue;
            }
        };
        let weeks = weeks_col
            .and_then(|c| weeks_text(&cell(r, c)))
            .or_else(|| weeks_text(&name))
            .unwrap_or_else(|| "1-16".to_owned());
        let room = room_col.map(|c| cell(r, c)).unwrap_or_default();
        let teacher = teacher_col.map(|c| cell(r, c)).unwrap_or_default();
        rows.push(Row { name, weekday, from, to, weeks, room, teacher });
    }
    if rows.is_empty() {
        return None;
    }
    normalize(&mut rows);
    let periods: Vec<Value> = (1..=MAX_PERIOD)
        .map(|no| {
            json!({
                "no": no,
                "start": FALLBACK[(no - 1) as usize].0,
                "end": FALLBACK[(no - 1) as usize].1,
            })
        })
        .collect();
    Some(Parsed { rows, periods, kind: "明细表", skipped, times_found: 0 })
}

/// 合并连堂、去重并排好序。
///
/// 有的导出把连堂课在每一行都写一遍（没合并单元格），这里按「同名 + 同一天 +
/// 节次相邻 + 周次相同」合并成一条 1-2 节，避免课表出现两条重复的课。
fn normalize(rows: &mut Vec<Row>) {
    rows.sort_by(|a, b| {
        (a.weekday, a.from, a.to, &a.weeks).cmp(&(b.weekday, b.from, b.to, &b.weeks))
    });
    let mut out: Vec<Row> = Vec::new();
    let mut seen: Vec<(String, u32, u32, String)> = Vec::new();
    for row in rows.drain(..) {
        if let Some(last) = out.last_mut() {
            if last.name == row.name
                && last.weekday == row.weekday
                && last.weeks == row.weeks
                && row.from <= last.to + 1
            {
                last.to = last.to.max(row.to);
                if last.room.is_empty() {
                    last.room = row.room;
                }
                if last.teacher.is_empty() {
                    last.teacher = row.teacher;
                }
                continue;
            }
        }
        let key = (row.name.clone(), row.weekday, row.from, row.weeks.clone());
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        out.push(row);
    }
    *rows = out;
}

/// 识别整张表：先按网格表，再退回明细表。
pub fn parse(sheet: &Sheet) -> Result<Parsed, String> {
    if sheet.cells.is_empty() {
        return Err("工作表是空的：请确认导出的是课程表所在的第一个工作表".into());
    }
    if let Some(parsed) = parse_grid(sheet) {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_list(sheet) {
        return Ok(parsed);
    }
    Err("没识别到课表。需要以下任一种表头：\n\
         ① 网格表：「节次 | 星期一 … 星期日」；\n\
         ② 明细表：「课程名 / 星期 / 节次 / 周次 / 教室 / 教师」。\n\
         识别的是 xlsx 的第一个工作表，必要时先把它移到最前面。"
        .into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(rows: &[&[&str]]) -> Sheet {
        Sheet {
            cells: rows
                .iter()
                .map(|row| row.iter().map(|c| (*c).to_owned()).collect())
                .collect(),
            merges: Vec::new(),
        }
    }

    #[test]
    fn reads_grid_timetable() {
        let s = sheet(&[
            &["节次", "星期一", "星期二", "星期三"],
            &["第1节 08:00-08:45", "", "大学英语\n李娜\n1-16周\nA203", ""],
            &["第2节 08:55-09:40", "", "大学英语\n李娜\n1-16周\nA203", ""],
            &["第3节 10:00-10:45", "高等数学(张伟) 1-16周 A101", "", ""],
        ]);
        let p = parse(&s).expect("应识别出网格表");
        assert_eq!(p.kind, "网格表");
        assert_eq!(p.rows.len(), 2, "两门课：英语连堂 + 高数");
        let english = p.rows.iter().find(|r| r.name.contains("大学英语")).unwrap();
        assert_eq!((english.weekday, english.from, english.to), (2, 1, 2));
        assert_eq!(english.teacher, "李娜");
        assert_eq!(english.room, "A203");
        assert_eq!(english.weeks, "1-16");
        let math = p.rows.iter().find(|r| r.name.contains("高等数学")).unwrap();
        assert_eq!((math.weekday, math.from, math.to), (1, 3, 3));
        assert_eq!(p.times_found, 3);
        assert_eq!(p.periods.len(), MAX_PERIOD as usize);
    }

    #[test]
    fn reads_list_timetable() {
        let s = sheet(&[
            &["序号", "课程名称", "星期", "节次", "周次", "教室", "教师"],
            &["1", "线性代数", "周三", "3-4", "1-8周", "B305", "王强"],
            &["2", "体育", "周五", "5-6", "1-16周(单)", "操场", "赵敏"],
        ]);
        let p = parse(&s).expect("应识别出明细表");
        assert_eq!(p.kind, "明细表");
        assert_eq!(p.rows.len(), 2);
        let lin = &p.rows[0];
        assert_eq!((lin.weekday, lin.from, lin.to), (3, 3, 4));
        assert_eq!(lin.weeks, "1-8");
        assert_eq!(lin.room, "B305");
        let pe = &p.rows[1];
        assert_eq!(pe.weeks, "1-16单");
    }

    #[test]
    fn parses_legacy_cell_text() {
        let row = course_from_block("高等数学(代码A) (张伟)\n\n(1-16, A101)").expect("旧格式应能解析");
        assert_eq!(row.name, "高等数学");
        assert_eq!(row.teacher, "张伟");
        assert_eq!(row.room, "A101");
        assert_eq!(row.weeks, "1-16");
    }

    #[test]
    fn rejects_non_course_cells() {
        assert!(course_from_block("第1节 08:00-08:45").is_none());
        assert!(course_from_block("星期一").is_none());
        assert!(course_from_block("   \n  ").is_none());
        assert!(course_from_block("(5-16,天工楼中栋306教室)").is_none());
        assert!(course_from_block("18160086").is_none());
        assert!(course_from_block("正常").is_none());
    }
}
