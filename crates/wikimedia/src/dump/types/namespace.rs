use anyhow::bail;
use crate::Result;
use std::cmp::PartialEq;

#[derive(Clone, Debug, Eq)]
pub struct Namespace {
    key: i32,
    case: Case,
    name: Option<&'static str>,
    talk: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Case {
    FirstLetter,
    CaseSensitive,
}

/// Accessors
impl Namespace {
    pub fn key(&self) -> i32 {
        self.key
    }

    pub fn case(&self) -> Case {
        self.case
    }

    pub fn name_option(&self) -> Option<&'static str> {
        self.name.clone()
    }

    pub fn name(&self) -> &'static str {
        self.name.unwrap_or("Page")
    }

    pub fn talk(&self) -> bool {
        self.talk
    }
}

/// Lookup
impl Namespace {
    pub fn from_key(key: i64) -> Result<Namespace> {
        let ns = match key {
            -2 => Self::MEDIA,
            -1 => Self::SPECIAL,
            0 => Self::PAGE,
            1 => Self::TALK,
            2 => Self::USER,
            3 => Self::USER_TALK,
            4 => Self::WIKIPEDIA,
            5 => Self::WIKIPEDIA_TALK,
            6 => Self::FILE,
            7 => Self::FILE_TALK,
            8 => Self::MEDIAWIKI,
            9 => Self::MEDIAWIKI_TALK,
            10 => Self::TEMPLATE,
            11 => Self::TEMPLATE_TALK,
            12 => Self::HELP,
            13 => Self::HELP_TALK,
            14 => Self::CATEGORY,
            15 => Self::CATEGORY_TALK,
            710 => Self::TIMEDTEXT,
            711 => Self::TIMEDTEXT_TALK,
            828 => Self::MODULE,
            829 => Self::MODULE_TALK,
            2300 => Self::GADGET,
            2301 => Self::GADGET_TALK,
            2302 => Self::GADGET_DEFINITION,
            2303 => Self::GADGET_DEFINITION_TALK,

            _ => bail!("Namespace not found with key {key}"),
        };

        assert_eq!(ns.key as i64, key);
        Ok(ns)
    }

    pub fn from_name(name: Option<&str>) -> Result<Namespace> {
        Ok(match name {
            Some("Media") => Self::MEDIA,
            Some("Special") => Self::SPECIAL,
            Some("Page") | None => Self::PAGE,
            Some("Talk") => Self::TALK,
            Some("User") => Self::USER,
            Some("User talk") => Self::USER_TALK,
            Some("Wikipedia") => Self::WIKIPEDIA,
            Some("Wikipedia talk") => Self::WIKIPEDIA_TALK,
            Some("File") => Self::FILE,
            Some("File talk") => Self::FILE_TALK,
            Some("MediaWiki") => Self::MEDIAWIKI,
            Some("MediaWiki talk") => Self::MEDIAWIKI_TALK,
            Some("Template") => Self::TEMPLATE,
            Some("Template talk") => Self::TEMPLATE_TALK,
            Some("Help") => Self::HELP,
            Some("Help talk") => Self::HELP_TALK,
            Some("Category") => Self::CATEGORY,
            Some("Category talk") => Self::CATEGORY_TALK,
            Some("TimedText") => Self::TIMEDTEXT,
            Some("TimedText talk") => Self::TIMEDTEXT_TALK,
            Some("Module") => Self::MODULE,
            Some("Module talk") => Self::MODULE_TALK,
            Some("Gadget") => Self::GADGET,
            Some("Gadget talk") => Self::GADGET_TALK,
            Some("Gadget definition") => Self::GADGET_DEFINITION,
            Some("Gadget definition talk") => Self::GADGET_DEFINITION_TALK,

            _ => bail!("Namespace not found with name {name:?}"),
        })
    }

    pub fn from_page_slug(slug: &str) -> Result<Namespace> {
        let prefix = match slug.split_once(':') {
            None => None,
            Some((prefix, _)) => Some(prefix.replace('_', " ")),
        };

        let prefix_str = match prefix.as_ref() {
            None => None,
            Some(p) => Some(p.as_str()),
        };

        Self::from_name(prefix_str)
    }

    pub fn from_page_title(title: &str) -> Result<Namespace> {
        let prefix = match title.split_once(':') {
            None => None,
            Some((prefix, _)) => Some(prefix),
        };

        Self::from_name(prefix)
    }
}

/// Instances
impl Namespace {
    pub const MEDIA: Namespace = Namespace {
        key: -2,
        case: Case::FirstLetter,
        name: Some("Media"),
        talk: false,
    };

    pub const SPECIAL: Namespace = Namespace {
        key: -1,
        case: Case::FirstLetter,
        name: Some("Special"),
        talk: false,
    };

    pub const PAGE: Namespace = Namespace {
        key: 0,
        case: Case::FirstLetter,
        name: None,
        talk: false,
    };

    pub const TALK: Namespace = Namespace {
        key: 1,
        case: Case::FirstLetter,
        name: Some("Talk"),
        talk: true,
    };

    pub const USER: Namespace = Namespace {
        key: 2,
        case: Case::FirstLetter,
        name: Some("User"),
        talk: false,
    };

    pub const USER_TALK: Namespace = Namespace {
        key: 3,
        case: Case::FirstLetter,
        name: Some("User talk"),
        talk: true,
    };

    pub const WIKIPEDIA: Namespace = Namespace {
        key: 4,
        case: Case::FirstLetter,
        name: Some("Wikipedia"),
        talk: false,
    };

    pub const WIKIPEDIA_TALK: Namespace = Namespace {
        key: 5,
        case: Case::FirstLetter,
        name: Some("Wikipedia talk"),
        talk: true,
    };

    pub const FILE: Namespace = Namespace {
        key: 6,
        case: Case::FirstLetter,
        name: Some("File"),
        talk: false,
    };

    pub const FILE_TALK: Namespace = Namespace {
        key: 7,
        case: Case::FirstLetter,
        name: Some("File talk"),
        talk: true,
    };

    pub const MEDIAWIKI: Namespace = Namespace {
        key: 8,
        case: Case::FirstLetter,
        name: Some("MediaWiki"),
        talk: false,
    };

    pub const MEDIAWIKI_TALK: Namespace = Namespace {
        key: 9,
        case: Case::FirstLetter,
        name: Some("MediaWiki talk"),
        talk: true,
    };

    pub const TEMPLATE: Namespace = Namespace {
        key: 10,
        case: Case::FirstLetter,
        name: Some("Template"),
        talk: false,
    };

    pub const TEMPLATE_TALK: Namespace = Namespace {
        key: 11,
        case: Case::FirstLetter,
        name: Some("Template talk"),
        talk: true,
    };

    pub const HELP: Namespace = Namespace {
        key: 12,
        case: Case::FirstLetter,
        name: Some("Help"),
        talk: false,
    };

    pub const HELP_TALK: Namespace = Namespace {
        key: 13,
        case: Case::FirstLetter,
        name: Some("Help talk"),
        talk: true,
    };

    pub const CATEGORY: Namespace = Namespace {
        key: 14,
        case: Case::FirstLetter,
        name: Some("Category"),
        talk: false,
    };

    pub const CATEGORY_TALK: Namespace = Namespace {
        key: 15,
        case: Case::FirstLetter,
        name: Some("Category talk"),
        talk: true,
    };

    pub const TIMEDTEXT: Namespace = Namespace {
        key: 710,
        case: Case::FirstLetter,
        name: Some("TimedText"),
        talk: false,
    };

    pub const TIMEDTEXT_TALK: Namespace = Namespace {
        key: 711,
        case: Case::FirstLetter,
        name: Some("TimedText talk"),
        talk: true,
    };

    pub const MODULE: Namespace = Namespace {
        key: 828,
        case: Case::FirstLetter,
        name: Some("Module"),
        talk: false,
    };

    pub const MODULE_TALK: Namespace = Namespace {
        key: 829,
        case: Case::FirstLetter,
        name: Some("Module talk"),
        talk: true,
    };

    pub const GADGET: Namespace = Namespace {
        key: 2300,
        case: Case::CaseSensitive,
        name: Some("Gadget"),
        talk: false,
    };

    pub const GADGET_TALK: Namespace = Namespace {
        key: 2301,
        case: Case::CaseSensitive,
        name: Some("Gadget talk"),
        talk: true,
    };

    pub const GADGET_DEFINITION: Namespace = Namespace {
        key: 2302,
        case: Case::CaseSensitive,
        name: Some("Gadget definition"),
        talk: false,
    };

    pub const GADGET_DEFINITION_TALK: Namespace = Namespace {
        key: 2303,
        case: Case::CaseSensitive,
        name: Some("Gadget definition talk"),
        talk: true,
    };
}

impl PartialEq<Self> for Namespace {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}
