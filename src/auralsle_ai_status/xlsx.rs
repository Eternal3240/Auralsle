//! 最小 xlsx 读取器：只解出「单元格文本 + 合并区域」，够教务课表用。
//!
//! 为什么不引 `calamine` / `quick-xml`：上游用 `cargo build --locked`，加依赖会改
//! `Cargo.toml` + `Cargo.lock`，破坏可复现构建。xlsx 本质是 ZIP + XML，上游已经有
//! `flate2`（inflate）和 `regex`，自己读反而零依赖变更：
//!
//! - ZIP：只认 central directory + stored(0) / deflate(8)，不校验 CRC、不支持 ZIP64
//!   （教务导出的课表只有几十 KB）。
//! - XML：用正则抓 `<c>` / `<v>` / `<t>` / `<mergeCell>`，处理 `t="s"` 共享字符串、
//!   `t="str"` 内联字符串、`t="inlineStr"` 与数字单元格，并解常见 XML 实体。
//!
//! 这不是通用 Excel 库（公式、样式、日期序列值一律按文本原样返回），但足以覆盖
//! 湖南师大教务系统导出的课表，也兼容 WPS / Excel 另存为的常见写法。

use flate2::read::DeflateDecoder;
use regex::Regex;
use std::{fs, io::Read, path::Path};

/// 一张工作表：按行 / 列索引的文本网格 + 合并区域。
#[derive(Debug, Default)]
pub struct Sheet {
    pub cells: Vec<Vec<String>>,
    /// (起始列, 起始行, 结束列, 结束行)，全部 0-based、含端点。
    pub merges: Vec<(usize, usize, usize, usize)>,
}

fn u16le(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32le(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

struct Entry {
    name: String,
    method: u16,
    csize: usize,
    offset: usize,
}

/// 从 EOCD 反查 central directory。ZIP 允许末尾最多 65535 字节注释，所以从尾部倒扫。
fn central_directory(data: &[u8]) -> Result<Vec<Entry>, String> {
    if data.len() < 22 {
        return Err("不是有效的 xlsx（文件太小）".into());
    }
    let lo = data.len().saturating_sub(22 + 65535);
    let eocd = (lo..=data.len() - 22)
        .rev()
        .find(|&i| data.get(i..i + 4) == Some(&[0x50, 0x4b, 0x05, 0x06]))
        .ok_or("不是有效的 xlsx（找不到 ZIP 目录）")?;
    let count = u16le(data, eocd + 10).ok_or("ZIP 目录损坏")? as usize;
    let mut p = u32le(data, eocd + 16).ok_or("ZIP 目录损坏")? as usize;
    let mut out = Vec::with_capacity(count);
    for _ in 0..count {
        if data.get(p..p + 4) != Some(&[0x50, 0x4b, 0x01, 0x02]) {
            break;
        }
        let method = u16le(data, p + 10).ok_or("ZIP 目录损坏")?;
        let csize = u32le(data, p + 20).ok_or("ZIP 目录损坏")? as usize;
        let nlen = u16le(data, p + 28).ok_or("ZIP 目录损坏")? as usize;
        let elen = u16le(data, p + 30).ok_or("ZIP 目录损坏")? as usize;
        let clen = u16le(data, p + 32).ok_or("ZIP 目录损坏")? as usize;
        let offset = u32le(data, p + 42).ok_or("ZIP 目录损坏")? as usize;
        let name = String::from_utf8_lossy(
            data.get(p + 46..p + 46 + nlen).ok_or("ZIP 文件名越界")?,
        )
        .into_owned();
        out.push(Entry { name, method, csize, offset });
        p += 46 + nlen + elen + clen;
    }
    Ok(out)
}

/// 取一个 ZIP 成员的内容：stored 直接切片，deflate 走 `flate2`（raw deflate）。
fn entry_bytes(data: &[u8], e: &Entry) -> Result<Vec<u8>, String> {
    let p = e.offset;
    if data.get(p..p + 4) != Some(&[0x50, 0x4b, 0x03, 0x04]) {
        return Err(format!("ZIP 成员 {} 的局部头损坏", e.name));
    }
    let nlen = u16le(data, p + 26).ok_or("ZIP 局部头损坏")? as usize;
    let elen = u16le(data, p + 28).ok_or("ZIP 局部头损坏")? as usize;
    let start = p + 30 + nlen + elen;
    let raw = data
        .get(start..start + e.csize)
        .ok_or(format!("ZIP 成员 {} 数据越界", e.name))?;
    match e.method {
        0 => Ok(raw.to_vec()),
        8 => {
            let mut out = Vec::new();
            DeflateDecoder::new(raw)
                .read_to_end(&mut out)
                .map_err(|err| format!("解压 {} 失败：{err}", e.name))?;
            Ok(out)
        }
        other => Err(format!("不支持的 ZIP 压缩方式 {other}（{}）", e.name)),
    }
}

fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let r = &rest[i..];
        let Some(end) = r.find(';') else {
            out.push_str(r);
            return out;
        };
        let ent = &r[1..end];
        match ent {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let num = ent
                    .strip_prefix("#x")
                    .or_else(|| ent.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| ent.strip_prefix('#').and_then(|d| d.parse::<u32>().ok()));
                if let Some(ch) = num.and_then(char::from_u32) {
                    out.push(ch);
                } else {
                    out.push_str(&r[..=end]);
                }
            }
        }
        rest = &r[end + 1..];
    }
    out.push_str(rest);
    out
}

/// `A` → 0，`Z` → 25，`AA` → 26。
fn col_index(s: &str) -> usize {
    s.bytes()
        .fold(0usize, |acc, b| acc * 26 + (b.to_ascii_uppercase() - b'A' + 1) as usize)
        .saturating_sub(1)
}

fn shared_strings(xml: &str) -> Vec<String> {
    let si = Regex::new(r"(?s)<si>(.*?)</si>").unwrap();
    let t = Regex::new(r"(?s)<t[^>]*>(.*?)</t>").unwrap();
    si.captures_iter(xml)
        .map(|c| t.captures_iter(&c[1]).map(|m| decode(&m[1])).collect::<String>())
        .collect()
}

fn parse_merges(xml: &str) -> Vec<(usize, usize, usize, usize)> {
    // 只认 <mergeCells> 里的 ref；<dimension ref="A1:F5"> 也用同样的写法，
    // 若一起匹配会凭空多出一个整表合并区，课表连堂判断就全乱了。
    let block = Regex::new(r"(?s)<mergeCells[^>]*>(.*?)</mergeCells>").unwrap();
    let scope = block.captures(xml).map(|c| c[1].to_owned()).unwrap_or_default();
    let re = Regex::new(r#"ref="([A-Z]+)(\d+):([A-Z]+)(\d+)""#).unwrap();
    re.captures_iter(&scope)
        .filter_map(|c| {
            Some((
                col_index(&c[1]),
                c[2].parse::<usize>().ok()?.saturating_sub(1),
                col_index(&c[3]),
                c[4].parse::<usize>().ok()?.saturating_sub(1),
            ))
        })
        .collect()
}

fn parse_cells(xml: &str, shared: &[String]) -> Vec<Vec<String>> {
    let row_re = Regex::new(r"(?s)<row[^>]*>(.*?)</row>").unwrap();
    // 自闭合的空单元格 <c r="C2" s="1"/> 必须单独匹配：按「<c …>…</c>」硬找的话，
    // 它会一口吞掉同一行后面那格的真数据（Excel/WPS 导出里极常见），整行课程就此消失。
    let cell_re = Regex::new(r"(?s)<c\b([^>]*?)(?:/>|>(.*?)</c>)").unwrap();
    let ref_re = Regex::new(r#"r="([A-Z]+)(\d+)""#).unwrap();
    let t_re = Regex::new(r#"t="([^"]+)""#).unwrap();
    let v_re = Regex::new(r"(?s)<v[^>]*>(.*?)</v>").unwrap();
    let t_tag = Regex::new(r"(?s)<t[^>]*>(.*?)</t>").unwrap();
    let mut grid: Vec<Vec<String>> = Vec::new();
    for row in row_re.captures_iter(xml) {
        for c in cell_re.captures_iter(&row[1]) {
            let attrs = &c[1];
            let inner = c.get(2).map(|m| m.as_str()).unwrap_or("");
            let Some(r) = ref_re.captures(attrs) else { continue };
            let col = col_index(&r[1]);
            let row_idx = r[2].parse::<usize>().unwrap_or(1).saturating_sub(1);
            let kind = t_re.captures(attrs).map(|m| m[1].to_owned()).unwrap_or_default();
            let raw = if kind == "inlineStr" {
                t_tag.captures(inner).map(|m| decode(&m[1]))
            } else {
                v_re.captures(inner).map(|m| decode(&m[1]))
            };
            let value = if kind == "s" {
                raw.as_deref()
                    .and_then(|s| s.trim().parse::<usize>().ok())
                    .and_then(|i| shared.get(i).cloned())
                    .unwrap_or_default()
            } else {
                raw.unwrap_or_default()
            };
            if value.is_empty() {
                continue;
            }
            if grid.len() <= row_idx {
                grid.resize_with(row_idx + 1, Vec::new);
            }
            let line = &mut grid[row_idx];
            if line.len() <= col {
                line.resize(col + 1, String::new());
            }
            line[col] = value;
        }
    }
    grid
}

/// 解析 xlsx 字节（抽出来便于单测，不必落临时文件）。
fn parse_bytes(data: &[u8]) -> Result<Sheet, String> {
    let entries = central_directory(data)?;
    let sheet = entries
        .iter()
        .find(|e| e.name == "xl/worksheets/sheet1.xml")
        .or_else(|| {
            entries.iter().find(|e| {
                e.name.starts_with("xl/worksheets/sheet") && e.name.ends_with(".xml")
            })
        })
        .ok_or("xlsx 里找不到工作表")?;
    let shared = entries
        .iter()
        .find(|e| e.name == "xl/sharedStrings.xml")
        .map(|e| entry_bytes(data, e))
        .transpose()?
        .map(|b| shared_strings(&String::from_utf8_lossy(&b)))
        .unwrap_or_default();
    let xml = String::from_utf8_lossy(&entry_bytes(data, sheet)?).into_owned();
    Ok(Sheet {
        cells: parse_cells(&xml, &shared),
        merges: parse_merges(&xml),
    })
}

/// 读第一个工作表。失败信息直接给用户看，所以带中文原因。
pub fn read_sheet(path: &Path) -> Result<Sheet, String> {
    if !path.is_file() {
        return Err(format!("找不到文件：{}", path.display()));
    }
    if !path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
    {
        return Err("只支持 .xlsx（.xls 请先另存为 .xlsx）".into());
    }
    let data = fs::read(path).map_err(|e| format!("读取失败：{e}"))?;
    parse_bytes(&data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 手工拼一个 stored ZIP：不填 CRC（解析器也不校验），只为单测造样本。
    fn zip_stored(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut central = Vec::new();
        for (name, data) in entries {
            let off = out.len() as u32;
            out.extend_from_slice(&[0x50, 0x4b, 0x03, 0x04]);
            out.extend_from_slice(&20u16.to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // flags
            out.extend_from_slice(&0u16.to_le_bytes()); // method 0 = stored
            out.extend_from_slice(&0u16.to_le_bytes()); // time
            out.extend_from_slice(&0u16.to_le_bytes()); // date
            out.extend_from_slice(&0u32.to_le_bytes()); // crc（不校验）
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&(name.len() as u16).to_le_bytes());
            out.extend_from_slice(&0u16.to_le_bytes()); // extra len
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(data);

            central.extend_from_slice(&[0x50, 0x4b, 0x01, 0x02]);
            central.extend_from_slice(&20u16.to_le_bytes()); // version made by
            central.extend_from_slice(&20u16.to_le_bytes()); // version needed
            central.extend_from_slice(&0u16.to_le_bytes()); // flags
            central.extend_from_slice(&0u16.to_le_bytes()); // method
            central.extend_from_slice(&0u16.to_le_bytes()); // time
            central.extend_from_slice(&0u16.to_le_bytes()); // date
            central.extend_from_slice(&0u32.to_le_bytes()); // crc
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(data.len() as u32).to_le_bytes());
            central.extend_from_slice(&(name.len() as u16).to_le_bytes());
            central.extend_from_slice(&0u16.to_le_bytes()); // extra
            central.extend_from_slice(&0u16.to_le_bytes()); // comment
            central.extend_from_slice(&0u16.to_le_bytes()); // disk
            central.extend_from_slice(&0u16.to_le_bytes()); // internal
            central.extend_from_slice(&0u32.to_le_bytes()); // external
            central.extend_from_slice(&off.to_le_bytes());
            central.extend_from_slice(name.as_bytes());
        }
        let cd_off = out.len() as u32;
        let cd_size = central.len() as u32;
        out.extend_from_slice(&central);
        out.extend_from_slice(&[0x50, 0x4b, 0x05, 0x06]);
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&(entries.len() as u16).to_le_bytes());
        out.extend_from_slice(&cd_size.to_le_bytes());
        out.extend_from_slice(&cd_off.to_le_bytes());
        out.extend_from_slice(&0u16.to_le_bytes());
        out
    }

    #[test]
    fn reads_inline_strings_entities_and_merges() {
        let sheet = r#"<?xml version="1.0"?><worksheet><sheetData>
            <row r="3"><c r="A3" t="str"><v>节次/周次</v></c><c r="B3" t="str"><v>星期一</v></c><c r="C3" t="str"><v>星期二</v></c></row>
            <row r="4"><c r="A4" t="str"><v>第一节
(08:00 - 08:45)</v></c><c r="C4" t="str"><v>单片机&amp;原理(18160086.03) (杨进宝)

(1-10,天工楼中栋402教室)</v></c></row>
            </sheetData><mergeCells count="1"><mergeCell ref="C4:C5"/></mergeCells></worksheet>"#;
        let zip = zip_stored(&[("xl/worksheets/sheet1.xml", sheet.as_bytes())]);
        let s = parse_bytes(&zip).expect("应能解析 stored xlsx");
        assert_eq!(s.cells[2][0], "节次/周次");
        assert_eq!(s.cells[2][2], "星期二");
        assert!(s.cells[3][2].contains("单片机&原理"));
        assert!(s.cells[3][2].contains("杨进宝"));
        assert_eq!(s.merges, vec![(2, 3, 2, 4)], "C4:C5 -> 0-based (2,3)-(2,4)");
    }

    #[test]
    fn reads_shared_strings_and_inline_str() {
        let shared = r#"<?xml version="1.0"?><sst><si><t>计算机网络</t></si><si><r><t>张</t></r><r><t>建军</t></r></si></sst>"#;
        let sheet = r#"<worksheet><sheetData>
            <row r="1"><c r="A1" t="s"><v>0</v></c><c r="B1" t="s"><v>1</v></c><c r="C1" t="inlineStr"><is><t>第 10 周</t></is></c><c r="D1"><v>42</v></c></row>
            </sheetData></worksheet>"#;
        let zip = zip_stored(&[
            ("xl/sharedStrings.xml", shared.as_bytes()),
            ("xl/worksheets/sheet1.xml", sheet.as_bytes()),
        ]);
        let s = parse_bytes(&zip).expect("应能解析共享字符串");
        assert_eq!(s.cells[0][0], "计算机网络");
        assert_eq!(s.cells[0][1], "张建军", "富文本 si 应拼接所有 t");
        assert_eq!(s.cells[0][2], "第 10 周");
        assert_eq!(s.cells[0][3], "42");
    }

    #[test]
    fn rejects_non_zip() {
        assert!(parse_bytes(b"not a zip at all").is_err());
    }

    /// 可选的真机冒烟：设置 `AURALSLE_TEST_XLSX=<课表路径>` 才跑，验证真实教务导出。
    #[test]
    fn optional_real_workbook_smoke() {
        let Ok(p) = std::env::var("AURALSLE_TEST_XLSX") else { return };
        let sheet = read_sheet(Path::new(&p)).expect("真实课表应能解析");
        assert!(!sheet.cells.is_empty(), "应解析出单元格");
        assert!(!sheet.merges.is_empty(), "教务课表应带合并单元格");
        println!(
            "真实课表：{} 行、{} 个合并区域",
            sheet.cells.len(),
            sheet.merges.len()
        );
    }
}
