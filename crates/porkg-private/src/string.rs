use std::{borrow::Cow, fmt};

use thiserror::Error;

/// Error expanding variables in a string.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("{} at position {}", kind, offset)]
pub struct ExpandError<'a> {
    kind: ExpandErrorKind<'a>,
    offset: usize,
}

impl<'a> ExpandError<'a> {
    fn new(offset: usize, kind: ExpandErrorKind<'a>) -> Self {
        Self { kind, offset }
    }

    pub fn kind(&self) -> &ExpandErrorKind<'a> {
        &self.kind
    }

    pub fn offset(&self) -> usize {
        self.offset
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExpandErrorKind<'a> {
    /// An unknown variable was referenced.
    UnknownVariable { name: Cow<'a, str> },
    /// An incomplete expansion was found.
    IncompleteExpansion,
    /// An invalid escape sequence was found.
    InvalidEscapeSequence,
}

impl fmt::Display for ExpandErrorKind<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ExpandErrorKind::UnknownVariable { name } => write!(f, "unknown variable `{}`", name),
            ExpandErrorKind::IncompleteExpansion => write!(f, "incomplete expansion"),
            ExpandErrorKind::InvalidEscapeSequence => write!(f, "invalid escape sequence"),
        }
    }
}

/// Expand variables in a string.
///
/// Variables are defined as `${name}` and are replaced by the value returned by the `context` function.
pub fn expand<V: AsRef<str>>(
    value: &str,
    context: impl Fn(&str) -> Option<V>,
) -> Result<Cow<str>, ExpandError> {
    if value.is_empty() {
        return Ok(Cow::Borrowed(value));
    }

    let mut result = String::new();
    let mut ofs = 0;

    while ofs < value.len() {
        if let Some(index) = value[ofs..].find('\\') {
            let start = ofs + index;

            let c = match value.as_bytes().get(start + 1) {
                Some(b'$') | Some(b'\\') => value.as_bytes()[start + 1] as char,
                Some(b'n') => '\n',
                Some(b'r') => '\r',
                Some(b't') => '\t',
                _ => {
                    return Err(ExpandError::new(
                        start,
                        ExpandErrorKind::InvalidEscapeSequence,
                    ));
                }
            };

            if result.is_empty() {
                result.reserve(value.len());
            }
            result.push_str(&value[ofs..start]);
            result.push(c);

            ofs = start + 2;
            continue;
        }

        let Some(index) = value[ofs..].find('$') else {
            break;
        };

        if result.is_empty() {
            result.reserve(value.len());
        }

        let start = ofs + index + 1;
        if value.as_bytes().get(start) != Some(&b'{') {
            result.push_str(&value[ofs..start]);
            ofs = start;
            continue;
        }

        result.push_str(&value[ofs..(ofs + index)]);

        let start = start + 1;
        let Some(end) = value[start..].find('}') else {
            return Err(ExpandError::new(
                start,
                ExpandErrorKind::IncompleteExpansion,
            ));
        };

        let end = start + end;
        let name = &value[start..end];
        let Some(value) = context(name) else {
            return Err(ExpandError::new(
                start,
                ExpandErrorKind::UnknownVariable {
                    name: Cow::Borrowed(name),
                },
            ));
        };

        result.push_str(value.as_ref());
        ofs = end + 1;
    }

    if result.is_empty() {
        Ok(Cow::Borrowed(value))
    } else {
        result.push_str(&value[ofs..]);
        Ok(Cow::Owned(result))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_expand() {
        let context = |name: &str| match name {
            "FOO" => Some("foo"),
            "BAR" => Some("bar"),
            _ => None,
        };

        assert_eq!(expand("hello", context), Ok(Cow::Borrowed("hello")));
        assert_eq!(
            expand("hello $FOO", context),
            Ok(Cow::Borrowed("hello $FOO"))
        );
        assert_eq!(
            expand("hello ${FOO}", context),
            Ok(Cow::Borrowed("hello foo"))
        );
        assert_eq!(
            expand("he\\\\ll\\\\o\\n \\${FOO} ${FOO}", context),
            Ok(Cow::Borrowed("he\\ll\\o\n ${FOO} foo"))
        );
        assert_eq!(
            expand("hello \\f", context),
            Err(ExpandError::new(6, ExpandErrorKind::InvalidEscapeSequence))
        );
        assert_eq!(
            expand("hello ${BAZ}", context),
            Err(ExpandError::new(
                8,
                ExpandErrorKind::UnknownVariable {
                    name: Cow::Borrowed("BAZ"),
                },
            ))
        );

        assert_eq!(
            expand("hello ${BAZ", context),
            Err(ExpandError::new(8, ExpandErrorKind::IncompleteExpansion,))
        );
    }
}
