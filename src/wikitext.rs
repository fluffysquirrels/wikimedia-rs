use crate::{
    article_dump,
    args::CommonArgs,
    Result,
    slug,
    TempDir,
};
use std::{
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

pub async fn convert_page_to_html(
    common_args: &CommonArgs,
    page: &article_dump::Page,
) -> Result<Vec<u8>> {

    let pandoc_start = Instant::now();

    let temp_dir = TempDir::create(&*common_args.out_dir, /* keep: */ false)?;

    // Write Lua filter
    const LUA_FILTER: &'static str =
        "
            function Link(el)
                local target = el.target
                if string.find(target, \"^http\") == nil then
                    target = \"https://en.wikipedia.org/wiki/\" .. el.target
                end
                return pandoc.Link(el.content, target)
            end
        ";
    let lua_filter_path = temp_dir.path()?.join("filter.lua");
    std::fs::write(&*lua_filter_path, LUA_FILTER.as_bytes())?;

    // Write header suffix
    let header_suffix_path = temp_dir.path()?.join("header_suffix.html");
    const HEADER_SUFFIX: &'static str =
        "
            <style>
                a.header-links { color: #55f }
            </style>
        ";
    std::fs::write(&*header_suffix_path, HEADER_SUFFIX.as_bytes())?;

    // Write body prefix
    let page_slug = slug::page_title_to_slug(&*page.title);
    let enwiki_link = format!("https://en.wikipedia.org/wiki/{page_slug}");
    let enwiki_href = html_escape::encode_double_quoted_attribute(&*enwiki_link);
    let id_link = format!("/enwiki/page/by-id/{page_id}", page_id = page.id);
    let id_href = html_escape::encode_double_quoted_attribute(&*id_link);
    let title_link = format!("/enwiki/page/by-title/{page_slug}");
    let title_href = html_escape::encode_double_quoted_attribute(&*title_link);
    let body_prefix = format!(
        r#"
           <p><a class="header-links" href="{enwiki_href}">This page on enwiki</a></p>
           <p><a class="header-links" href="{id_href}">This page by MediaWiki ID</a></p>
           <p><a class="header-links" href="{title_href}">This page by title</a></p>
        "#);
    let body_prefix_path = temp_dir.path()?.join("body_prefix.html");
    std::fs::write(&*body_prefix_path, body_prefix.as_bytes())?;

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
            "--metadata", &*format!("title:{}", page.title.replace('\'', "_")),
            "--lua-filter", &*lua_filter_path.to_string_lossy(),
            "--include-in-header", &*header_suffix_path.to_string_lossy(),
            "--include-before-body", &*body_prefix_path.to_string_lossy(),
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let mut child_stdin =
        child.stdin.take().ok_or(anyhow::Error::msg("Failed to open stdin"))?;

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
        return Err(anyhow::Error::msg(
            format!("Error exit code running pandoc code={code} stdout='{stdout}' \
                     stderr='{stderr}'",
                    code = child_out.status,
                    stdout = String::from_utf8_lossy(&child_out.stdout),
                    stderr = String::from_utf8_lossy(&child_out.stderr))));
    }

    tracing::debug!(duration = ?pandoc_duration, "Converted wikitext to HTML");

    Ok(child_out.stdout)
}
