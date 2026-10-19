//! Board addresses people paste: the browser address of a GitLab group or
//! project board, or of a GitHub project. Parsing is strict because the result
//! picks which provider scope the server's own CLI credentials read.
use super::*;

const MAX_ADDRESS: usize = 2048;

fn unsupported() -> Error {
    Error::invalid(
        "Paste the address of a GitLab or GitHub board, for example https://gitlab.com/groups/team/-/boards",
    )
}

fn positive(segment: &str) -> Option<u64> {
    (!segment.is_empty() && segment.bytes().all(|b| b.is_ascii_digit()))
        .then(|| segment.parse::<u64>().ok())
        .flatten()
        .filter(|n| *n > 0)
}

/// The scope a pasted address names. The number is the board it names, or 0
/// when the address only names a group or project (GitLab `…/-/boards`); the
/// caller then lists that scope's boards. A GitLab `id` query parameter picks a
/// board the way the path number does, so it is read and checked; the fragment
/// and the other query parameters carry only view state and are dropped.
/// Anything that is not a plain `https` board address on a bare host name is
/// refused: user names, passwords, ports, encoded or relative path parts, and
/// addresses that name neither provider.
pub(super) fn parse(input: &str) -> Result<BoardSource, Error> {
    let input = input.trim();
    if input.is_empty()
        || input.len() > MAX_ADDRESS
        || input.chars().any(|c| c.is_control() || c.is_whitespace())
        || input.contains('\\')
    {
        return Err(unsupported());
    }
    let url = reqwest::Url::parse(input).map_err(|_| unsupported())?;
    if url.scheme() != "https" {
        return Err(Error::invalid("Board addresses must start with https://"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(Error::invalid(
            "Board addresses cannot contain a user name or password",
        ));
    }
    if url.port().is_some() {
        return Err(Error::invalid(
            "Board addresses with a port are not supported",
        ));
    }
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .filter(|host| safe_host(host))
        .ok_or_else(unsupported)?;
    // The URL parser resolves "." and ".." silently; the typed path must not
    // have contained them, or any escape, in the first place.
    let typed_path = input
        .split_once("://")
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.find('/').map(|start| &rest[start..]))
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    if typed_path.contains('%') || typed_path.split('/').any(|s| s == "." || s == "..") {
        return Err(Error::invalid(
            "Board addresses cannot contain encoded or relative path parts",
        ));
    }
    let mut segments: Vec<&str> = url
        .path_segments()
        .map(|segments| segments.collect())
        .unwrap_or_default();
    if segments.last() == Some(&"") {
        segments.pop();
    }
    if segments.iter().any(|s| s.is_empty()) {
        return Err(unsupported());
    }
    if host == "github.com" || host == "www.github.com" {
        github(&segments)
    } else {
        gitlab(host, &segments, query_id(&url)?)
    }
}

/// The board a GitLab address selects with `?id=`, if it has one. Several
/// different values, or one that is not a positive whole number, are refused
/// instead of being dropped, so the address cannot name another board than the
/// one resolved.
fn query_id(url: &reqwest::Url) -> Result<Option<u64>, Error> {
    let mut selected = None;
    for (key, value) in url.query_pairs() {
        if key != "id" {
            continue;
        }
        let id = positive(&value).ok_or_else(|| {
            Error::invalid("The board number in the address must be a positive whole number")
        })?;
        if selected.is_some_and(|known| known != id) {
            return Err(Error::invalid("The address names more than one board"));
        }
        selected = Some(id);
    }
    Ok(selected)
}

fn github(segments: &[&str]) -> Result<BoardSource, Error> {
    let [kind, owner, "projects", number, rest @ ..] = segments else {
        return Err(Error::invalid(
            "Paste a GitHub project address, for example https://github.com/orgs/team/projects/3",
        ));
    };
    let number = positive(number).ok_or_else(|| {
        Error::invalid("Open the GitHub project and copy its address, which ends in the project number")
    })?;
    let view = match rest {
        [] => true,
        ["views", view] => positive(view).is_some(),
        _ => false,
    };
    if !matches!(*kind, "orgs" | "users") || !safe_segment(owner) || !view {
        return Err(Error::invalid(
            "Paste a GitHub project address, for example https://github.com/orgs/team/projects/3",
        ));
    }
    Ok(BoardSource::Github {
        owner: (*owner).to_owned(),
        number,
        url: String::new(),
    })
}

/// `https://HOST/groups/GROUP/-/boards[/ID]` is a group board and
/// `https://HOST/NAMESPACE/PROJECT/-/boards[/ID]` a project board. Groups and
/// projects can nest, so the path before `/-/` is taken whole; a group address
/// is never read as a project.
fn gitlab(host: String, segments: &[&str], query_id: Option<u64>) -> Result<BoardSource, Error> {
    let Some(marker) = segments.iter().position(|s| *s == "-") else {
        return Err(Error::invalid(
            "Paste the board address from GitLab; it contains /-/boards",
        ));
    };
    let (scope, tail) = segments.split_at(marker);
    let number = match tail {
        ["-", "boards"] => 0,
        ["-", "boards", id] => positive(id).ok_or_else(|| {
            Error::invalid("The board number in the address must be a positive whole number")
        })?,
        _ => {
            return Err(Error::invalid(
                "Paste the board address from GitLab; it contains /-/boards",
            ));
        }
    };
    let number = match (number, query_id) {
        (0, id) => id.unwrap_or(0),
        (path_number, None) => path_number,
        (path_number, Some(id)) if path_number == id => path_number,
        _ => return Err(Error::invalid("The address names more than one board")),
    };
    let (group, path) = match scope {
        ["groups", path @ ..] if !path.is_empty() => (true, path),
        path if path.len() >= 2 && path[0] != "groups" => (false, path),
        _ => {
            return Err(Error::invalid(
                "The address needs a GitLab group or project path before /-/boards",
            ));
        }
    };
    if !path.iter().all(|s| safe_segment(s)) {
        return Err(unsupported());
    }
    Ok(BoardSource::Gitlab {
        host,
        group,
        path: path.join("/"),
        number,
        url: String::new(),
    })
}
