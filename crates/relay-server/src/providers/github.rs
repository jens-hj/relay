use super::*;

pub(super) fn rest(
    config: &RuntimeConfig,
    path: &str,
    method: &str,
    fields: Value,
) -> Result<Value, Error> {
    let mut command = Command::new(&config.gh);
    command.args(["api", "--hostname", "github.com", path, "--method", method]);
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
fn graphql(config: &RuntimeConfig, query: &str, variables: Value) -> Result<Value, Error> {
    let mut command = Command::new(&config.gh);
    command
        .args(["api", "--hostname", "github.com", "graphql", "-f"])
        .arg(format!("query={query}"));
    for (key, value) in variables.as_object().unwrap() {
        if !value.is_null() {
            command
                .arg(if value.is_number() { "-F" } else { "-f" })
                .arg(format!(
                    "{key}={}",
                    value
                        .as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| value.to_string())
                ));
        }
    }
    request(command)
}
fn cursor(v: &Value, seen: &mut HashSet<String>) -> Result<Option<String>, Error> {
    match v["pageInfo"]["hasNextPage"].as_bool() {
        Some(false) => Ok(None),
        Some(true) => {
            let next = text(&v["pageInfo"], "endCursor")?;
            if next.is_empty() || !seen.insert(next.clone()) || seen.len() >= MAX_PAGES {
                return Err(Error::invalid("Invalid or excessive GitHub pagination"));
            }
            Ok(Some(next))
        }
        None => Err(Error::invalid("Missing GitHub pagination metadata")),
    }
}
pub(super) fn owner(config: &RuntimeConfig, login: &str) -> Result<(String, String), Error> {
    let v = rest(config, &format!("users/{login}"), "GET", json!({}))?;
    Ok((
        if v["type"] == "Organization" {
            "organization"
        } else {
            "user"
        }
        .into(),
        text(&v, "node_id")?,
    ))
}
pub(super) fn metadata(config: &RuntimeConfig, source: &BoardSource) -> Result<RemoteBoard, Error> {
    let mut budget = ReadBudget::new();
    let BoardSource::Github {
        owner: login,
        number,
        ..
    } = source
    else {
        unreachable!()
    };
    if *number == 0 {
        return Err(Error::invalid(
            "Select an existing destination to discover its columns",
        ));
    }
    let (kind, _) = owner(config, login)?;
    let query = format!(
        "query($owner:String!,$number:Int!){{{kind}(login:$owner){{projectV2(number:$number){{id title url viewerCanUpdate}}}}}}"
    );
    let v = graphql(config, &query, json!({"owner":login,"number":number}))?;
    let p = &v["data"][&kind]["projectV2"];
    let mut remote = RemoteBoard {
        board: Board {
            id: String::new(),
            project_id: String::new(),
            name: text(p, "title")?,
            source: BoardSource::Github {
                owner: login.clone(),
                number: *number,
                url: text(p, "url")?,
            },
            columns: Vec::new(),
            last_synced_at: None,
            error: None,
        },
        remote_id: text(p, "id")?,
        status_field: None,
        writable: p["viewerCanUpdate"].as_bool().unwrap_or(false),
        label_lists: Vec::new(),
    };
    let mut after = None;
    let mut seen = HashSet::new();
    loop {
        let v = graphql(
            config,
            "query($id:ID!,$after:String){node(id:$id){... on ProjectV2{fields(first:100,after:$after){nodes{... on ProjectV2SingleSelectField{id name options{id name}}}pageInfo{hasNextPage endCursor}}}}}",
            json!({"id":remote.remote_id,"after":after}),
        )?;
        let c = &v["data"]["node"]["fields"];
        budget.include(c)?;
        for field in array(c, "nodes")? {
            if field["name"] == "Status" {
                if remote.status_field.is_some() {
                    return Err(Error::invalid("Multiple Status fields are unsupported"));
                }
                remote.status_field = Some(text(field, "id")?);
                for option in array(field, "options")? {
                    remote.board.columns.push(BoardColumn {
                        id: text(option, "id")?,
                        title: text(option, "name")?,
                    });
                }
            }
        }
        after = cursor(c, &mut seen)?;
        if after.is_none() {
            break;
        }
    }
    remote.board.columns.push(BoardColumn {
        id: NO_STATUS.into(),
        title: "No status".into(),
    });
    Ok(remote)
}
pub(super) fn tasks(config: &RuntimeConfig, board: &RemoteBoard) -> Result<Vec<RemoteTask>, Error> {
    let mut budget = ReadBudget::new();
    let mut after = None;
    let mut seen = HashSet::new();
    let mut result = Vec::new();
    loop {
        let v = graphql(
            config,
            "query($id:ID!,$after:String){node(id:$id){... on ProjectV2{items(first:100,after:$after){nodes{id type content{__typename ... on Issue{id number title body url repository{nameWithOwner}}}fieldValueByName(name:\"Status\"){... on ProjectV2ItemFieldSingleSelectValue{optionId}}}pageInfo{hasNextPage endCursor}}}}}",
            json!({"id":board.remote_id,"after":after}),
        )?;
        let c = &v["data"]["node"]["items"];
        budget.include(c)?;
        for item in array(c, "nodes")? {
            let issue = &item["content"];
            if item["type"] == "ISSUE" && issue["__typename"] != "Issue" {
                return Err(Error::invalid(
                    "GitHub board contains inaccessible issues; last good board retained",
                ));
            }
            if item["type"] != "ISSUE" {
                continue;
            }
            let column = item["fieldValueByName"]["optionId"]
                .as_str()
                .unwrap_or(NO_STATUS)
                .to_owned();
            validate_column(board, &column)?;
            let mut labels = Vec::new();
            let mut label_after = None;
            let mut label_seen = HashSet::new();
            loop {
                let v = graphql(
                    config,
                    "query($id:ID!,$after:String){node(id:$id){... on Issue{labels(first:100,after:$after){nodes{name}pageInfo{hasNextPage endCursor}}}}}",
                    json!({"id":text(issue,"id")?,"after":label_after}),
                )?;
                let c = &v["data"]["node"]["labels"];
                budget.include(c)?;
                for label in array(c, "nodes")? {
                    labels.push(text(label, "name")?);
                }
                label_after = cursor(c, &mut label_seen)?;
                if label_after.is_none() {
                    break;
                }
            }
            result.push(RemoteTask {
                reference: IssueRef {
                    provider: Provider::Github,
                    repository: text(&issue["repository"], "nameWithOwner")?,
                    number: number(issue, "number")?,
                    url: text(issue, "url")?,
                },
                title: text(issue, "title")?,
                body: text(issue, "body")?,
                labels,
                columns: vec![column],
                item_id: text(item, "id")?,
            });
        }
        after = cursor(c, &mut seen)?;
        if after.is_none() {
            break;
        }
    }
    Ok(result)
}
pub(super) fn create_board(
    config: &RuntimeConfig,
    source: &BoardSource,
    name: &str,
) -> Result<BoardSource, Error> {
    let BoardSource::Github { owner: login, .. } = source else {
        unreachable!()
    };
    let (_, owner_id) = owner(config, login)?;
    let v = graphql(
        config,
        "mutation($owner:ID!,$title:String!){createProjectV2(input:{ownerId:$owner,title:$title}){projectV2{id number url}}}",
        json!({"owner":owner_id,"title":name}),
    )?;
    let p = &v["data"]["createProjectV2"]["projectV2"];
    Ok(BoardSource::Github {
        owner: login.clone(),
        number: number(p, "number")?,
        url: text(p, "url")?,
    })
}
pub(super) fn add(
    config: &RuntimeConfig,
    board: &RemoteBoard,
    reference: &IssueRef,
) -> Result<String, Error> {
    let issue = rest(
        config,
        &format!("repos/{}/issues/{}", reference.repository, reference.number),
        "GET",
        json!({}),
    )?;
    let v = graphql(
        config,
        "mutation($project:ID!,$issue:ID!){addProjectV2ItemById(input:{projectId:$project,contentId:$issue}){item{id}}}",
        json!({"project":board.remote_id,"issue":text(&issue,"node_id")?}),
    )?;
    text(&v["data"]["addProjectV2ItemById"]["item"], "id")
}
pub(super) fn move_task(
    config: &RuntimeConfig,
    board: &RemoteBoard,
    item: &str,
    column: &str,
) -> Result<(), Error> {
    if column == NO_STATUS && board.status_field.is_none() {
        return Ok(());
    }
    let field = board
        .status_field
        .as_ref()
        .ok_or_else(|| Error::invalid("Board has no Status field"))?;
    let (value, mutation) = if column == NO_STATUS {
        (
            graphql(
                config,
                "mutation($project:ID!,$item:ID!,$field:ID!){clearProjectV2ItemFieldValue(input:{projectId:$project,itemId:$item,fieldId:$field}){projectV2Item{id}}}",
                json!({"project":board.remote_id,"item":item,"field":field}),
            )?,
            "clearProjectV2ItemFieldValue",
        )
    } else {
        (
            graphql(
                config,
                "mutation($project:ID!,$item:ID!,$field:ID!,$option:String!){updateProjectV2ItemFieldValue(input:{projectId:$project,itemId:$item,fieldId:$field,value:{singleSelectOptionId:$option}}){projectV2Item{id}}}",
                json!({"project":board.remote_id,"item":item,"field":field,"option":column}),
            )?,
            "updateProjectV2ItemFieldValue",
        )
    };
    if text(&value["data"][mutation]["projectV2Item"], "id")? != item {
        return Err(Error::invalid(
            "GitHub status mutation returned a different item",
        ));
    }
    Ok(())
}

/// GitHub exposes no general Projects v2 creation permission probe. Authorize
/// self-owned destinations or organization administrators conservatively; other
/// members can publish to an existing board with viewerCanUpdate=true.
pub(super) fn check_creation(config: &RuntimeConfig, login: &str) -> Result<(), Error> {
    let (kind, _) = owner(config, login)?;
    let viewer = rest(config, "user", "GET", json!({}))?;
    if kind == "user" {
        if viewer["login"]
            .as_str()
            .is_some_and(|s| s.eq_ignore_ascii_case(login))
        {
            return Ok(());
        }
    } else {
        let member = rest(
            config,
            &format!("user/memberships/orgs/{login}"),
            "GET",
            json!({}),
        )?;
        if member["state"] == "active" && member["role"] == "admin" {
            return Ok(());
        }
    }
    Err(Error::invalid(
        "New GitHub boards require the authenticated owner or organization administrator; select an existing destination otherwise",
    ))
}
