//! Location of a value inside a resource.

use std::fmt;

use crate::labels;

/// One step into a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// Object attribute or map key.
    Attr(String),
    /// List element identified by its key attribute (`rules[https]`).
    Key(String),
    /// List element identified by position.
    Index(usize),
    /// Map key that must not be printed.
    Masked,
}

/// Path from the resource root to a value, e.g. `input.rules[https].port`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Path(Vec<Segment>);

impl Path {
    pub fn push(&mut self, segment: Segment) {
        self.0.push(segment);
    }

    pub fn pop(&mut self) {
        self.0.pop();
    }

    /// Attribute names along the path, skipping list positions.
    pub fn attrs(&self) -> impl Iterator<Item = &str> {
        self.0.iter().filter_map(|segment| match segment {
            Segment::Attr(name) => Some(name.as_str()),
            Segment::Key(_) | Segment::Index(_) | Segment::Masked => None,
        })
    }

    /// Dot-joined attribute names without list positions, as used in config
    /// (`input.rules`).
    pub fn attr_path(&self) -> String {
        self.attrs().collect::<Vec<_>>().join(".")
    }
}

fn is_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (position, segment) in self.0.iter().enumerate() {
            match segment {
                Segment::Attr(name) if is_identifier(name) => {
                    if position > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(name)?;
                }
                // Map keys with dots, spaces or slashes are quoted to stay unambiguous.
                Segment::Attr(name) => write!(f, "[{name:?}]")?,
                Segment::Key(key) => write!(f, "[{key}]")?,
                Segment::Index(index) => write!(f, "[{index}]")?,
                Segment::Masked => write!(f, "[({})]", labels::MASKED)?,
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_like_terraform_references() {
        let mut path = Path::default();
        path.push(Segment::Attr("input".into()));
        path.push(Segment::Attr("rules".into()));
        path.push(Segment::Key("https".into()));
        path.push(Segment::Attr("ports".into()));
        path.push(Segment::Index(2));
        path.push(Segment::Attr("a.b/c".into()));
        path.push(Segment::Masked);
        assert_eq!(
            path.to_string(),
            "input.rules[https].ports[2][\"a.b/c\"][(masked)]"
        );
        assert_eq!(path.attr_path(), "input.rules.ports.a.b/c");
    }
}
