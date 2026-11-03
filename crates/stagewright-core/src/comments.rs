//! Comments on the script, kept in `comments.yaml` beside the scenes.
//!
//! Each comment is a thread pinned to a passage of one scene: the quoted text,
//! a little of what comes before and after it (to find it again if the same
//! words appear twice), and the line it started on when it was last placed.
//! The editor follows the passage as the text changes and stores the new quote
//! when it moves; if the passage is deleted, the comment keeps its last quote
//! and shows as detached. Scene files never contain comments, so exports and
//! other Fountain apps never see them.

use serde::{Deserialize, Serialize};

pub const FILE: &str = "comments.yaml";

const HEADER: &str =
    "# Comments on the script, written by Stagewright. Each is pinned to the\n# quoted text in its scene.\n";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub author: String,
    /// When it was written (ISO 8601, UTC).
    pub at: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edited: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Comment {
    pub id: String,
    /// The scene's id.
    pub scene: String,
    /// The passage commented on.
    pub quote: String,
    /// Up to 40 characters just before and after it.
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    /// Body line (from 0) it started on, as a hint.
    #[serde(default)]
    pub line: usize,
    #[serde(default)]
    pub resolved: bool,
    #[serde(default)]
    pub messages: Vec<Message>,
}

/// Where a comment sits, as the editor last saw it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Anchor {
    pub id: String,
    pub quote: String,
    #[serde(default)]
    pub before: String,
    #[serde(default)]
    pub after: String,
    #[serde(default)]
    pub line: usize,
}

/// One change to the comments. Times and ids come from the caller, so the
/// same change gives the same file on every co-writer's machine.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Op {
    Add {
        id: String,
        scene: String,
        quote: String,
        #[serde(default)]
        before: String,
        #[serde(default)]
        after: String,
        #[serde(default)]
        line: usize,
        author: String,
        at: String,
        text: String,
    },
    Reply {
        id: String,
        author: String,
        at: String,
        text: String,
    },
    Edit {
        id: String,
        index: usize,
        text: String,
        at: String,
    },
    /// Delete one message; deleting the first deletes the whole thread.
    DeleteMessage {
        id: String,
        index: usize,
    },
    Delete {
        id: String,
    },
    Resolve {
        id: String,
        resolved: bool,
    },
    /// The passages moved (the scene was edited): store where they are now.
    Anchor {
        anchors: Vec<Anchor>,
    },
}

pub fn parse(text: &str) -> Result<Vec<Comment>, String> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let v: Option<Vec<Comment>> = serde_yaml::from_str(text).map_err(|e| e.to_string())?;
    Ok(v.unwrap_or_default())
}

pub fn write(comments: &[Comment]) -> String {
    if comments.is_empty() {
        return format!("{HEADER}[]\n");
    }
    format!(
        "{HEADER}{}",
        serde_yaml::to_string(comments).expect("comments serialise")
    )
}

fn find<'a>(list: &'a mut [Comment], id: &str) -> Result<&'a mut Comment, String> {
    list.iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| format!("no comment '{id}' (it may have been deleted)"))
}

fn not_blank(text: &str) -> Result<(), String> {
    if text.trim().is_empty() {
        Err("a comment can't be empty".into())
    } else {
        Ok(())
    }
}

/// Apply one change.
pub fn apply(list: &mut Vec<Comment>, op: Op) -> Result<(), String> {
    match op {
        Op::Add {
            id,
            scene,
            quote,
            before,
            after,
            line,
            author,
            at,
            text,
        } => {
            not_blank(&text)?;
            if quote.is_empty() {
                return Err("select some text to comment on".into());
            }
            if list.iter().any(|c| c.id == id) {
                return Err(format!("a comment '{id}' already exists"));
            }
            list.push(Comment {
                id,
                scene,
                quote,
                before,
                after,
                line,
                resolved: false,
                messages: vec![Message {
                    author,
                    at,
                    text: text.trim_end().to_string(),
                    edited: None,
                }],
            });
        }
        Op::Reply { id, author, at, text } => {
            not_blank(&text)?;
            let c = find(list, &id)?;
            c.messages.push(Message {
                author,
                at,
                text: text.trim_end().to_string(),
                edited: None,
            });
            // Replying reopens a resolved thread, as in Google Docs.
            c.resolved = false;
        }
        Op::Edit { id, index, text, at } => {
            not_blank(&text)?;
            let c = find(list, &id)?;
            let m = c.messages.get_mut(index).ok_or("no such message")?;
            m.text = text.trim_end().to_string();
            m.edited = Some(at);
        }
        Op::DeleteMessage { id, index } => {
            let c = find(list, &id)?;
            if index >= c.messages.len() {
                return Err("no such message".into());
            }
            if index == 0 {
                list.retain(|c| c.id != id);
            } else {
                c.messages.remove(index);
            }
        }
        Op::Delete { id } => {
            find(list, &id)?;
            list.retain(|c| c.id != id);
        }
        Op::Resolve { id, resolved } => find(list, &id)?.resolved = resolved,
        Op::Anchor { anchors } => {
            for a in anchors {
                // A comment deleted meanwhile (by a co-writer) is simply skipped.
                if let Some(c) = list.iter_mut().find(|c| c.id == a.id)
                    && !a.quote.is_empty()
                {
                    c.quote = a.quote;
                    c.before = a.before;
                    c.after = a.after;
                    c.line = a.line;
                }
            }
        }
    }
    Ok(())
}

/// `text` (comments.yaml) with `op` applied.
pub fn edit_text(text: &str, op: Op) -> Result<String, String> {
    let mut list = parse(text)?;
    apply(&mut list, op)?;
    Ok(write(&list))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn add(id: &str, text: &str) -> Op {
        Op::Add {
            id: id.into(),
            scene: "sc-1".into(),
            quote: "a kettle".into(),
            before: "A fire, ".into(),
            after: ", a fiddle".into(),
            line: 0,
            author: "Liam".into(),
            at: "2026-11-03T09:00:00Z".into(),
            text: text.into(),
        }
    }

    #[test]
    fn threads_round_trip() {
        let t = edit_text("", add("cm-1", "Too sparse?")).unwrap();
        assert!(t.starts_with("# Comments"));
        let t = edit_text(
            &t,
            Op::Reply {
                id: "cm-1".into(),
                author: "Marco".into(),
                at: "2026-11-03T09:05:00Z".into(),
                text: "Add a dog.".into(),
            },
        )
        .unwrap();
        let t = edit_text(
            &t,
            Op::Resolve {
                id: "cm-1".into(),
                resolved: true,
            },
        )
        .unwrap();
        let list = parse(&t).unwrap();
        assert_eq!(list.len(), 1);
        assert!(list[0].resolved);
        assert_eq!(list[0].messages[1].author, "Marco");
        // Writing is stable: the same list gives the same text.
        assert_eq!(write(&list), t);
    }

    #[test]
    fn replies_reopen_and_deleting_the_first_message_deletes_the_thread() {
        let mut list = Vec::new();
        apply(&mut list, add("cm-1", "One")).unwrap();
        apply(
            &mut list,
            Op::Resolve {
                id: "cm-1".into(),
                resolved: true,
            },
        )
        .unwrap();
        apply(
            &mut list,
            Op::Reply {
                id: "cm-1".into(),
                author: "Marco".into(),
                at: "t".into(),
                text: "Two".into(),
            },
        )
        .unwrap();
        assert!(!list[0].resolved);
        apply(
            &mut list,
            Op::DeleteMessage {
                id: "cm-1".into(),
                index: 1,
            },
        )
        .unwrap();
        assert_eq!(list[0].messages.len(), 1);
        apply(
            &mut list,
            Op::DeleteMessage {
                id: "cm-1".into(),
                index: 0,
            },
        )
        .unwrap();
        assert!(list.is_empty());
    }

    #[test]
    fn empty_and_missing() {
        let mut list = Vec::new();
        assert!(apply(&mut list, add("cm-1", "  ")).is_err());
        assert!(apply(&mut list, Op::Delete { id: "nope".into() }).is_err());
        // Re-anchoring a comment that's gone is not an error (a co-writer deleted it).
        apply(
            &mut list,
            Op::Anchor {
                anchors: vec![Anchor {
                    id: "gone".into(),
                    quote: "x".into(),
                    before: String::new(),
                    after: String::new(),
                    line: 0,
                }],
            },
        )
        .unwrap();
        assert_eq!(parse("[]\n").unwrap(), vec![]);
        assert_eq!(parse("").unwrap(), vec![]);
    }
}
