//! quick-xml 事件流 → 轻量 DOM 树（规避命名空间/深嵌套难题，plan.md PoC-① 风险缓解首选方案）。

use std::collections::HashMap;

use quick_xml::events::Event;
use quick_xml::Reader;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum XmlTreeError {
    #[error("XML 解析错误: {0}")]
    Parse(#[from] quick_xml::Error),
    #[error("根元素之后存在多余内容或根元素缺失")]
    NoRootElement,
}

/// 元素节点
#[derive(Debug, Clone, PartialEq)]
pub struct El {
    pub name: String,
    pub attrs: HashMap<String, String>,
    pub children: Vec<Node>,
}

/// 树节点：元素或文本
#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    El(El),
    Text(String),
}

impl El {
    /// 第一个指定名称的直接子元素
    pub fn child(&self, name: &str) -> Option<&El> {
        self.children.iter().find_map(|n| match n {
            Node::El(e) if e.name == name => Some(e),
            _ => None,
        })
    }

    /// 全部指定名称的直接子元素
    pub fn children_named(&self, name: &str) -> Vec<&El> {
        self.children
            .iter()
            .filter_map(|n| match n {
                Node::El(e) if e.name == name => Some(e),
                _ => None,
            })
            .collect()
    }

    /// 递归查询属性
    pub fn attr(&self, name: &str) -> Option<&str> {
        self.attrs.get(name).map(|s| s.as_str())
    }

    /// 任意属性（按本地名匹配，忽略命名空间前缀）
    pub fn attr_local(&self, local: &str) -> Option<&str> {
        self.attrs
            .iter()
            .find(|(k, _)| k.rsplit(':').next() == Some(local))
            .map(|(_, v)| v.as_str())
    }

    /// 所有后代中指定名称的元素（文档序，深度优先）
    pub fn descendants<'a>(&'a self, name: &'a str) -> Vec<&'a El> {
        let mut out = Vec::new();
        fn walk<'a>(el: &'a El, name: &'a str, out: &mut Vec<&'a El>) {
            for c in &el.children {
                if let Node::El(e) = c {
                    if e.name == name {
                        out.push(e);
                    }
                    walk(e, name, out);
                }
            }
        }
        walk(self, name, &mut out);
        out
    }

    /// 自身与全部后代的文本拼接
    pub fn text_content(&self) -> String {
        let mut out = String::new();
        fn walk(el: &El, out: &mut String) {
            for c in &el.children {
                match c {
                    Node::Text(t) => out.push_str(t),
                    Node::El(e) => walk(e, out),
                }
            }
        }
        walk(self, &mut out);
        out
    }

    /// 重新序列化为 XML 字符串（用于表格/公式原始数据存档）。
    /// 文本节点转义，属性按 HashMap 无序输出。
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        fn escape_text(s: &str, out: &mut String) {
            for ch in s.chars() {
                match ch {
                    '&' => out.push_str("&amp;"),
                    '<' => out.push_str("&lt;"),
                    '>' => out.push_str("&gt;"),
                    _ => out.push(ch),
                }
            }
        }
        fn escape_attr(s: &str, out: &mut String) {
            escape_text(s, out);
        }
        fn write(el: &El, out: &mut String) {
            out.push('<');
            out.push_str(&el.name);
            let mut keys: Vec<&String> = el.attrs.keys().collect();
            keys.sort();
            for k in keys {
                out.push(' ');
                out.push_str(k);
                out.push_str("=\"");
                escape_attr(&el.attrs[k], out);
                out.push('"');
            }
            if el.children.is_empty() {
                out.push_str("/>");
                return;
            }
            out.push('>');
            for c in &el.children {
                match c {
                    Node::Text(t) => escape_text(t, out),
                    Node::El(e) => write(e, out),
                }
            }
            out.push_str("</");
            out.push_str(&el.name);
            out.push('>');
        }
        write(self, &mut out);
        out
    }
}

/// 解析 XML 字符串为树（返回根元素）
pub fn parse_xml_to_tree(xml: &str) -> Result<El, XmlTreeError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);

    let mut stack: Vec<El> = Vec::new();
    let mut root: Option<El> = None;

    loop {
        match reader.read_event()? {
            Event::Start(e) => {
                let el = el_from_event(&e)?;
                stack.push(el);
            }
            Event::Empty(e) => {
                let el = el_from_event(&e)?;
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::El(el));
                } else if root.is_none() {
                    root = Some(el);
                } else {
                    return Err(XmlTreeError::NoRootElement);
                }
            }
            Event::End(_) => {
                let el = stack.pop().expect("End 事件必须有匹配的 Start");
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::El(el));
                } else if root.is_none() {
                    root = Some(el);
                } else {
                    return Err(XmlTreeError::NoRootElement);
                }
            }
            Event::Text(t) => {
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(Node::Text(t.unescape()?.into_owned()));
                }
            }
            Event::CData(t) => {
                if let Some(parent) = stack.last_mut() {
                    let txt = String::from_utf8_lossy(t.as_ref()).into_owned();
                    parent.children.push(Node::Text(txt));
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }

    if !stack.is_empty() {
        return Err(XmlTreeError::NoRootElement);
    }
    root.ok_or(XmlTreeError::NoRootElement)
}

fn el_from_event(e: &quick_xml::events::BytesStart<'_>) -> Result<El, quick_xml::Error> {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut attrs = HashMap::new();
    for attr in e.attributes().with_checks(false) {
        let attr = attr?;
        let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        let value = attr.unescape_value()?.into_owned();
        attrs.insert(key, value);
    }
    Ok(El {
        name,
        attrs,
        children: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_nested_tree() {
        let tree = parse_xml_to_tree(
            r#"<w:p a="1"><w:r><w:t>hi</w:t></w:r><w:r><w:t>there</w:t></w:r></w:p>"#,
        )
        .unwrap();
        assert_eq!(tree.name, "w:p");
        assert_eq!(tree.attr("a"), Some("1"));
        let runs = tree.children_named("w:r");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].child("w:t").unwrap().text_content(), "hi");
        assert_eq!(tree.text_content(), "hithere");
    }

    #[test]
    fn self_closing_element() {
        let tree = parse_xml_to_tree(r#"<w:p><w:br/></w:p>"#).unwrap();
        let p = tree.child("w:p").unwrap_or(&tree);
        assert!(p.child("w:br").is_some());
        assert!(p.children_named("w:br")[0].children.is_empty());
    }

    #[test]
    fn namespaced_attributes() {
        let tree =
            parse_xml_to_tree(r#"<a:blip r:embed="rId5" c7:state="ok"/>"#).unwrap();
        assert_eq!(tree.attr("r:embed"), Some("rId5"));
        assert_eq!(tree.attr_local("embed"), Some("rId5"));
        assert_eq!(tree.attr_local("state"), Some("ok"));
    }

    #[test]
    fn descendants_document_order() {
        let tree = parse_xml_to_tree(
            r#"<w:tbl><w:tr><w:tc><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc></w:tr><w:tr><w:tc><w:t>b</w:t></w:tc></w:tr></w:tbl>"#,
        )
        .unwrap();
        let trs = tree.descendants("w:tr");
        assert_eq!(trs.len(), 2);
        let ts = tree.descendants("w:t");
        assert_eq!(ts.len(), 2);
        assert_eq!(ts[0].text_content(), "a");
    }

    #[test]
    fn mixed_content_preserved() {
        let tree =
            parse_xml_to_tree(r#"<w:r>pre<w:t>mid</w:t>post</w:r>"#).unwrap();
        assert_eq!(tree.children.len(), 3);
        assert_eq!(tree.text_content(), "premidpost");
    }

    #[test]
    fn to_xml_roundtrip_escapes() {
        let tree = parse_xml_to_tree(r#"<w:t xml:space="preserve">a &amp; b &lt;c&gt;</w:t>"#)
            .unwrap();
        let xml = tree.to_xml();
        assert!(xml.contains("a &amp; b &lt;c&gt;"), "got: {xml}");
        // 再解析应得到相同文本
        let back = parse_xml_to_tree(&xml).unwrap();
        assert_eq!(back.text_content(), "a & b <c>");
    }

    #[test]
    fn invalid_xml_is_error() {
        assert!(parse_xml_to_tree("<w:p><w:r></w:p>").is_err());
    }
}
