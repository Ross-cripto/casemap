use crate::Mark;

pub(crate) fn bold_line(text: &str, color: bool) -> String {
    if color {
        format!("\x1b[1m{text}\x1b[0m\n")
    } else {
        format!("{text}\n")
    }
}

pub(crate) fn mark_sgr(mark: Mark) -> Option<&'static str> {
    match mark {
        Mark::Same => None,
        Mark::Added => Some("\x1b[32m"),
        Mark::Removed => Some("\x1b[31m"),
        Mark::Changed => Some("\x1b[33m"),
    }
}
