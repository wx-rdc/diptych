//! `word/styles.xml` 解析：styleId → 标题级别映射（plan.md §4.1：Heading1 → heading_level: 1）。

use std::collections::HashMap;

use super::xml_tree::parse_xml_to_tree;

/// 从样式的 name / styleId 推断标题级别。
/// 兼容两种命名：`w:name val="heading 1"`、styleId=`Heading1`（不区分大小写）。
fn heading_level_from(name: Option<&str>, style_id: Option<&str>) -> Option<u8> {
    let from_str = |s: &str| -> Option<u8> {
        let lower = s.to_lowercase();
        let rest = lower.strip_prefix("heading")?;
        let rest = rest.trim_start();
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if digits.is_empty() {
            return None;
        }
        let level: u8 = digits.parse().ok()?;
        (1..=9).contains(&level).then_some(level)
    };
    from_str(name.unwrap_or("")).or_else(|| from_str(style_id.unwrap_or("")))
}

/// 解析 styles.xml，返回 styleId → 标题级别
pub fn parse_styles(xml: &str) -> Result<HashMap<String, u8>, super::xml_tree::XmlTreeError> {
    let root = parse_xml_to_tree(xml)?;
    let mut map = HashMap::new();
    for style in root.descendants("w:style") {
        if style.attr("w:type") != Some("paragraph") && style.attr("type") != Some("paragraph") {
            continue;
        }
        let style_id = style
            .attr("w:styleId")
            .or_else(|| style.attr("styleId"))
            .map(|s| s.to_string());
        let name = style
            .child("w:name")
            .and_then(|n| n.attr("w:val").or_else(|| n.attr("val")))
            .map(|s| s.to_string());
        if let (Some(id), Some(level)) = (&style_id, heading_level_from(name.as_deref(), style_id.as_deref())) {
            map.insert(id.clone(), level);
        }
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    const STYLES: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<w:styles xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:style w:type="paragraph" w:styleId="Heading1">
    <w:name w:val="heading 1"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Heading2">
    <w:name w:val="heading 2"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="Title">
    <w:name w:val="Title"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="ListNumber">
    <w:name w:val="List Number"/>
  </w:style>
  <w:style w:type="paragraph" w:styleId="CustomId">
    <w:name w:val="heading 3"/>
  </w:style>
  <w:style w:type="character" w:styleId="Heading4Char">
    <w:name w:val="heading 4 char"/>
  </w:style>
</w:styles>"#;

    #[test]
    fn parses_heading_levels() {
        let map = parse_styles(STYLES).unwrap();
        assert_eq!(map.get("Heading1"), Some(&1));
        assert_eq!(map.get("Heading2"), Some(&2));
        // name 优先：CustomId 名为 heading 3
        assert_eq!(map.get("CustomId"), Some(&3));
        // 非段落样式不算；Title/ListNumber 不是标题
        assert!(!map.contains_key("Heading4Char"));
        assert!(!map.contains_key("Title"));
        assert!(!map.contains_key("ListNumber"));
        assert_eq!(map.len(), 3);
    }

    #[test]
    fn style_id_only_fallback() {
        let xml = r#"<w:styles><w:style w:type="paragraph" w:styleId="Heading5"><w:name w:val="My Style"/></w:style></w:styles>"#;
        let map = parse_styles(xml).unwrap();
        assert_eq!(map.get("Heading5"), Some(&5));
    }

    #[test]
    fn empty_styles() {
        assert!(parse_styles("<w:styles/>").unwrap().is_empty());
    }
}
