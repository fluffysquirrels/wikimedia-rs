# To do

## WIP

## Must do before publishing

* Documentation:
    * Documentation comments for all command args
    * Pre-requisites for `bin/publish`
        * cargo install tomato-toml <https://crates.io/crates/tomato-toml>
    * `doc/publish.md` instructions
    * Quick start from zero to web.
    * bin scripts
    * Top level module documentation
    * Architecture (basics of crate and module layout)
    * out-dir layout:

      ```
      out/dumps
      out/dumps/enwiki
      out/dumps/enwiki/20230320
      out/dumps/enwiki/20230320/articlesmultistreamdump
      out/http_cache
      out/store
      out/store/chunks
      out/store/chunks/articles-*.cap
      out/store/chunks/lock
      out/store/chunks/temp
      out/store/index
      out/store/index/index.db
      out/store/index/index.db-shm
      out/store/index/index.db-wal
      ```

    * Logging to JSON, reading with `node-bunyan` or `bunyan-view`
    ```
    CARGO_TERM_QUIET="true" WMD_OUT_DIR="${HOME}/wmd/out/import-2" \
    wmd --log-json import-dump --job-dir ~/wmd/out/job/ --count 10 --clear 2> >(jq '.')
    ```

* `--version latest` should fall back to the previous version if data is missing.
* Update default logging for a good experience out of the box.

* web should support alternative dumps (not just `enwiki`) with the correct URLs.
    * Separate stores for each dump? Or one big store?

* tracing-bunyan-formatter docs.rs config:
  ```toml
  [package.metadata.docs.rs]
  all-features = true
  # enable unstable features in the documentation
  rustdoc-args = ["--cfg", "docsrs", "--cfg", "tokio_unstable"]
  # it's necessary to _also_ pass `--cfg tokio_unstable` to rustc, or else
  # dependencies will not be enabled, and the docs build will fail.
  rustc-args = ["--cfg", "tokio_unstable"]
  ```

* Document that import --limit is approximate.
* Split source into several crates
    * Remove nightly `#![feature()]` use that isn't required.
    * Document crate split in the README.md
* Document bin name (`wmd`), CLI tool crate name (`wikimedia-downloader`),
  crate name (`wikimedia`), repo name (`wikimedia-rs`)
* Support `import-dump` with no `--dump`, `--version`, `--job`?
* Support `cargo install` wikimedia-downloader
    * Mirror selection?
* sqlite error log in tracing https://docs.rs/rusqlite/latest/rusqlite/trace/fn.config_log.html
* wikitext to HTML
    * Test: Batch render all pages.
        * pandoc error during rendering for this page (from dump enwiki/20230301/articlesdump):
          `wmd get-store-page --out html --mediawiki-id 62585868`
          `{
             "ns_id": 0,
             "id": 62585868,
             "title": "Suga's Interlude",
             "revision": {
               "id": 35936988,
             },
           }`

* web
    * 404 page for no route match
    * 404 page for pages by slug should link to enwiki.
    * Error logging for WebError.
    * Browsable
    * Don't show error details to non-local hosts
    * Separate web request log
        * Optional apache format?
        * JSON bunyan or similar

    * PoisonError after panic on todo! in a page handler.
        * Should exit, let the process supervisor restart us.
    * https://github.com/tower-rs/tower-http
    * https://docs.rs/tower-http/latest/tower_http/catch_panic/index.html
    * Error handling
    * category by title should redirect to category url
* Categories
    * web
        * web: add examples to wmd web index /
        * web: page's category links go to the category page
        * web: page/by-name/Category:foo redirects to category/by-name/foo
        * web: list of pages in category.
            * 404 if no pages found.
    * cli: list of categories.
    * cli: list of pages in category.
* Title search with FTS
* Non-unique titles!
* Case insensitive titles
    * Redirect in web when title is not canonical.

* Images  
  Options:
    * On demand (web page render) get download URL from API
    * Batch import all enwiki download URLs from API during import
    * Batch import just the files the pages link to from API during import
    * Batch download all enwiki images during import
        * Possibly re-encode large images to save space
* Clean up old files
    * In http_cache: `find http_cache -type f -mtime +5`
    * In temp directories from crashes and bugs.

## Might do

### Features

* Support compiling without `valuable`? Support compiling without nightly?
* Option to recompress as LZ4 or zstd in Rust.
* Android app
    *  https://developer.android.com/develop/ui/views/layout/webapps/webview#kotlin
    * https://gendignoux.com/blog/2022/10/24/rust-library-android.html#introduction-building-an-android-app-with-the-command-line-tools
    * https://docs.rs/jni/latest/jni/
    * https://crates.io/crates/jnix
    * https://crates.io/crates/android-ndk
    * https://crates.io/crates/catch_panic
    * For completeness: https://docs.rs/rust-jni/latest/rust_jni/
* Maybe: create or document symlinks like I have them
    * out/version -> dumps/enwiki/20230301/
    * out/job -> version/articlesdump/
* Replicate a wikimedia site in semi-real time using API
* Fetch from API on demand
* Batch import from API
* API
    * User agent should include app URL
    * https://m.mediawiki.org/wiki/API:Etiquette#The_maxlag_parameter
    * https://en.m.wikipedia.org/wiki/Special:Statistics
    * https://en.m.wikipedia.org/wiki/Special:MediaStatistics
    * https://meta.m.wikimedia.org/wiki/Wikimedia_Enterprise
    * https://dumps.wikimedia.org/other/enterprise_html/
* Investigate sub-pages. Make sure you can view them in web and links to them work.
* Performance
    * Cache MappedChunk in Arc<>, LRU, something.
    * Split a file on a literal and return it as a rayon paralleliterator, e.g. bz2 or lz4 multistream files, newline delimited text files. optionally include the literal (useful for multistream files). try it on wmd import! try it on csv and jsonl files.
* Improve downloads
    * Set download rate limit
    * Retries
    * Resume partial download
    * Better performance: write while beginning next read
    * Refactor to make it re-usable. Separate crate?
    * Cancellation support
    * Progress bar
        * Crate [`indicatif`](https://crates.io/crates/indicatif) looks good.
            * Breaks when logs are written too.
        * Or progress logs with ETA is probably fine too.
    * Configurable timeout
* Some way to handle multiple stores when we are importing a new version
    * Could be as simple as writing new store to
      `enwiki/{next_version}/store`, then restarting web pointing at
      the new store when it's done
* Handle multiple dumps (i.e. other wikimedia sites) / versions
    * Separate stores per (dump,version)?
* Improve import
    * Restartable / checkpointed / idempotent
        * Skip duplicate pages.
        * Record completed job files, skip them on the next run.
    * One shot download and import, option to keep raw dumps or only
      have one .xml.bz2 on disk during import.
    * daemon or cronjob
    * `<page>` sha1 hash check. It's the text between `<text>HERE</text>`,
      XML entity decoded, then SHA1 summed, then encoded in base 36.
        * `wmd get-dump-page --job-file out/job-multistream/zstd/enwiki-20230320-pages-articles-multistream1.xml-p1p41242.zstd --compression zstd --out json-with-body --limit 1 | jq '.revision.text' -r | head -c -1 | sha1sum | awk '{print $1}'`
        * === `SHA1_BASE36=$(zstdcat out/job-multistream/zstd/enwiki-20230320-pages-articles-multistream1.xml-p1p41242.zstd| head -c 10000 | grep  -P -e  '(?<=<sha1\>)[^<]+(?=\</sha1\>)' -o | head -n1); python3.8 -c "print(hex(int(\"${SHA1_BASE36}\", base=36))[2:])"`
* scheduled work
    * cron or a daemon that has a job scheduler
    * https://crates.io/crates/background-jobs-core
    * https://crates.io/crates/background-jobs
* Dream lazy import:
    * Start the web app, immediately be able to browse by page ID
      using multistream dumps and data file HTTP range requests
    * Index multistream indexes in the background (~ 1 minute)
    * Can now browse or search by page title
    * Start downloading and indexing data files, filling full text
      search, by category indexes. Partial results for category
      listing and FTS might be available during indexing with a
      warning notice that they are incomplete.
    * Finish indexing, all data is available, no warning.
* Read wikimedia multistream dumps
    * get-multistream-* commands
        * Read index files to
          `(index_file_name,
            data_file_name,
            data_stream_offset,
            possibly data_stream_len,
            page_id,
            page_title)`
        * Index this in something searchable by `page_id`, `page_title`
        * Lookup page in multistream data file by page_id.
* get-dump-page subcommand has raw xml output option.
* Images
    * Look at:
        * https://m.mediawiki.org/wiki/API:Allimages
        * https://dumps.wikimedia.org/index.json
        * https://dumps.wikimedia.org/other/wikibase/commonswiki/
        * https://meta.wikimedia.org/wiki/Data_dumps
        * https://meta.wikimedia.org/wiki/Category:Data_dumps
        * imagetable
        * imagelinkstable
* Subcommand to run from cron.
    * Summary at the end.
    * Notifications on success and failure would be great.
    * Log to disk
    * Delete old dump versions when newer ones are complete
        * How to tell when new ones are complete?
          Check names and file sizes, optionally hashes too.
    * Handle it gracefully when:
        *  The status of the job is not "done" (e.g. still in
           progress). At the moment the `download` subcommand just returns
           an Err() with a message, which isn't machine readable. Probably
           return a custom `Error` struct with an `kind: ErrorKind` field.
        *  Downloads fail. Retry automatically after a short delay or next
           time the cronjob runs.
* Render with `pandoc`
    * Rewrite image links
* Clean up temp files on future runs
    * Left from failed downloads
    * Left from failed chunk writes to the store
* Store
    * capnproto orphan API for serialising chunks
    * Add chunk to store metadata, including path, ChunkId,
      count of pages, low page.id, high page.id.
    * async?
    * Race between writing a chunk and committing the index.
        * Keep a chunks WIP table in the index, insert chunk id,
          commit and flush, write the chunk to temp file, move the
          chunk to out dir, in one transaction write the index entries
          for the chunk and remove the chunk_id from the WIP table,
          commit and flush
    * Try compression for chunks: LZ4 or zstd
* store::Index
    * Benchmark
        * Mutex around writer versus send commands to a thread.
    * Support concurrent reads from multiple threads.
        * Add `Index.conn_read_only()`?
        * Use a connection pooling library
            * https://crates.io/crates/mobc
                * Async
                * Uses `metrics` and `tracing`
                * No custom `metrics` labels? Can patch it.
            * https://crates.io/crates/deadpool
                * Async
                * https://crates.io/crates/deadpool-sqlite
                * No max idle time. Hooks to implement it.
            * No, no async: https://crates.io/crates/r2d2
                * rusqlite adapter: https://github.com/ivanceras/r2d2-sqlite
    * Detect Index.conn is dead / errored and reset with a new one.
    * Tune query batch size.
    * Add CLI command to re-index from chunk store.
    * Migrations framework, or at the very least an argument to delete the DB and start from
    * sqlite optimise improvements
        * run at end of import, after clear.
        * PRAGMA optimize; https://www.sqlite.org/pragma.html#pragma_optimize
        * ANALYZE
        * Force WAL checkpoint  
          `pragma wal_checkpoint(TRUNCATE)`  
          https://www.sqlite.org/pragma.html#pragma_wal_checkpoint
        * VACUUM
    * sqlite compilation options
        * https://www.sqlite.org/compile.html#enable_stat4
    * Maybe clear() should delete the files and re-open?
* web
    * Optional: Tower middleware, like rate limiting, concurrency limits
    * Add compression for non-local hosts?
    * TLS? Or instructions to set up a reverse proxy.
    * Typed DRY route building?? Could just regex the path.

### Documentation
* Crate item documentation
* Add brief syntax examples for `--file-name-regex`.

### Telemetry / observability
* Thread id in bunyan logs.
* Display custom tracing values (e.g. Duration) differently in console pretty mode.
* tracing::events for HTTP cache hits and misses, implement CacheManager.
* tracing complex fields logged as JSON rather than Debug
    * Args, others?
* Try tokio console
* Separate web access log
    * Optional apache format?
    * JSON bunyan or similar
* tracing
    * store::index
        * sqlite
            * https://docs.rs/rusqlite/latest/rusqlite/trace/fn.config_log.html
            * https://www.sqlite.org/c3ref/trace_v2.html
            * rusqlite supports trace v1 only, should be easy to patch, can upstream.
            * https://www.sqlite.org/c3ref/stmt_scanstatus.html
            * https://www.sqlite.org/c3ref/total_changes.html
            * Wherever `sqlite3` CLI gets `.stats` from.
            * https://www.sqlite.org/c3ref/total_changes.html
            * https://www.sqlite.org/c3ref/changes.html
            * https://docs.rs/rusqlite/latest/rusqlite/struct.Statement.html#method.get_status
    * axum / tower / tower-http
    * reqwest-tracing
* Metrics
    * Styles
        * Events in a log file
        * Gettable metrics on a service (`web`)
        * Push metrics
            * For jobs (like `wmd import-dump`)
            * aggregates too?
    * https://docs.rs/metrics
    * https://crates.io/crates/metrics-runtime
    * https://docs.rs/metrics-util/0.14.0/metrics_util/
    * https://docs.rs/metrics-tracing-context/0.13.0/metrics_tracing_context/
    * Output
        * Prometheus
            * https://docs.rs/metrics-exporter-prometheus
            * https://prometheus.io/docs/practices/naming/
            * https://prometheus.io/docs/practices/instrumentation/#batch-jobs
            * https://www.robustperception.io/exposing-the-software-version-to-prometheus/
        * sqlite
            * https://docs.rs/metrics-sqlite/latest/metrics_sqlite/  
              Uses diesel to write to the DB, very few / no sqlite options.
        * tracing at end of a cli command
        * tracing every n seconds
        * OpenTelemetry
        * [OpenTSDB](http://opentsdb.net/)
        * Grafana
        * JSON
        * statsd?
            * https://crates.io/crates/metrics-exporter-statsd
    * Sources
        * web
            * Axum: https://crates.io/crates/axum-prometheus
            * Tower?
            * Tower HTTP?
        * reqwest
        * import results
        * store::chunk
            * total size
            * num chunks
            * num pages
        * store::index
            * sqlite
                * https://www.sqlite.org/c3ref/db_status.html
                * https://www.sqlite.org/c3ref/status.html
                * https://www.sqlite.org/c3ref/stmt_status.html
                * number of queries, duration, some indication of percentile cost
                * specific sqlite labels: table? read-only vs read-write?
                * index size in bytes and row count
                * stats views?
                * PRAGMA `page_count`, `page_size`, db bytes (product
                  of `page_count` and `page_size`)
                * https://www.sqlite.org/pragma.html
                * https://www.sqlite.org/dbstat.html
                    * select * from dbstat; -- stats per page
                    * select name, count(*), sum(ncell), sum(payload), sum(unused), sum(pgsize) from dbstat group by name; -- stats by btree
                    * shell snippet to show pretty printed stats by btree  
                      sqlite3 out/store/index/index.db --csv  --header  
                      'select name, count(*), sum(ncell), sum(payload), sum(unused), sum(pgsize) from dbstat group by name;'  
                      | mlr --icsv --opprint cat
                    * List indexes and tables  
                      sqlite3 out/store/index/index.db --csv  --header  
                      'select * from sqlite_master;'  
                      | mlr --icsv --opprint cat
                    * https://www.sqlite.org/sqlanalyze.html
                        * sqlite3_analyzer store/index/index.db
        * store metrics for current state
        * store import batch result metrics
        * HTTP caching

### Code quality

* End to end tests:
    * Download a small dump job file
    * Import the file
    * View a page with `get-store-page` or `web`
* Tidy up store::import(), it's too long.
* Covering indexes for index operations?
* newtype tuple structs
    * MediawikiId
    * NamespaceId
    * PageTitle
    * PageSlug
    * MirrorUrl
* Revisit removing async closures (in http and operations modules)
* Unit tests
    * Dump parsing
    * Wikitext conversion (including sanitisation, categories, weird wikitext pandoc hates)
* Tidy up args to `operations::download_job_file`
* Validate dump name, job name to have no relative paths, path traversal.
* mod dump
    * More fields.
    * `<siteinfo>`
    * Performance
* Use anyhow macros: bail, format_err.
* Split web server and cli tool?
* Unify `get_dump_versions` date validation and `VersionSpecArg` date validation
* Avoid boilerplate to record context variables in `download` subcommand.
    * Perhaps use `tracing::span` to record context variables, with
      events setting their parent to that span
    * Tidy up logging and error handling with some more spans / instrument use / closures
    * E.g. repetition in http module.
* Consider: making `http::{download, metadata}_client()` return different tuple struct
  wrappers to avoid mixing the 2 up.

### Misc

* Look into other sites
    * https://meta.wikimedia.org/wiki/Wikimedia_projects
    * : wiktionary, meta.wikimedia, mediawiki docs, wikisource, wikibooks, wikiquote, wikimedia commons
* https://wikitech.wikimedia.org/wiki/Main_Page
* Wikimedia APIs
    * https://meta.wikimedia.org/wiki/Research:Data
    * https://wikitech.wikimedia.org/wiki/Portal:Data_Services
    * https://wikitech.wikimedia.org/wiki/Help:Cloud_Services_introduction
        * https://wikitech.wikimedia.org/wiki/Help:Toolforge/Kubernetes
        * https://wikitech.wikimedia.org/wiki/Help:Toolforge/Database
    * https://meta.wikimedia.org/wiki/Wikimedia_movement
    * https://en.wikipedia.org/api/rest_v1/
        * `curl --compressed 'https://en.wikipedia.org/api/rest_v1/page/html/The_Matrix'`
    * Look at https://github.com/magnusmanske/mediawiki_rust
    * w/api.php
        * https://www.mediawiki.org/wiki/API:Etiquette
        * https://en.wikipedia.org/wiki/Special:ApiSandbox#action=query&format=rawfm&prop=info&titles=Albert%20Einstein&inprop=url%7Ctalkid
        * https://en.wikipedia.org/wiki/Special:ApiSandbox#action=query&format=json&prop=info%7Cpageimages%7Cpageterms%7Crevisions&indexpageids=1&titles=The%20Matrix&callback=&formatversion=2&inprop=url%7Ctalkid&rvprop=ids%7Ctimestamp%7Cflags%7Ccomment%7Cuser&rvlimit=10
        * `curl 'https://en.wikipedia.org/w/api.php?action=query&format=json&prop=info%7Cpageimages%7Cpageterms%7Crevisions&indexpageids=1&titles=The%20Matrix&callback=&formatversion=2&inprop=url%7Ctalkid&rvprop=ids%7Ctimestamp%7Cflags%7Ccomment%7Cuser&rvlimit=10' -v -H "Accept: application/json"`
    * EventStreams
        * https://wikitech.wikimedia.org/wiki/Event_Platform/EventStreams
        * https://docs.rs/eventstreams/latest/eventstreams/
            * Early POC.
        * `curl -s -H 'Accept: application/json' \
               https://stream.wikimedia.org/v2/stream/recentchange | jq .`
* Wikimedia tools
    * https://github.com/spencermountain/dumpster-dip
    * https://github.com/spencermountain/dumpster-dive
* MediaWiki wikitext
    * https://www.mediawiki.org/wiki/Wikitext
    * https://docs.rs/parse_wiki_text/latest/parse_wiki_text/
    * https://crates.io/crates/mediawiki_parser -- not as complete
    * https://www.mediawiki.org/wiki/Alternative_parsers
* Probably not at this point: bin/build scripts for "release but with symbols"; "release but stripped and lto" -- might be useful, might not.
* More unit testing
* Add parent names to JSON output (e.g. dump name and job name in `FileInfoOutput`)?
* https://crates.io/crates/opendal
* https://github.com/moka-rs/moka : in process cache.
* https://crates.io/crates/woddle : rust job scheduler.
