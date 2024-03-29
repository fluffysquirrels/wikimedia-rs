use anyhow::{bail, Context, format_err};
use crate::{
    dump::{self, CategoryName},
    Result,
    TempDir,
};
use std::{
    fs,
    path::Path,
    time::{Duration, Instant},
};
use tokio::io::AsyncWriteExt;

pub async fn convert_page_to_html(
    page: &dump::Page,
    dump_name: &dump::DumpName,
    out_dir: &Path,
) -> Result<String> {

    let pandoc_start = Instant::now();

    let temp_dir = TempDir::create(out_dir, /* keep: */ false)?;

    // Write Lua filter

    // TODO: Escape these as a Lua string literal.
    let dump_name = &*dump_name.0;
    let page_by_title = format!("/{dump_name}/page/by-title/");
    let category_by_name = format!("/{dump_name}/category/by-name/");

    let lua_filter = format!(
        r##"
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
        "##);
    let lua_filter_path = temp_dir.path()?.join("filter.lua");
    fs::write(&*lua_filter_path, lua_filter.as_bytes())?;

    // Write template
    let template_path = temp_dir.path()?.join("template.html");
    const TEMPLATE: &'static str =
        r#"
$if(toc)$
  <nav id="$idprefix$TOC" role="doc-toc">

    <h2 id="$idprefix$toc-title">Table of contents</h2>

    $table-of-contents$
  </nav>
$endif$

$body$
        "#;
    fs::write(&*template_path, TEMPLATE.as_bytes())?;

    let wikitext = page.revision_text().unwrap_or("");

    let wikitext = escape_templates(wikitext);

    let mut child =
        tokio::process::Command::new("pandoc")
            .args(&[
                "--from", "mediawiki",
                "--to", "html",
                "--sandbox",
                "--standalone",
                "--template", &*template_path.to_string_lossy(),
                "--id-prefix", "wikitext-",
                "--toc",
                "--number-sections",
                "--number-offset", "1",
                "--lua-filter", &*lua_filter_path.to_string_lossy(),
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .context("While starting pandoc. Is it installed and on your path?")?;

    let mut child_stdin =
        child.stdin.take().ok_or(format_err!("Failed to open stdin"))?;

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

    tracing::debug!(duration = ?pandoc_duration, "Pandoc completed");

    let html = String::from_utf8_lossy(&*child_out.stdout);

    tracing::trace!(pandoc_output_html = &*html, "Pandoc output HTML");

    let sanitised =
        ammonia::Builder::default()
            .url_schemes(maplit::hashset![
                "http", "https", "mailto"
            ])
            .link_rel(Some("noopener noreferrer nofollow"))
            .add_tag_attributes("a" , &["id"])
            .add_tag_attributes("h1", &["id"])
            .add_tag_attributes("h2", &["id"])
            .add_tag_attributes("h3", &["id"])
            .add_tag_attributes("h4", &["id"])
            .add_tag_attributes("h5", &["id"])
            .add_tag_attributes("h6", &["id"])
            .add_tag_attributes("li", &["id"])
            .clean(&*html)
            .to_string();

    tracing::trace!(ammonia_output_html = sanitised, "ammonia output HTML");

    Ok(sanitised)
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

fn escape_templates(wikitext: &str) -> String {
    fn replacer<'t>(caps: &regex::Captures<'t>) -> String {
        let inner = caps.get(0).expect("regex capture 0").as_str();
        let inner = html_escape::encode_text(inner);
        format!("<pre>{inner}</pre>")
    }

    // TODO: This doesn't handle nested template invocations.
    lazy_regex!(r#"\{\{[^}]+\}\}"#).replace_all(wikitext, replacer).to_string()
}

#[cfg(test)]
mod tests {
    use super::escape_templates;

    #[test]
    fn escape_templates_cases() {
        let cases: &[(&str, &str)] = [
            ("", ""),
            ("asdf", "asdf"),
            ("{{a}}", "<pre>{{a}}</pre>"),
            ("{{<pre>a</pre>}}", "<pre>{{&lt;pre&gt;a&lt;/pre&gt;}}</pre>"),
        ].as_slice();

        for (input, expected) in cases.into_iter() {
            let out = escape_templates(input);
            println!("\nCase:\n\
                      |   in:       '{input}'\n\
                      |   out:      '{out}'\n\
                      |   expected: '{expected}'\n");
            assert_eq!(out, *expected);
        }
    }
}
