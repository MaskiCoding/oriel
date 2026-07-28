//! The README's config example is the first thing a reader copies. Parse it
//! with the real schema so it cannot drift away from the code.

fn readme_toml() -> String {
    let readme = include_str!("../README.md");
    let start = readme.find("```toml").expect("README has a toml block") + "```toml".len();
    let rest = &readme[start..];
    let end = rest.find("```").expect("toml block is closed");
    rest[..end].to_string()
}

#[test]
fn readme_example_parses_with_the_real_schema() {
    let cfg = config::parse(&readme_toml()).expect("README config example must parse");
    assert_eq!(cfg.lenses.len(), 2, "example should show two lenses");
    assert_eq!(cfg.lenses[0].trigger, "cmd+tab");
    assert_eq!(cfg.lenses[1].trigger, "alt+tab");
    assert_eq!(cfg.rules.len(), 2, "example should show two rules");
}

/// Every `[section]` the shipped first-run file writes must appear in the
/// README, or the documented schema is quietly incomplete.
#[test]
fn readme_documents_every_shipped_section() {
    let readme = include_str!("../README.md");
    let shipped = config::to_toml(&config::Config::default());
    for line in shipped.lines() {
        let line = line.trim();
        if line.starts_with('[') && line.ends_with(']') {
            assert!(
                readme.contains(line),
                "README does not document the {line} section"
            );
        }
    }
}
