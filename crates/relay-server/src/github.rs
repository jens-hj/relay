//! Read-only Projects v2 synchronization. gh supplies authentication; no shell or token handling.
use crate::{Error, RuntimeConfig};
use relay_core::*;
use serde_json::{Value, json};
use std::{collections::HashSet, process::Command};

fn graphql(config: &RuntimeConfig, query: &str, variables: Value) -> Result<Value, Error> {
    let mut cmd = Command::new(&config.gh);
    cmd.env_remove("RELAY_TOKEN");
    cmd.args(["api", "--hostname", "github.com", "graphql", "-f"])
        .arg(format!("query={query}"));
    for (key, value) in variables.as_object().unwrap() {
        if !value.is_null() {
            cmd.arg(if value.is_number() { "-F" } else { "-f" })
                .arg(format!(
                    "{key}={}",
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                ));
        }
    }
    let output = cmd.output().map_err(|_| {
        Error::invalid("Cannot execute gh; install gh and authenticate with gh auth login")
    })?;
    if !output.status.success() {
        return Err(Error::invalid(
            "GitHub request failed; check gh authentication, project read access, and network",
        ));
    }
    let value: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| Error::invalid("Invalid GitHub JSON response"))?;
    if value.get("errors").is_some() {
        return Err(Error::invalid(
            "GitHub GraphQL rejected the request; check project access and configuration",
        ));
    }
    Ok(value)
}
fn string(v: &Value, key: &str) -> Result<String, Error> {
    v[key]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| Error::invalid(format!("GitHub response missing {key}")))
}
fn cursor(connection: &Value, seen: &mut HashSet<String>) -> Result<Option<String>, Error> {
    if !connection["pageInfo"]["hasNextPage"]
        .as_bool()
        .ok_or_else(|| Error::invalid("GitHub pagination metadata missing"))?
    {
        return Ok(None);
    }
    let next = string(&connection["pageInfo"], "endCursor")?;
    if !seen.insert(next.clone()) {
        return Err(Error::invalid(
            "GitHub returned a repeated pagination cursor",
        ));
    }
    Ok(Some(next))
}
#[derive(Debug)]
pub(crate) struct Board {
    pub title: String,
    pub url: String,
    pub columns: Vec<BoardColumn>,
    pub issues: Vec<Issue>,
}
pub(crate) fn sync(config: &RuntimeConfig, project_id: &str) -> Result<Board, Error> {
    let remote = config.remote.as_ref().ok_or_else(|| Error::invalid("Configure RELAY_GITHUB_REPO, RELAY_GITHUB_PROJECT_OWNER and RELAY_GITHUB_PROJECT_NUMBER"))?;
    // GitHub emits NOT_FOUND for the owner kind that does not exist; resolve owner first.
    let owner = Command::new(&config.gh)
        .env_remove("RELAY_TOKEN")
        .args([
            "api",
            "--hostname",
            "github.com",
            &format!("users/{}", remote.owner),
        ])
        .output()
        .map_err(|_| Error::invalid("Cannot execute gh; install gh and authenticate"))?;
    if !owner.status.success() {
        return Err(Error::invalid(
            "GitHub owner lookup failed; check gh authentication and project owner",
        ));
    }
    let owner: Value = serde_json::from_slice(&owner.stdout)
        .map_err(|_| Error::invalid("Invalid GitHub owner response"))?;
    let kind = if owner["type"] == "Organization" {
        "organization"
    } else {
        "user"
    };
    let query = format!(
        "query($owner:String!,$number:Int!){{{kind}(login:$owner){{projectV2(number:$number){{id title url}}}}}}"
    );
    let value = graphql(
        config,
        &query,
        json!({"owner":remote.owner,"number":remote.number}),
    )?;
    let project = &value["data"][kind]["projectV2"];
    let id = string(project, "id")?;
    let mut board = Board {
        title: string(project, "title")?,
        url: string(project, "url")?,
        columns: vec![],
        issues: vec![],
    };
    let mut after = None;
    let mut seen = HashSet::new();
    loop {
        let v = graphql(
            config,
            "query($id:ID!,$after:String){node(id:$id){... on ProjectV2{fields(first:100,after:$after){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}pageInfo{hasNextPage endCursor}}}}}",
            json!({"id":id,"after":after}),
        )?;
        let c = &v["data"]["node"]["fields"];
        for field in c["nodes"]
            .as_array()
            .ok_or_else(|| Error::invalid("GitHub fields missing"))?
        {
            if field["name"] == "Status" {
                for option in field["options"]
                    .as_array()
                    .ok_or_else(|| Error::invalid("GitHub Status options missing"))?
                {
                    board.columns.push(BoardColumn {
                        id: string(option, "id")?,
                        title: string(option, "name")?,
                    });
                }
            }
        }
        after = cursor(c, &mut seen)?;
        if after.is_none() {
            break;
        }
    }
    board.columns.push(BoardColumn {
        id: "github-no-status".into(),
        title: "No status".into(),
    });
    after = None;
    seen.clear();
    loop {
        let v = graphql(
            config,
            "query($id:ID!,$after:String){node(id:$id){... on ProjectV2{items(first:100,after:$after){nodes{type content{__typename ... on Issue{number title body url repository{nameWithOwner} labels(first:100){nodes{name}}}} fieldValueByName(name:\"Status\"){... on ProjectV2ItemFieldSingleSelectValue{optionId}}}pageInfo{hasNextPage endCursor}}}}}",
            json!({"id":id,"after":after}),
        )?;
        let c = &v["data"]["node"]["items"];
        for item in c["nodes"]
            .as_array()
            .ok_or_else(|| Error::invalid("GitHub items missing"))?
        {
            let content = &item["content"];
            if item["type"] != "ISSUE"
                || content["__typename"] != "Issue"
                || content["repository"]["nameWithOwner"].as_str()
                    != Some(remote.repository.as_str())
            {
                continue;
            }
            let number = content["number"]
                .as_u64()
                .ok_or_else(|| Error::invalid("GitHub issue number missing"))?;
            let column = item["fieldValueByName"]["optionId"]
                .as_str()
                .unwrap_or("github-no-status")
                .to_owned();
            if !board.columns.iter().any(|c| c.id == column) {
                return Err(Error::invalid("GitHub issue has an unknown Status option"));
            }
            board.issues.push(Issue {
                id: format!("github:{}:{number}", remote.repository),
                project_id: project_id.into(),
                reference: IssueRef {
                    provider: Provider::Github,
                    repository: remote.repository.clone(),
                    number,
                    url: string(content, "url")?,
                },
                title: string(content, "title")?,
                body: string(content, "body")?,
                column_id: column,
                labels: content["labels"]["nodes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l["name"].as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                result: None,
            });
        }
        after = cursor(c, &mut seen)?;
        if after.is_none() {
            break;
        }
    }
    Ok(board)
}
