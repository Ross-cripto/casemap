use std::fmt;
use std::str::FromStr;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Color(String);

#[derive(Debug, thiserror::Error)]
#[error("expected a color as #RGB or #RRGGBB")]
pub struct InvalidColor;

impl FromStr for Color {
    type Err = InvalidColor;

    fn from_str(source: &str) -> Result<Self, Self::Err> {
        let digits = source.strip_prefix('#').ok_or(InvalidColor)?;
        if matches!(digits.len(), 3 | 6) && digits.bytes().all(|digit| digit.is_ascii_hexdigit()) {
            Ok(Self(source.to_owned()))
        } else {
            Err(InvalidColor)
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output.write_str(&self.0)
    }
}

#[derive(Clone, Debug)]
pub struct Theme {
    pub background: Color,
    pub card: Color,
    pub card_border: Color,
    pub line: Color,
    pub focus: Color,
    pub text: Color,
    pub muted: Color,
    pub yes: Color,
    pub no: Color,
    pub added: Color,
    pub changed: Color,
    pub removed: Color,
    pub sections: Vec<Color>,
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            background: Color("#121418".into()),
            card: Color("#1b1e24".into()),
            card_border: Color("#272a31".into()),
            line: Color("#4a4f59".into()),
            focus: Color("#e9eaee".into()),
            text: Color("#f3f4f6".into()),
            muted: Color("#a6adbb".into()),
            yes: Color("#91d6a6".into()),
            no: Color("#f19794".into()),
            added: Color("#91d6a6".into()),
            changed: Color("#f1c47a".into()),
            removed: Color("#9da4b1".into()),
            sections: colors(["#a8bafa", "#78c9c1", "#d5a8e8", "#f1b780", "#8dc5e6"]),
        }
    }

    pub fn light() -> Self {
        Self {
            background: Color("#f7f7fa".into()),
            card: Color("#ffffff".into()),
            card_border: Color("#dfe1e5".into()),
            line: Color("#a9aeb6".into()),
            focus: Color("#16171a".into()),
            text: Color("#20232b".into()),
            muted: Color("#576071".into()),
            yes: Color("#247045".into()),
            no: Color("#aa3737".into()),
            added: Color("#247045".into()),
            changed: Color("#8e570a".into()),
            removed: Color("#6f7684".into()),
            sections: colors(["#495e9d", "#236e67", "#78488e", "#8b5324", "#2c6685"]),
        }
    }

    pub(super) fn section(&self, cluster_index: Option<usize>) -> &Color {
        match cluster_index {
            Some(index) if !self.sections.is_empty() => &self.sections[index % self.sections.len()],
            _ => &self.muted,
        }
    }
}

fn colors<const COUNT: usize>(sources: [&str; COUNT]) -> Vec<Color> {
    sources
        .iter()
        .map(|source| Color((*source).to_owned()))
        .collect()
}
