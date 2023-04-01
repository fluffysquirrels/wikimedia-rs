use clap::builder::PossibleValue;
use http_cache_reqwest::CacheMode as HttpCacheMode;
use once_cell::sync::Lazy;
use std::{
    collections::BTreeMap,
    ffi::OsStr,
    ops::Deref,
};

#[derive(Clone)]
pub struct HttpCacheModeParser;

const HTTP_CACHE_MODES: &'static [HttpCacheMode] = &[
    HttpCacheMode::Default,
    HttpCacheMode::NoStore,
    HttpCacheMode::Reload,
    HttpCacheMode::NoCache,
    HttpCacheMode::ForceCache,
    HttpCacheMode::OnlyIfCached,
];

static HTTP_CACHE_MODE_MAP: Lazy<BTreeMap<String, HttpCacheMode>> = Lazy::new(|| {
    HTTP_CACHE_MODES.iter()
                    .map(|v| (format!("{:?}", v), v.clone()))
                    .collect::<BTreeMap<String, HttpCacheMode>>()
});

impl clap::builder::TypedValueParser for HttpCacheModeParser {
    type Value = HttpCacheMode;

    fn parse_ref(
        &self,
        _cmd: &clap::Command,
        _arg: Option<&clap::Arg>,
        value: &OsStr,
    ) -> Result<Self::Value, clap::Error> {
        let value_cow = value.to_string_lossy();
        HTTP_CACHE_MODE_MAP.get(value_cow.deref())
            .ok_or_else(|| clap::Error::raw(
                clap::error::ErrorKind::InvalidValue,
                format!("Argument value was not a valid HttpCacheMode value: '{value_cow}'. \
                         Possible values are: {vals}.\n",
                        vals = HTTP_CACHE_MODE_MAP.keys()
                                                  .map(|s| format!("'{s}'"))
                                                  .collect::<Vec<String>>()
                                                  .join(", ")
                                           )))
            .cloned()
    }

    fn possible_values(
        &self
    ) -> Option<Box<dyn Iterator<Item = PossibleValue>>> {
        Some(Box::new(
            HTTP_CACHE_MODE_MAP.keys()
                               .map(|name: &String| {
                                   let clap_str: clap::builder::Str = name.into();
                                   let possible_value: PossibleValue = clap_str.into();
                                   possible_value
                               })
                               .collect::<Vec<PossibleValue>>()
                               .into_iter()
        ))
    }
}
