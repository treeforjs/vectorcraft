//! Help → project links (this app's GitHub repository and its online guide). The commands return the URL; the
//! frontend opens it.

use serde_json::{Value, json};

use super::*;

/// The app's id on GitHub.
pub const APP_ID: &str = "vectorcraft";

/// This app's source repository.
pub fn github_url() -> String {
    format!("https://github.com/treeforjs/{APP_ID}")
}

/// The online guide: the README on GitHub.
pub fn docs_url() -> String {
    format!("{}#readme", github_url())
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        cmd!(query "help.github", "VectorCraft on GitHub", ["Help"], None, "{} → {url} source code, issues and releases", always, |_, _| url(github_url())),
        cmd!(query "help.docs", "VectorCraft Help…", ["Help"], Some("F1"), "{} → {url} the online guide (the project's README)", always, |_, _| url(docs_url())),
        cmd!(query "help.links", "Links", [], None, "{} → {github}", always, |_, _| Ok(json!({ "github": github_url() }))),
    ]
}

fn url(u: String) -> Result<Value> {
    Ok(json!({ "url": u }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links() {
        let mut s = Session::new();
        assert_eq!(s.execute("help.github", &json!({})).unwrap()["url"], "https://github.com/treeforjs/vectorcraft");
        assert_eq!(s.execute("help.docs", &json!({})).unwrap()["url"], "https://github.com/treeforjs/vectorcraft#readme");
        assert_eq!(s.execute("help.links", &json!({})).unwrap()["github"], "https://github.com/treeforjs/vectorcraft");
    }
}
