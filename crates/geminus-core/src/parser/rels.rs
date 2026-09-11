//! `word/_rels/document.xml.rels` 解析：rId → media 目标映射。

use std::collections::HashMap;

use super::xml_tree::parse_xml_to_tree;

/// 解析关系表，返回 Id → Target（如 "rId5" → "media/image1.png"）
pub fn parse_rels(xml: &str) -> Result<HashMap<String, String>, super::xml_tree::XmlTreeError> {
    let root = parse_xml_to_tree(xml)?;
    let mut map = HashMap::new();
    for rel in root.descendants("Relationship") {
        if let (Some(id), Some(target)) = (rel.attr("Id"), rel.attr("Target")) {
            map.insert(id.to_string(), target.to_string());
        }
    }
    Ok(map)
}

/// 将 rels Target 归一化为 zip 内路径：
/// Target 相对于 `word/` 目录（如 "media/image1.png"），也可能带 "word/" 前缀或 "../" 前缀。
pub fn normalize_target(target: &str) -> String {
    let t = target.trim_start_matches("./");
    if t.starts_with("word/") {
        return t.to_string();
    }
    format!("word/{t}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELS: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">
  <Relationship Id="rId3" Type=".../settings" Target="settings.xml"/>
  <Relationship Id="rId5" Type=".../image" Target="media/image1.png"/>
</Relationships>"#;

    #[test]
    fn parses_relationships() {
        let map = parse_rels(RELS).unwrap();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("rId5").unwrap(), "media/image1.png");
        assert_eq!(map.get("rId3").unwrap(), "settings.xml");
    }

    #[test]
    fn normalizes_targets() {
        assert_eq!(normalize_target("media/image1.png"), "word/media/image1.png");
        assert_eq!(
            normalize_target("word/media/image2.png"),
            "word/media/image2.png"
        );
    }

    #[test]
    fn empty_rels() {
        let map = parse_rels("<Relationships/>").unwrap();
        assert!(map.is_empty());
    }
}
