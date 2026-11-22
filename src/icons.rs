#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    User,
    Square,
    Diamond,
    File,
    FolderOpen,
    Search,
    List,
    GitBranch,
    GitCompare,
    Monitor,
    Terminal,
    Image,
    RefreshCw,
    Clock,
    Check,
    X,
    TriangleAlert,
    CircleHelp,
    Settings,
    Bot,
    Code,
    Eye,
    Scan,
    Map,
    ChartBar,
    MessageSquare,
    ArrowRight,
    Download,
    Bell,
    Calendar,
    CircleDot,
    CreditCard,
    Database,
    FileText,
    Filter,
    Globe,
    House,
    Info,
    KeyRound,
    Link,
    Lock,
    LogIn,
    LogOut,
    Mail,
    Package,
    Pencil,
    Plus,
    Send,
    Share2,
    ShoppingCart,
    Split,
    Star,
    Tag,
    Trash2,
    Upload,
    Users,
    Webhook,
}

pub const LICENSE: &str = include_str!("flow_image/assets/icons/LICENSE");

impl Icon {
    pub const ALL: &[Self] = &[
        Self::User,
        Self::Square,
        Self::Diamond,
        Self::File,
        Self::FolderOpen,
        Self::Search,
        Self::List,
        Self::GitBranch,
        Self::GitCompare,
        Self::Monitor,
        Self::Terminal,
        Self::Image,
        Self::RefreshCw,
        Self::Clock,
        Self::Check,
        Self::X,
        Self::TriangleAlert,
        Self::CircleHelp,
        Self::Settings,
        Self::Bot,
        Self::Code,
        Self::Eye,
        Self::Scan,
        Self::Map,
        Self::ChartBar,
        Self::MessageSquare,
        Self::ArrowRight,
        Self::Download,
        Self::Bell,
        Self::Calendar,
        Self::CircleDot,
        Self::CreditCard,
        Self::Database,
        Self::FileText,
        Self::Filter,
        Self::Globe,
        Self::House,
        Self::Info,
        Self::KeyRound,
        Self::Link,
        Self::Lock,
        Self::LogIn,
        Self::LogOut,
        Self::Mail,
        Self::Package,
        Self::Pencil,
        Self::Plus,
        Self::Send,
        Self::Share2,
        Self::ShoppingCart,
        Self::Split,
        Self::Star,
        Self::Tag,
        Self::Trash2,
        Self::Upload,
        Self::Users,
        Self::Webhook,
    ];

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|icon| icon.as_str() == name)
    }

    pub fn as_str(self) -> &'static str {
        self.definition().0
    }

    pub fn svg_body(self) -> &'static str {
        self.definition().1
    }

    fn definition(self) -> (&'static str, &'static str) {
        match self {
            Self::User => ("user", include_str!("flow_image/assets/icons/user.svg.inc")),
            Self::Square => (
                "square",
                include_str!("flow_image/assets/icons/square.svg.inc"),
            ),
            Self::Diamond => (
                "diamond",
                include_str!("flow_image/assets/icons/diamond.svg.inc"),
            ),
            Self::File => ("file", include_str!("flow_image/assets/icons/file.svg.inc")),
            Self::FolderOpen => (
                "folder-open",
                include_str!("flow_image/assets/icons/folder-open.svg.inc"),
            ),
            Self::Search => (
                "search",
                include_str!("flow_image/assets/icons/search.svg.inc"),
            ),
            Self::List => ("list", include_str!("flow_image/assets/icons/list.svg.inc")),
            Self::GitBranch => (
                "git-branch",
                include_str!("flow_image/assets/icons/git-branch.svg.inc"),
            ),
            Self::GitCompare => (
                "git-compare",
                include_str!("flow_image/assets/icons/git-compare.svg.inc"),
            ),
            Self::Monitor => (
                "monitor",
                include_str!("flow_image/assets/icons/monitor.svg.inc"),
            ),
            Self::Terminal => (
                "terminal",
                include_str!("flow_image/assets/icons/terminal.svg.inc"),
            ),
            Self::Image => (
                "image",
                include_str!("flow_image/assets/icons/image.svg.inc"),
            ),
            Self::RefreshCw => (
                "refresh-cw",
                include_str!("flow_image/assets/icons/refresh-cw.svg.inc"),
            ),
            Self::Clock => (
                "clock",
                include_str!("flow_image/assets/icons/clock.svg.inc"),
            ),
            Self::Check => (
                "check",
                include_str!("flow_image/assets/icons/check.svg.inc"),
            ),
            Self::X => ("x", include_str!("flow_image/assets/icons/x.svg.inc")),
            Self::TriangleAlert => (
                "triangle-alert",
                include_str!("flow_image/assets/icons/triangle-alert.svg.inc"),
            ),
            Self::CircleHelp => (
                "circle-help",
                include_str!("flow_image/assets/icons/circle-help.svg.inc"),
            ),
            Self::Settings => (
                "settings",
                include_str!("flow_image/assets/icons/settings.svg.inc"),
            ),
            Self::Bot => ("bot", include_str!("flow_image/assets/icons/bot.svg.inc")),
            Self::Code => ("code", include_str!("flow_image/assets/icons/code.svg.inc")),
            Self::Eye => ("eye", include_str!("flow_image/assets/icons/eye.svg.inc")),
            Self::Scan => ("scan", include_str!("flow_image/assets/icons/scan.svg.inc")),
            Self::Map => ("map", include_str!("flow_image/assets/icons/map.svg.inc")),
            Self::ChartBar => (
                "chart-bar",
                include_str!("flow_image/assets/icons/chart-bar.svg.inc"),
            ),
            Self::MessageSquare => (
                "message-square",
                include_str!("flow_image/assets/icons/message-square.svg.inc"),
            ),
            Self::ArrowRight => (
                "arrow-right",
                include_str!("flow_image/assets/icons/arrow-right.svg.inc"),
            ),
            Self::Download => (
                "download",
                include_str!("flow_image/assets/icons/download.svg.inc"),
            ),
            Self::Bell => ("bell", include_str!("flow_image/assets/icons/bell.svg.inc")),
            Self::Calendar => (
                "calendar",
                include_str!("flow_image/assets/icons/calendar.svg.inc"),
            ),
            Self::CircleDot => (
                "circle-dot",
                include_str!("flow_image/assets/icons/circle-dot.svg.inc"),
            ),
            Self::CreditCard => (
                "credit-card",
                include_str!("flow_image/assets/icons/credit-card.svg.inc"),
            ),
            Self::Database => (
                "database",
                include_str!("flow_image/assets/icons/database.svg.inc"),
            ),
            Self::FileText => (
                "file-text",
                include_str!("flow_image/assets/icons/file-text.svg.inc"),
            ),
            Self::Filter => (
                "filter",
                include_str!("flow_image/assets/icons/filter.svg.inc"),
            ),
            Self::Globe => (
                "globe",
                include_str!("flow_image/assets/icons/globe.svg.inc"),
            ),
            Self::House => (
                "house",
                include_str!("flow_image/assets/icons/house.svg.inc"),
            ),
            Self::Info => ("info", include_str!("flow_image/assets/icons/info.svg.inc")),
            Self::KeyRound => (
                "key-round",
                include_str!("flow_image/assets/icons/key-round.svg.inc"),
            ),
            Self::Link => ("link", include_str!("flow_image/assets/icons/link.svg.inc")),
            Self::Lock => ("lock", include_str!("flow_image/assets/icons/lock.svg.inc")),
            Self::LogIn => (
                "log-in",
                include_str!("flow_image/assets/icons/log-in.svg.inc"),
            ),
            Self::LogOut => (
                "log-out",
                include_str!("flow_image/assets/icons/log-out.svg.inc"),
            ),
            Self::Mail => ("mail", include_str!("flow_image/assets/icons/mail.svg.inc")),
            Self::Package => (
                "package",
                include_str!("flow_image/assets/icons/package.svg.inc"),
            ),
            Self::Pencil => (
                "pencil",
                include_str!("flow_image/assets/icons/pencil.svg.inc"),
            ),
            Self::Plus => ("plus", include_str!("flow_image/assets/icons/plus.svg.inc")),
            Self::Send => ("send", include_str!("flow_image/assets/icons/send.svg.inc")),
            Self::Share2 => (
                "share-2",
                include_str!("flow_image/assets/icons/share-2.svg.inc"),
            ),
            Self::ShoppingCart => (
                "shopping-cart",
                include_str!("flow_image/assets/icons/shopping-cart.svg.inc"),
            ),
            Self::Split => (
                "split",
                include_str!("flow_image/assets/icons/split.svg.inc"),
            ),
            Self::Star => ("star", include_str!("flow_image/assets/icons/star.svg.inc")),
            Self::Tag => ("tag", include_str!("flow_image/assets/icons/tag.svg.inc")),
            Self::Trash2 => (
                "trash-2",
                include_str!("flow_image/assets/icons/trash-2.svg.inc"),
            ),
            Self::Upload => (
                "upload",
                include_str!("flow_image/assets/icons/upload.svg.inc"),
            ),
            Self::Users => (
                "users",
                include_str!("flow_image/assets/icons/users.svg.inc"),
            ),
            Self::Webhook => (
                "webhook",
                include_str!("flow_image/assets/icons/webhook.svg.inc"),
            ),
        }
    }
}

pub(crate) struct Directive<'source> {
    pub step: &'source str,
    pub icon: Icon,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum DirectiveError {
    #[error("write @icon <step> <name> with one step and one icon name")]
    Syntax,
    #[error("unknown icon {0:?}")]
    UnknownName(String),
    #[error("@icon points at step {0:?}, which this flowchart does not have")]
    UnknownStep(String),
}

pub(crate) fn directive(line: &str) -> Option<Result<Directive<'_>, DirectiveError>> {
    let mut tokens = line.trim().strip_prefix("%%")?.split_whitespace();
    if tokens.next()? != "@icon" {
        return None;
    }
    let (Some(step), Some(name), None) = (tokens.next(), tokens.next(), tokens.next()) else {
        return Some(Err(DirectiveError::Syntax));
    };
    Some(match Icon::parse(name) {
        Some(icon) => Ok(Directive { step, icon }),
        None => Err(DirectiveError::UnknownName(name.to_owned())),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser;

    #[test]
    fn every_icon_has_svg_elements() {
        for &icon in Icon::ALL {
            assert!(
                ["<path", "<circle", "<rect", "<line", "<polyline"]
                    .iter()
                    .any(|element| icon.svg_body().contains(element)),
                "missing SVG elements for {}",
                icon.as_str()
            );
        }
    }

    #[test]
    fn steps_default_to_circle_dot_and_accept_an_explicit_square() {
        let diagram = parser::parse(
            "flowchart TD\nactor([Customer]) --> action[Pay]\n\
             action --> decision{Accepted?}\ndecision --> boxed[Boxed]\n\
             %% @icon boxed square",
        )
        .unwrap();
        let names: Vec<_> = diagram
            .nodes
            .iter()
            .map(|node| node.effective_icon().as_str())
            .collect();

        assert_eq!(names, ["user", "circle-dot", "diamond", "square"]);
    }
}
