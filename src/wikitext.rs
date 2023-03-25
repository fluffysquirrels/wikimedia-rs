use anyhow::{bail, format_err};
use crate::{
    args::CommonArgs,
    dump::{self, CategoryName},
    Result,
    slug,
    store::StorePageId,
    TempDir,
};
use std::{
    fs,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

pub async fn convert_page_to_html(
    common_args: &CommonArgs,
    page: &dump::Page,
    store_page_id: Option<StorePageId>,
) -> Result<Vec<u8>> {

    let pandoc_start = Instant::now();

    let temp_dir = TempDir::create(&*common_args.out_dir, /* keep: */ false)?;

    // Write Lua filter

    // TODO; Encode this as a Lua string literal.
    let page_by_title: &str = "/enwiki/page/by-title/";
    let category_by_name: &str = "/enwiki/category/by-name/";
    let enwiki_page_by_title: &str = "https://en.wikipedia.org/wiki/";
    let lua_filter = format!(
        r#"
            function Link(el)
                local target = el.target
                if string.find(target, "^http") ~= nil then
                    -- nothing to do for http(s) links.
                elseif string.find(target, "^Category:") ~= nil then
                    -- internal link for category page
                    local name = string.gsub(target, "Category:", "", 1)
                    target = "{category_by_name}" .. name
                else
                    -- internal link for regular page
                    target = "{page_by_title}" .. el.target
                end
                return pandoc.Link(el.content, target)
            end
        "#);
    let lua_filter_path = temp_dir.path()?.join("filter.lua");
    fs::write(&*lua_filter_path, lua_filter.as_bytes())?;

    // Write header suffix
    let header_suffix_path = temp_dir.path()?.join("header_suffix.html");
    const HEADER_SUFFIX: &'static str =
        "
            <style>
                a.header-links { color: #55f }
            </style>
        ";
    fs::write(&*header_suffix_path, HEADER_SUFFIX.as_bytes())?;

    // Write body prefix
    let page_slug = slug::title_to_slug(&*page.title);

    fn link_html(text: &str, url: &str) -> String {
        let href = html_escape::encode_double_quoted_attribute(url);
        let text = html_escape::encode_safe(text);
        format!(r#"<p><a class="header-links" href="{href}">{text}</a></p>"#)
    }

    let links_html = [
        link_html("This page on enwiki", &*format!("{enwiki_page_by_title}{page_slug}")),
        link_html("This page by MediaWiki ID",
                  &*format!("/enwiki/page/by-id/{page_id}", page_id = page.id)),
        link_html("This page by title", &*format!("{page_by_title}{page_slug}")),
        store_page_id
            .map(|id|
                 link_html("This page by page store ID",
                           &*format!("/enwiki/page/by-store-id/{id}")))
            .unwrap_or("".to_string()),
        ].join("\n");

    // Left as format!() for when I add extra stuff soon.
    let body_prefix = format!("{links_html}");

    let body_prefix_path = temp_dir.path()?.join("body_prefix.html");
    fs::write(&*body_prefix_path, body_prefix.as_bytes())?;

    let html_title = html_escape::encode_safe(&*page.title).to_string();

    let mut child =
        tokio::process::Command::new("pandoc")
        .args(&[
            "--from", "mediawiki",
            "--to", "html",
            "--sandbox",
            "--standalone",
            "--toc",
            "--number-sections",
            "--number-offset", "1",
            "--metadata", &*format!("pagetitle:{} | wmd", html_title), // goes in <head><title>
            "--metadata", &*format!("title:{}", html_title), // goes in <h1>
            "--lua-filter", &*lua_filter_path.to_string_lossy(),
            "--include-in-header", &*header_suffix_path.to_string_lossy(),
            "--include-before-body", &*body_prefix_path.to_string_lossy(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let mut child_stdin =
        child.stdin.take().ok_or(format_err!("Failed to open stdin"))?;

    let wikitext = page.revision.as_ref()
        .and_then(|r| r.text.as_ref())
        .map(|t| t.as_str())
        .unwrap_or("");

    child_stdin.write_all(wikitext.as_bytes()).await?;
    drop(child_stdin); // Closes child's stdin so it will read EOF.

    // TODO: Collect stderr manually to print on timeout.

    let child_out = child.wait_with_output();
    let child_out = tokio::time::timeout(Duration::from_secs(5), child_out);
    let child_out = child_out.await??;
    let pandoc_duration = pandoc_start.elapsed();
    if !child_out.status.success() {
        bail!("Error exit code running pandoc code={code} stdout='{stdout}' \
               stderr='{stderr}'",
              code = child_out.status,
              stdout = String::from_utf8_lossy(&child_out.stdout),
              stderr = String::from_utf8_lossy(&child_out.stderr));
    }

    tracing::debug!(duration = ?pandoc_duration, "Converted wikitext to HTML");

    Ok(child_out.stdout)
}

pub fn parse_categories(
    wikitext: &str
) -> Vec<CategoryName> {
    let mut vec = lazy_regex!(r#"\[\[Category:([^\]]+)\]\]"#).captures_iter(wikitext)
        .map(|captures| {
            let name = captures.get(1).expect("capture group 1").as_str().to_string();
            CategoryName(name)
        })
        .collect::<Vec<CategoryName>>();
    vec.sort();
    vec.dedup();
    vec
}
