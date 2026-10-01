use ego_tree::NodeRef;
use scraper::{ElementRef, Html, Node, Selector};

use crate::js;

pub type Ref<'a> = NodeRef<'a, Node>;

pub fn select<'a, 'b>(
    document: &'a Html,
    selector: &'b Selector,
) -> scraper::element_ref::Select<'a, 'b> {
    document.root_element().select(selector)
}

pub fn text_of(node: Ref) -> String {
    let mut out = String::new();
    for n in node.descendants() {
        if let Some(text) = n.value().as_text() {
            out.push_str(text);
        }
    }
    out
}

pub fn truthy(value: Option<&str>) -> Option<&str> {
    value.filter(|v| !v.is_empty())
}

pub fn tag(el: &ElementRef) -> String {
    el.value().name().to_ascii_lowercase()
}

pub fn is(el: &ElementRef, name: &str) -> bool {
    el.value().name().eq_ignore_ascii_case(name)
}

pub fn classes<'a>(el: &ElementRef<'a>) -> impl Iterator<Item = &'a str> {
    el.attr("class")
        .unwrap_or("")
        .split(js::is_space)
        .filter(|c| !c.is_empty())
}

pub fn has_class(el: &ElementRef, class: &str) -> bool {
    classes(el).any(|c| c == class)
}

pub fn child_elements(node: Ref<'_>) -> impl Iterator<Item = ElementRef<'_>> {
    node.children().filter_map(ElementRef::wrap)
}

pub fn children_named<'a>(
    node: Ref<'a>,
    names: &'static [&'static str],
) -> impl Iterator<Item = ElementRef<'a>> {
    child_elements(node).filter(move |el| names.iter().any(|name| is(el, name)))
}

pub fn descendants(node: Ref<'_>) -> impl Iterator<Item = ElementRef<'_>> {
    node.descendants().skip(1).filter_map(ElementRef::wrap)
}

pub fn descendants_named<'a>(
    node: Ref<'a>,
    names: &'static [&'static str],
) -> impl Iterator<Item = ElementRef<'a>> {
    descendants(node).filter(move |el| names.iter().any(|name| is(el, name)))
}

pub fn next_element(node: Ref<'_>) -> Option<ElementRef<'_>> {
    let mut next = node.next_sibling();
    while let Some(n) = next {
        if let Some(el) = ElementRef::wrap(n) {
            return Some(el);
        }
        next = n.next_sibling();
    }
    None
}

pub fn has_id_or_name(el: &ElementRef) -> bool {
    el.attr("id").is_some() || el.attr("name").is_some()
}
