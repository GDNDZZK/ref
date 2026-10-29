//! Site-specific handling for `webfetch`: URLs whose HTML page is mostly chrome around
//! content that is available in a cleaner form.

use reqwest::Url;
use serde_json::Value;

pub enum Special {
    /// Fetch this URL instead (e.g. the raw file behind a GitHub file page).
    Rewrite { url: Url, note: String },
    /// A GitHub issue or pull request, read from the REST API.
    GithubIssue { owner: String, repo: String, number: u64 },
}

pub fn special(url: &Url) -> Option<Special> {
    let host = url.host_str()?.to_ascii_lowercase();
    let seg: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();
    if host == "github.com" || host == "www.github.com" {
        // /<owner>/<repo>/blob/<ref>/<path…> → raw.githubusercontent.com/<owner>/<repo>/<ref>/<path…>
        if seg.len() >= 5 && seg[2] == "blob" {
            // Segments are already percent-encoded.
            let raw = Url::parse(&format!("https://raw.githubusercontent.com/{}/{}/{}", seg[0], seg[1], seg[3..].join("/"))).ok()?;
            return Some(Special::Rewrite { url: raw, note: format!("Raw file behind the GitHub page {url}") });
        }
        if seg.len() == 4 && matches!(seg[2], "issues" | "pull")
            && let Ok(number) = seg[3].parse::<u64>() {
                return Some(Special::GithubIssue { owner: seg[0].into(), repo: seg[1].into(), number });
            }
    }
    None
}

/// API token for GitHub requests (higher rate limit, private repositories), if configured.
pub fn github_token() -> Option<String> {
    ["GITHUB_TOKEN", "GH_TOKEN"].iter().filter_map(|k| std::env::var(k).ok()).find(|t| !t.trim().is_empty())
}

/// Markdown for an issue / pull request from the `issues/<n>` and `issues/<n>/comments` API
/// responses. Returns `(title, markdown)`.
pub fn github_issue_markdown(issue: &Value, comments: &[Value]) -> (String, String) {
    let s = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).unwrap_or("").to_string();
    let user = |v: &Value| v.get("user").map(|u| s(u, "login")).unwrap_or_default();
    let day = |t: String| t.get(..10).map(str::to_string).unwrap_or(t);
    let is_pr = issue.get("pull_request").is_some_and(|p| !p.is_null());
    let number = issue.get("number").and_then(Value::as_u64).unwrap_or(0);
    let title = s(issue, "title");
    let mut state = s(issue, "state");
    if is_pr && issue["pull_request"].get("merged_at").is_some_and(|m| !m.is_null()) {
        state = "merged".into();
    } else if !s(issue, "state_reason").is_empty() && state == "closed" {
        state = format!("closed ({})", s(issue, "state_reason").replace('_', " "));
    }
    let labels: Vec<String> = issue.get("labels").and_then(Value::as_array).map(|a| a.iter().map(|l| s(l, "name")).filter(|n| !n.is_empty()).collect()).unwrap_or_default();
    let mut md = format!("# {title} (#{number})\n\n");
    let mut meta = vec![if is_pr { format!("Pull request, {state}") } else { format!("Issue, {state}") }, format!("by {}", user(issue)), format!("opened {}", day(s(issue, "created_at")))];
    if !labels.is_empty() {
        meta.push(format!("labels: {}", labels.join(", ")));
    }
    if let Some(c) = issue.get("comments").and_then(Value::as_u64) {
        meta.push(format!("{c} comments"));
    }
    md.push_str(&meta.join(" · "));
    md.push_str("\n\n");
    let body = s(issue, "body");
    md.push_str(if body.trim().is_empty() { "(no description)" } else { body.trim() });
    md.push('\n');
    for c in comments {
        md.push_str(&format!("\n---\n\n**{}** · {}\n\n{}\n", user(c), day(s(c, "created_at")), s(c, "body").trim()));
    }
    let total = issue.get("comments").and_then(Value::as_u64).unwrap_or(0) as usize;
    if total > comments.len() {
        md.push_str(&format!("\n---\n\n({} more comments not shown: {}?page=2)\n", total - comments.len(), s(issue, "comments_url")));
    }
    (title, md)
}
