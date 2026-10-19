use super::*;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};

pub(super) fn escaped(s: &str) -> String {
    utf8_percent_encode(s, NON_ALPHANUMERIC).to_string()
}
fn board_url(host: &str, group: bool, path: &str, number: u64) -> String {
    format!(
        "https://{host}/{}{path}/-/boards/{number}",
        if group { "groups/" } else { "" }
    )
}
pub(super) fn rest(
    config: &RuntimeConfig,
    host: &str,
    path: &str,
    method: &str,
    fields: Value,
) -> Result<Value, Error> {
    if !safe_host(host) {
        return Err(Error::invalid("Invalid GitLab host"));
    }
    let mut command = Command::new(&config.glab);
    command.args(["api", "--hostname", host, path, "--method", method]);
    for (key, value) in fields.as_object().unwrap() {
        command
            .arg(if value.is_number() || value.is_boolean() {
                "-F"
            } else {
                "-f"
            })
            .arg(format!(
                "{key}={}",
                value
                    .as_str()
                    .map(str::to_owned)
                    .unwrap_or_else(|| value.to_string())
            ));
    }
    request(command)
}
fn paged(config: &RuntimeConfig, host: &str, path: &str) -> Result<Vec<Value>, Error> {
    let mut result = Vec::new();
    let mut budget = ReadBudget::new();
    for page in 1..=MAX_PAGES {
        let v = rest(
            config,
            host,
            &format!(
                "{path}{}per_page=100&page={page}",
                if path.contains('?') { "&" } else { "?" }
            ),
            "GET",
            json!({}),
        )?;
        let items = v
            .as_array()
            .ok_or_else(|| Error::invalid("GitLab list response must be an array"))?;
        budget.include(&v)?;
        result.extend(items.clone());
        if items.len() < 100 {
            return Ok(result);
        }
    }
    Err(Error::invalid("Excessive GitLab pagination"))
}
pub(super) fn scope(source: &BoardSource) -> (&str, String) {
    let BoardSource::Gitlab {
        host, group, path, ..
    } = source
    else {
        unreachable!()
    };
    (
        host,
        format!(
            "{}/{}",
            if *group { "groups" } else { "projects" },
            escaped(path)
        ),
    )
}
fn access(v: &Value) -> bool {
    let permission = &v["permissions"];
    [
        permission["project_access"]["access_level"].as_u64(),
        permission["group_access"]["access_level"].as_u64(),
        v["access_level"].as_u64(),
    ]
    .into_iter()
    .flatten()
    .any(|a| a >= 30)
}
pub(super) fn can_write(config: &RuntimeConfig, source: &BoardSource) -> Result<bool, Error> {
    let (host, path) = scope(source);
    let v = rest(config, host, &path, "GET", json!({}))?;
    if access(&v) {
        return Ok(true);
    }
    let user = rest(config, host, "user", "GET", json!({}))?;
    let member = rest(
        config,
        host,
        &format!("{path}/members/all/{}", number(&user, "id")?),
        "GET",
        json!({}),
    )?;
    Ok(access(&member))
}
pub(super) fn metadata(config: &RuntimeConfig, source: &BoardSource) -> Result<RemoteBoard, Error> {
    let BoardSource::Gitlab {
        host,
        group,
        path,
        number,
        ..
    } = source
    else {
        unreachable!()
    };
    if *number == 0 {
        return Err(Error::invalid(
            "Select an existing board to discover columns",
        ));
    }
    let (_, base) = scope(source);
    let v = rest(
        config,
        host,
        &format!("{base}/boards/{number}"),
        "GET",
        json!({}),
    )?;
    // Scoped boards cannot be modeled as a raw all-issues mirror. Reject instead of
    // quietly dropping filters (including newer providers' premium scope types).
    for key in [
        "milestone",
        "iteration",
        "iteration_cadence",
        "assignee",
        "weight",
        "labels",
    ] {
        if let Some(value) = v.get(key)
            && !value.is_null()
            && value != &json!([])
            && value != &json!("")
        {
            return Err(Error::invalid(format!(
                "GitLab board scope '{key}' is unsupported; last good board retained"
            )));
        }
    }
    if v["milestone_id"].as_i64().is_some_and(|n| n != 0)
        || v["assignee_id"].as_u64().is_some_and(|n| n != 0)
        || v["weight"].as_i64().is_some_and(|n| n >= 0)
    {
        return Err(Error::invalid("Scoped GitLab boards are unsupported"));
    }
    let mut lists = paged(config, host, &format!("{base}/boards/{number}/lists"))?;
    lists.sort_by_key(|list| list["position"].as_i64().unwrap_or(i64::MAX));
    let mut columns = Vec::new();
    let mut label_lists = Vec::new();
    if v["hide_backlog_list"] != true {
        columns.push(BoardColumn {
            id: "gitlab-open".into(),
            title: "Open".into(),
        });
    }
    for list in lists {
        if list.get("assignee").is_some_and(|v| !v.is_null())
            || list.get("milestone").is_some_and(|v| !v.is_null())
            || list.get("iteration").is_some_and(|v| !v.is_null())
        {
            return Err(Error::invalid(
                "GitLab assignee/milestone/iteration lists are unsupported",
            ));
        }
        if let Some(kind) = list["list_type"].as_str() {
            if kind == "backlog" || kind == "closed" {
                continue;
            }
            if kind != "label" {
                return Err(Error::invalid(format!(
                    "Unsupported GitLab list type: {kind}"
                )));
            }
        }
        let label = text(&list["label"], "name")?;
        let id = format!("gitlab-list-{}", super::number(&list, "id")?);
        columns.push(BoardColumn {
            id: id.clone(),
            title: label.clone(),
        });
        label_lists.push((id, label));
    }
    if v["hide_closed_list"] != true {
        columns.push(BoardColumn {
            id: "gitlab-closed".into(),
            title: "Closed".into(),
        });
    }
    let writable = can_write(config, source).unwrap_or(false); // read-only users can still sync
    Ok(RemoteBoard {
        board: Board {
            id: String::new(),
            project_id: String::new(),
            name: text(&v, "name")?,
            source: BoardSource::Gitlab {
                host: host.clone(),
                group: *group,
                path: path.clone(),
                number: *number,
                url: board_url(host, *group, path, *number),
            },
            columns,
            last_synced_at: None,
            error: None,
        },
        remote_id: number.to_string(),
        status_field: None,
        writable,
        label_lists,
    })
}
pub(super) fn tasks(config: &RuntimeConfig, board: &RemoteBoard) -> Result<Vec<RemoteTask>, Error> {
    let (host, base) = scope(&board.board.source);
    let issues = paged(
        config,
        host,
        &format!("{base}/issues?state=all&with_labels_details=false"),
    )?;
    let mut result = Vec::new();
    for issue in issues {
        let iid = number(&issue, "iid")?;
        let repo = issue_project(&board.board.source, &issue, iid)?;
        let labels: Vec<String> = array(&issue, "labels")?
            .iter()
            .map(|v| {
                v.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| Error::invalid("Invalid GitLab issue label"))
            })
            .collect::<Result<_, _>>()?;
        let columns = memberships(board, &text(&issue, "state")?, &labels);
        if columns.is_empty() {
            continue;
        }
        result.push(RemoteTask {
            reference: IssueRef {
                provider: Provider::Gitlab,
                url: issue_url(host, &repo, iid),
                repository: repo,
                number: iid,
            },
            title: text(&issue, "title")?,
            body: issue["description"].as_str().unwrap_or("").into(),
            labels,
            columns,
            item_id: number(&issue, "id")?.to_string(),
        });
    }
    Ok(result)
}
/// The canonical URL Relay stores for a GitLab issue. GitLab also serves the
/// issue at `/-/work_items/`, but stored references keep this form so tasks
/// keep their identity whichever form GitLab reports.
pub(super) fn issue_url(host: &str, repository: &str, iid: u64) -> String {
    format!("https://{host}/{repository}/-/issues/{iid}")
}
/// Splits a GitLab issue page URL on `host` into its project path and number.
/// Accepts the issue page (`/-/issues/N`) and the work item page that newer
/// GitLab reports for the same issue (`/-/work_items/N`); anything else,
/// including another host, is refused.
pub(super) fn split_issue_url<'a>(host: &str, url: &'a str) -> Option<(&'a str, &'a str)> {
    let path = url
        .strip_prefix("https://")?
        .strip_prefix(host)?
        .strip_prefix('/')?;
    path.split_once("/-/issues/")
        .or_else(|| path.split_once("/-/work_items/"))
        .filter(|(repo, number)| !repo.is_empty() && !number.is_empty())
}
/// The canonical form of a GitLab issue or work item page URL on `host`, or
/// None when it is not one.
pub(super) fn canonical_issue_url(host: &str, url: &str) -> Option<String> {
    let (repo, number) = split_issue_url(host, url)?;
    let number = number.parse::<u64>().ok().filter(|n| *n > 0)?;
    Some(issue_url(host, repo, number))
}
/// The project path of a listed issue, read from its `web_url` and checked
/// against everything else GitLab says about it: the board's host, the
/// issue's own `iid`, `references.full` when present, and the board's scope
/// (the project itself, or a project in the group or its subgroups).
fn issue_project(source: &BoardSource, issue: &Value, iid: u64) -> Result<String, Error> {
    let BoardSource::Gitlab {
        host, group, path, ..
    } = source
    else {
        unreachable!()
    };
    let url = text(issue, "web_url")?;
    let (repo, tail) = split_issue_url(host, &url)
        .ok_or_else(|| Error::invalid("GitLab issue URL does not match board host"))?;
    if tail != iid.to_string() {
        return Err(Error::invalid("GitLab issue URL does not match its number"));
    }
    if let Some(full) = issue["references"]["full"].as_str()
        && full != format!("{repo}#{iid}")
    {
        return Err(Error::invalid(
            "GitLab issue reference does not match its URL",
        ));
    }
    // GitLab paths are case-insensitive; the board path is as the owner typed it.
    let lower = repo.to_ascii_lowercase();
    let scope = path.to_ascii_lowercase();
    let inside = if *group {
        lower.starts_with(&format!("{scope}/"))
    } else {
        lower == scope
    };
    if !inside {
        return Err(Error::invalid("GitLab issue is outside the board scope"));
    }
    Ok(repo.to_owned())
}
fn memberships(board: &RemoteBoard, state: &str, labels: &[String]) -> Vec<String> {
    let mut columns = if state == "closed" {
        vec!["gitlab-closed".into()]
    } else {
        let lists: Vec<_> = board
            .label_lists
            .iter()
            .filter(|(_, label)| labels.contains(label))
            .map(|(id, _)| id.clone())
            .collect();
        if lists.is_empty() {
            vec!["gitlab-open".into()]
        } else {
            lists
        }
    };
    columns.retain(|c| board.board.columns.iter().any(|v| v.id == *c));
    columns
}
pub(super) fn create_board(
    config: &RuntimeConfig,
    source: &BoardSource,
    name: &str,
) -> Result<BoardSource, Error> {
    let (host, base) = scope(source);
    let v = rest(
        config,
        host,
        &format!("{base}/boards"),
        "POST",
        json!({"name":name}),
    )?;
    let BoardSource::Gitlab {
        host, group, path, ..
    } = source
    else {
        unreachable!()
    };
    let number = number(&v, "id")?;
    Ok(BoardSource::Gitlab {
        host: host.clone(),
        group: *group,
        path: path.clone(),
        number,
        url: board_url(host, *group, path, number),
    })
}
pub(super) fn move_task(
    config: &RuntimeConfig,
    board: &RemoteBoard,
    reference: &IssueRef,
    column: &str,
) -> Result<(), Error> {
    let host = reference_host(reference)?;
    let path = format!(
        "projects/{}/issues/{}",
        escaped(&reference.repository),
        reference.number
    );
    let issue = rest(config, &host, &path, "GET", json!({}))?;
    let remove: Vec<_> = board
        .label_lists
        .iter()
        .map(|(_, label)| label.clone())
        .collect();
    // Scoped labels containing comma cannot be represented by GitLab add/remove_labels.
    if remove.iter().any(|s| s.contains(',')) {
        return Err(Error::invalid(
            "GitLab label names containing commas cannot be moved",
        ));
    }
    let mut fields = json!({"state_event":if column == "gitlab-closed" {"close"} else {"reopen"}});
    if column != "gitlab-closed" {
        let labels = array(&issue, "labels")?;
        let remove: Vec<_> = remove
            .iter()
            .filter(|l| labels.iter().any(|v| v.as_str() == Some(l.as_str())))
            .collect();
        if !remove.is_empty() {
            fields["remove_labels"] =
                json!(remove.into_iter().cloned().collect::<Vec<_>>().join(","));
        }
        if let Some((_, label)) = board.label_lists.iter().find(|(id, _)| id == column) {
            fields["add_labels"] = json!(label);
        }
    }
    rest(config, &host, &path, "PUT", fields)?;
    Ok(())
}

/// Every board of a group or project as `(number, name)`, read only.
pub(super) fn boards(
    config: &RuntimeConfig,
    host: &str,
    group: bool,
    path: &str,
) -> Result<Vec<(u64, String)>, Error> {
    let source = BoardSource::Gitlab {
        host: host.into(),
        group,
        path: path.into(),
        number: 0,
        url: String::new(),
    };
    let (_, base) = scope(&source);
    let mut boards: Vec<(u64, String)> = Vec::new();
    for board in paged(config, host, &format!("{base}/boards"))? {
        let id = super::number(&board, "id")?;
        if boards.iter().any(|(known, _)| *known == id) {
            continue;
        }
        let name = board["name"]
            .as_str()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .map_or_else(|| format!("Board {id}"), str::to_owned);
        boards.push((id, name));
    }
    Ok(boards)
}
