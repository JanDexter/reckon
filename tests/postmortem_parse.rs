//! Postmortem-corpus tests that don't pull the embedding model.
//!
//! `PostmortemIndex` lazy-embeds: the corpus + by-id lookup are built
//! offline; only `search()` triggers the ONNX download. These tests
//! exercise the offline path.

use reckon::postmortem::split_frontmatter;

#[test]
fn split_frontmatter_extracts_yaml_block() {
    let input = "---\nid: IR-117\ndate: 2024-09-04\n---\n\n# body\n\nprose";
    let (fm, body) = split_frontmatter(input).unwrap();
    assert!(fm.contains("id: IR-117"));
    assert!(fm.contains("date: 2024-09-04"));
    assert!(body.starts_with("# body"));
}

#[test]
fn split_frontmatter_handles_missing_frontmatter() {
    let input = "no frontmatter here\nsecond line";
    let (fm, body) = split_frontmatter(input).unwrap();
    assert!(fm.is_empty());
    assert_eq!(body, input);
}

#[test]
fn split_frontmatter_strips_utf8_bom() {
    let mut input = String::from("\u{feff}");
    input.push_str("---\nid: IR-1\n---\nbody");
    let (fm, body) = split_frontmatter(&input).unwrap();
    assert!(fm.contains("id: IR-1"));
    assert_eq!(body, "body");
}

#[test]
fn split_frontmatter_rejects_unclosed_block() {
    let input = "---\nid: IR-1\n\nno closing marker";
    assert!(split_frontmatter(input).is_err());
}
