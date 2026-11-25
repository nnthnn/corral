//! Minimal Linear GraphQL client for looking up tickets and attaching their
//! bodies to an agent session.
//!
//! Deliberately small: two read-only queries (search + a single issue) over
//! Linear's public GraphQL endpoint, authenticated with a personal API key.
//! The key is passed in by the caller (from `Settings.linear_api_key`) so this
//! module has no opinion about where it's stored.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::error::{Error, Result};

const ENDPOINT: &str = "https://api.linear.app/graphql";

/// Summary row returned by [`search`] — enough for a result list without
/// pulling whole ticket bodies for every hit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearIssueSummary {
    pub id: String,
    pub identifier: String,
    pub title: String,
    pub url: String,
    /// Workflow-state name (e.g. "In Progress"), when Linear returned one.
    pub state: Option<String>,
    /// Team name, when Linear returned one.
    pub team: Option<String>,
}

/// A full ticket, including its description, ready to be formatted into an
/// attachment for the agent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LinearIssue {
    pub id: String,
    pub identifier: String,
    pub title: String,
    /// Markdown body. `None` when the ticket has no description.
    pub description: Option<String>,
    pub url: String,
    pub state: Option<String>,
    pub team: Option<String>,
    pub assignee: Option<String>,
    pub priority: Option<String>,
    pub labels: Vec<String>,
}

impl LinearIssue {
    fn summary(&self) -> LinearIssueSummary {
        LinearIssueSummary {
            id: self.id.clone(),
            identifier: self.identifier.clone(),
            title: self.title.clone(),
            url: self.url.clone(),
            state: self.state.clone(),
            team: self.team.clone(),
        }
    }
}

const SEARCH_QUERY: &str = r#"
query CorralSearchIssues($term: String!, $first: Int!) {
  searchIssues(term: $term, first: $first) {
    nodes {
      id
      identifier
      title
      url
      state { name }
      team { name }
    }
  }
}
"#;

const ISSUE_QUERY: &str = r#"
query CorralIssue($id: String!) {
  issue(id: $id) {
    id
    identifier
    title
    description
    url
    state { name }
    team { name }
    assignee { name }
    priorityLabel
    labels { nodes { name } }
  }
}
"#;

/// Shared agent for both calls. Like the opencode HTTP helpers, bound connect
/// and response-read time so a wedged request can't hang a command forever.
fn http_agent() -> &'static ureq::Agent {
    static AGENT: std::sync::OnceLock<ureq::Agent> = std::sync::OnceLock::new();
    AGENT.get_or_init(|| {
        ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_recv_response(Some(Duration::from_secs(20)))
                .build(),
        )
    })
}

/// Whether `query` looks like a Linear issue identifier (`ENG-123`) rather
/// than free text. Team keys are alphanumeric and the numeric half is digits
/// only, so `auth-bug` (a plausible search term) is correctly *not* treated as
/// an identifier. Split out for tests; exact-identifier lookups use
/// `issue(id:)` because `searchIssues` is full-text and can miss identifiers.
fn is_issue_identifier(query: &str) -> bool {
    let Some((key, number)) = query.split_once('-') else {
        return false;
    };
    !key.is_empty()
        && key.len() <= 16
        && key.chars().all(|c| c.is_ascii_alphanumeric())
        && !number.is_empty()
        && number.chars().all(|c| c.is_ascii_digit())
}

fn str_at(value: &Value, pointer: &str) -> Option<String> {
    value
        .pointer(pointer)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn parse_summary(node: &Value) -> Option<LinearIssueSummary> {
    Some(LinearIssueSummary {
        id: str_at(node, "/id")?,
        identifier: str_at(node, "/identifier")?,
        title: str_at(node, "/title")?,
        url: str_at(node, "/url").unwrap_or_default(),
        state: str_at(node, "/state/name"),
        team: str_at(node, "/team/name"),
    })
}

fn parse_issue(node: &Value) -> Option<LinearIssue> {
    Some(LinearIssue {
        id: str_at(node, "/id")?,
        identifier: str_at(node, "/identifier")?,
        title: str_at(node, "/title")?,
        description: str_at(node, "/description"),
        url: str_at(node, "/url").unwrap_or_default(),
        state: str_at(node, "/state/name"),
        team: str_at(node, "/team/name"),
        assignee: str_at(node, "/assignee/name"),
        priority: str_at(node, "/priorityLabel"),
        labels: node
            .pointer("/labels/nodes")
            .and_then(Value::as_array)
            .map(|labels| labels.iter().filter_map(|l| str_at(l, "/name")).collect())
            .unwrap_or_default(),
    })
}

/// Linear reports GraphQL failures with HTTP 200 and a top-level `errors`
/// array, so they have to be checked explicitly rather than relying on the
/// HTTP status.
fn graphql_error_message(body: &Value) -> Option<String> {
    let errors = body.get("errors")?.as_array()?;
    let messages: Vec<&str> = errors
        .iter()
        .filter_map(|e| e.get("message").and_then(Value::as_str))
        .collect();
    (!messages.is_empty()).then(|| messages.join("; "))
}

fn graphql(api_key: &str, query: &str, variables: Value) -> Result<Value> {
    let response = http_agent()
        .post(ENDPOINT)
        .header("Authorization", api_key)
        .header("Accept", "application/json")
        .send_json(json!({ "query": query, "variables": variables }));

    let mut response = match response {
        Ok(response) => response,
        Err(ureq::Error::StatusCode(401)) => {
            return Err(Error::Other(
                "Linear rejected the API key (401). Check it under Settings → Linear.".into(),
            ));
        }
        Err(ureq::Error::StatusCode(code)) => {
            return Err(Error::Other(format!("Linear returned HTTP {code}.")));
        }
        Err(e) => {
            return Err(Error::Other(format!("could not reach Linear: {e}")));
        }
    };

    let body: Value = response
        .body_mut()
        .read_json()
        .map_err(|e| Error::Other(format!("Linear returned an unreadable response: {e}")))?;
    if let Some(message) = graphql_error_message(&body) {
        return Err(Error::Other(format!("Linear API error: {message}")));
    }
    Ok(body)
}

fn require_key(api_key: &str) -> Result<&str> {
    let key = api_key.trim();
    if key.is_empty() {
        return Err(Error::Other(
            "No Linear API key is set. Add one under Settings → Linear.".into(),
        ));
    }
    Ok(key)
}

/// Look up a single issue by identifier (or UUID). `Ok(None)` when Linear
/// returns no such issue.
fn issue_optional(api_key: &str, id: &str) -> Result<Option<LinearIssue>> {
    let body = graphql(api_key, ISSUE_QUERY, json!({ "id": id.trim() }))?;
    let Some(node) = body.pointer("/data/issue").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    parse_issue(node)
        .map(Some)
        .ok_or_else(|| Error::Other("Linear returned an unexpected issue shape.".into()))
}

/// Fetch a single issue. Errors when Linear has no such issue, with a message
/// suitable for surfacing directly to the user.
pub fn issue(api_key: &str, id: &str) -> Result<LinearIssue> {
    let key = require_key(api_key)?;
    issue_optional(key, id)?
        .ok_or_else(|| Error::Other(format!("Linear issue {id:?} was not found.")))
}

/// Search Linear issues by identifier or free text. An exact identifier is
/// tried first (via `issue(id:)`), then full-text search, so typing `ENG-123`
/// works even though `searchIssues` may not index identifiers.
pub fn search(api_key: &str, query: &str, limit: u32) -> Result<Vec<LinearIssueSummary>> {
    let key = require_key(api_key)?;
    let query = query.trim();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    if is_issue_identifier(query)
        && let Some(issue) = issue_optional(key, query)?
    {
        return Ok(vec![issue.summary()]);
    }

    let body = graphql(
        key,
        SEARCH_QUERY,
        json!({ "term": query, "first": limit.clamp(1, 50) }),
    )?;
    Ok(body
        .pointer("/data/searchIssues/nodes")
        .and_then(Value::as_array)
        .map(|nodes| nodes.iter().filter_map(parse_summary).collect())
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_issue_identifiers_but_not_free_text() {
        assert!(is_issue_identifier("ENG-123"));
        assert!(is_issue_identifier("ABC2-7"));
        assert!(!is_issue_identifier("auth-bug"));
        assert!(!is_issue_identifier("ENG-"));
        assert!(!is_issue_identifier("-123"));
        assert!(!is_issue_identifier("ENG-12a"));
        assert!(!is_issue_identifier("plain text"));
    }

    #[test]
    fn parses_a_search_summary() {
        let node: Value = serde_json::from_str(
            r#"{
                "id": "uuid-1",
                "identifier": "ENG-123",
                "title": "Fix login timeout",
                "url": "https://linear.app/acme/issue/ENG-123",
                "state": { "name": "In Progress" },
                "team": { "name": "Engineering" }
            }"#,
        )
        .unwrap();
        let summary = parse_summary(&node).unwrap();
        assert_eq!(summary.identifier, "ENG-123");
        assert_eq!(summary.state.as_deref(), Some("In Progress"));
        assert_eq!(summary.team.as_deref(), Some("Engineering"));
    }

    #[test]
    fn parses_a_full_issue() {
        let node: Value = serde_json::from_str(
            r#"{
                "id": "uuid-1",
                "identifier": "ENG-123",
                "title": "Fix login timeout",
                "description": "Sessions expire too early.\n\nRepro steps...",
                "url": "https://linear.app/acme/issue/ENG-123",
                "state": { "name": "Todo" },
                "team": { "name": "Engineering" },
                "assignee": { "name": "Ada" },
                "priorityLabel": "High",
                "labels": { "nodes": [{ "name": "bug" }, { "name": "auth" }] }
            }"#,
        )
        .unwrap();
        let issue = parse_issue(&node).unwrap();
        assert_eq!(issue.assignee.as_deref(), Some("Ada"));
        assert_eq!(issue.priority.as_deref(), Some("High"));
        assert_eq!(issue.labels, vec!["bug", "auth"]);
        assert!(issue.description.unwrap().contains("Repro steps"));
    }

    #[test]
    fn issue_without_description_parses_as_none() {
        let node: Value = serde_json::from_str(
            r#"{"id": "u", "identifier": "ENG-1", "title": "T", "description": null,
                "labels": { "nodes": [] }}"#,
        )
        .unwrap();
        assert_eq!(parse_issue(&node).unwrap().description, None);
    }

    #[test]
    fn surfaces_graphql_errors_from_a_200_response() {
        let body: Value = serde_json::from_str(
            r#"{"errors":[{"message":"Entity not found"},{"message":"second"}]}"#,
        )
        .unwrap();
        assert_eq!(
            graphql_error_message(&body).as_deref(),
            Some("Entity not found; second")
        );
        assert!(graphql_error_message(&json!({"data": {}})).is_none());
    }

    #[test]
    fn empty_api_key_fails_without_a_network_call() {
        let err = search("  ", "ENG-123", 20).unwrap_err().to_string();
        assert!(err.contains("No Linear API key"), "{err}");
        let err = issue("", "ENG-123").unwrap_err().to_string();
        assert!(err.contains("No Linear API key"), "{err}");
    }

    #[test]
    fn empty_query_returns_no_results() {
        // `require_key` runs first, so pass a non-empty key; the empty-query
        // short-circuit must happen before any network call.
        assert!(search("lin_api_test", "   ", 20).unwrap().is_empty());
    }
}
