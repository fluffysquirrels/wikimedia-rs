# To do

## Must do before publishing

* Document
    * Pre-requisites for build and run.
        * docker
        * pandoc
    * Platform
    * Architecture
    * Logging to JSON, reading with `node-bunyan` or `bunyan-view`
* import-dump
    * do it all
    * in parallel
    * while downloading article dumps
    * For later: daemon
    * search indices in RocksDB or sqlite? LMDB?
        * tantivy: https://lib.rs/crates/tantivy
        * https://lib.rs/crates/sonic-server
* Web server
    * `/{dump}/page/by-id/{wikimedia_id}`, e.g. `/enwiki/page/by-id/30007` for "The Matrix"
    * `/{dump}/page/by-title/{wikimedia_title}` e.g. `/enwiki/page/by-title/The_Matrix`
    * `/{dump}/page/by-store-id/{store_page_id}` e.g. `/enwiki/page/by-store-id/1.2`

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

* Tidy up args to `operations::download_job_file`
* Validate dump name, job name to have no relative paths, path traversal.
* mod article_dump
    * More fields.
    * `<siteinfo>`
    * Performance
* Render with `pandoc`
    * Rewrite image links
    * Save lua filter to a file, reference it by path.
    * TODO: Sanitise HTML
* Clean up temp files on future runs
    * Left from failed downloads
    * Left from failed chunk writes to the store
* Flatbuffers
    * capnproto vs flatbuffers
    * capnproto capabilities
* Page Store
    * Locking
    * Chunk list
    * Compression
        * LZ4
            * decompression multiple times faster than snappy
            * https://github.com/PSeitz/lz4_flex faster than lz4_fear, optionally unsafe
            * `lz4_fear`: use master branch, has a bugfix.  
              https://github.com/main--/rust-lz-fear  
              https://docs.rs/lz-fear/latest/lz_fear/
                * LZ4-HC not supported
            * https://github.com/lz4/lz4/blob/dev/doc/lz4_Frame_format.md
            * Pre-trained dictionary compression
            * snappy vs lz4: https://stackoverflow.com/a/67537112/94819
            * C lib: https://lz4.github.io/lz4/
        * snappy
            * https://docs.rs/snap/latest/snap/
            * https://github.com/google/snappy/blob/main/framing_format.txt
        * zstd
            * pure rust decompressor: https://github.com/KillingSpark/zstd-rs  
              3.5x slower than original C++ implementation
            * rust bindings for C++ lib: https://crates.io/crates/zstd
        * lzo
            * https://crates.io/crates/rust-lzo
            * some bindings
* Stores
    * RocksDB
    * https://crates.io/crates/marble
* Save default out path on first use to config file.
* Full text search
    * List of engines: https://gist.github.com/manigandham/58320ddb24fed654b57b4ba22aceae25
    * Rust
        * https://docs.rs/tantivy/latest/tantivy/
            * https://docs.rs/summavy/latest/summavy/index.html
        * https://github.com/quickwit-oss/quickwit
        * https://github.com/mosuka/bayard
        * https://github.com/toshi-search/Toshi
        * https://github.com/valeriansaliou/sonic
    * Manticore Search: https://github.com/manticoresoftware/manticoresearch
        * C++
        * https://github.com/manticoresoftware/manticoresearch/blob/master/doc/internals-index-format.md
        * https://manual.manticoresearch.com/Creating_a_table/Data_types#Row-wise-and-columnar-attribute-storages
        * https://github.com/manticoresoftware/columnar
        * https://manticoresearch.com/blog/manticore-alternative-to-elasticsearch/
            * Manticore parallelises queries automatically to a single shard, ES does not.
            * Lower write latency
            * Ingestion is 2x faster
            * Starts up faster
    * MySQL
    * Postgres
    * sqlite
    * http://www.sphinxsearch.co/
    * Solr
    * https://stackoverflow.com/questions/1284083/choosing-a-stand-alone-full-text-search-server-sphinx-or-solr/1297561#1297561
    * OpenSearch / ElasticSearch
    * ClickHouse
    * https://github.com/typesense/typesense
    * Datasets
        * https://archive.org/details/stackexchange
            * https://meta.stackexchange.com/questions/2677/database-schema-documentation-for-the-public-data-dump-and-sede
        * https://zenodo.org/record/45901
        * https://github.com/HackerNews/API
        * bigquery datasets https://cloud.google.com/bigquery/public-data
        * https://console.cloud.google.com/bigquery?p=bigquery-public-data&d=samples&page=dataset
        * https://aws.amazon.com/opendata/?wwps-cards.sort-by=item.additionalFields.sortDate&wwps-cards.sort-order=desc
        * https://aws.amazon.com/marketplace/pp/prodview-zxtb4t54iqjmy?sr=0-1&ref_=beagle&applicationId=AWSMPContessa
        * https://commoncrawl.org/
        * https://skeptric.com/common-crawl-index-athena/
        * https://github.com/awslabs/open-data-registry/tree/main/datasets
        * https://registry.opendata.aws/

## Might do

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
* bin/build scripts for "release but with symbols"; "release but stripped and lto" -- might be useful, might not.
* Consider: making `http::{download, metadata}_client()` return different tuple struct
  wrappers to avoid mixing the 2 up.
* Cache metadata downloads
    * Log cache hits and misses, implement CacheManager.
* More unit testing
* Logging to JSON
    * Possibly: Support logging both pretty format to stderr and JSON to a file
    * Document `bunyan` support with `bunyan-view`.
    * Or find / write a pretty printer for the tracing-subscriber JSON stream
        * [bunyan-view in Rust](https://github.com/dekobon/bunyan-view)
        * https://crates.io/crates/tracing-bunyan-formatter
    * `thread_name` `thread_id`.
    * `function_name!` macro: <https://docs.rs/function_name/latest/function_name/>
* tracing complex fields logged as JSON rather than Debug?
* Tidy up logging and error handling with some more spans / instrument use / closures
    * E.g. repetition in http module.
* Document shell completion script setup.
* Add brief syntax hints for `--file-name-regex`.
* Improve downloads
    * Set download rate limit
    * Retries
    * Resume partial download
    * Better performance: write while beginning next read
    * Refactor to make it re-usable. Separate crate?
    * Cancellation support
    * Progress bar
        * Crate [`indicatif`](https://crates.io/crates/indicatif) looks good.
    * Configurable timeout
* Add parent names to JSON output (e.g. dump name and job name in `FileInfoOutput`)?
* https://crates.io/crates/reqwest-tracing
* Separate `clap` arg definitions from value types, e.g. create new DumpName, JobName tuple structs
    * Separates concerns, creates potential for non-CLI uses.
* Unify `get_dump_versions` date validation and `VersionSpecArg` date validation
* Reading xml dumps
    * `xml-rs` looks ok https://docs.rs/xml-rs/latest/xml/index.html
    * `quick-xml` looks ok https://docs.rs/quick-xml/latest/quick_xml/
* Some kind of indexed lookup
    * bzip2 is very slow (12MB/s on my laptop)
    * Decompressed data would be faster to use, but 4-5x the size, so
      require maybe 80GB of disk space
    * Useful crates:
        * [`bzip2`](https://crates.io/crates/bzip2): Rust bindings to `libbz2`
            * Includes a single and a multi archive decoder.
        * [`flate2`](https://crates.io/crates/flate2) by default is pure Rust,
          supports deflate, zlib, gzip compression algorithms.
        * [`xz2`](https://crates.io/crates/xz2): Rust bindings to `liblzma`.
        * [`zstd`](https://crates.io/crates/zstd): Rust bindings to `zstd`.
    * The archives are stored in page ID order, but I'm more likely to want
      to fetch an article by page title.
    * Multistream wikipedia dumps work like this:
        * Take like 100 pages at a time ordered by page ID (an integer).
          Each page is encoded as XML.
        * Compress them into separate bzip2 subarchives
        * Concatenate the result.
          bzip2 CLI still supports decompressing the whole file
        * There is an index file from page id to subarchive offset and page title
        * Complete lookup process is:
            * Decompress the index file
              (optionally store it in a nicer format for faster seeking (it's sorted by page id))
            * From the page ID or page title lookup the byte offset of
              the subarchive in the index file
            * Decompress the subarchive in RAM (probably only a few MB
              of data, also it can be streamed)
            * Parse the XML of all the articles in the subarchive one
              by one, and return the one with the correct page ID or title.
              (if using a streaming XML parser and bzip2 decompressor,
              you can stop as soon as you see the correct one to avoid wasted work)
            * OR perhaps a binary search of the decompressed subarchive would be faster?
              You could avoid parsing the XML of some of the files.
    * Multistream wikipedia archives are not available from my local
      mirror in Europe
    * ZIM format doesn't look so bad, there's even a Rust crate to load them
        * They offer Wikimedia dumps as downloads, but they're old.
          Could have a subcommand for them.
        * Links:
            * [Rust crate `zim`](https://crates.io/crates/zim)
                * Read-only
            * <https://wiki.openzim.org/wiki/OpenZIM>
            * <https://wiki.openzim.org/wiki/ZIM_file_format>
            * Kiwix is an offline ZIM file reader, multi-platform including desktop OSes,
              Android, and a web server  
              <https://en.wikipedia.org/wiki/Kiwix>
    * I could implement something with similar goals and methods to the multistream dumps or ZIM
        * Take some number of pages, compress them concatenated in
          small-ish chunks (1MB? 10MB?)
        * Store the chunks. Options:
            *  One per file (might be inefficient)
            *  In some small number of files; either concatenated with
               an index file somewhere else, or in some seekable
               format  
               (easier to read/write in parallel than just one big file)
            *  In some seekable format in one file
        * Store indexes by page ID and page title to the chunk ID / offsets,
          and an offset in the decompressed chunk
        * Write or use something existing?
        * Index could be in sqlite, Postgres, sled, some other embedded KV store
        * Will an existing blob store work?
* Look at [XOWA](http://xowa.org/), offline viewer that reads Wikimedia database dumps
    * https://github.com/gnosygnu/xowa
    * Java
    * Last release 2021-01, downloadable files are old ~2016
* Avoid boilerplate to record context variables in `download` subcommand.
    * Perhaps use `tracing::span` to record context variables, with
      events setting their parent to that span

## Notes

* Cache metadata downloads
    * Save to `./out/cache`
    * Existing libraries
        * Archived: https://docs.rs/reqwest-middleware-cache/latest/reqwest_middleware_cache/
        * New hotness: https://github.com/06chaynes/http-cache
            * No body streaming
            * Serialises responses with bincode, don't think it's backwards compatible
            * Still fine for small metadata
        * This crate implements HTTP caching rules in Rust:  
          https://crates.io/crates/http-cache-semantics
            * I found the complete implementations by looking at its
              reverse dependencies on crates.io.
    * Discard old cached files, options:
        * Using standard HTTP headers
            * Save `ETag` and `Last-Modified` headers from response  
              `https://dumps.wikimedia.org` sends these for `dumpstatus.json`.  
              `https://mirror.accum.se/mirror/wikimedia.org/dumps` sends `Last-Modified`
              for `dumpstatus.json`.
                * Probably save in the same file as the response:
                    * Use WARC: https://commoncrawl.org/the-data/get-started/#WARC-Format  
                      There's a great-looking crate [`warc`](https://crates.io/crates/warc)
                    * Custom, as a prefix
                    * Or using another standard serialisation library, like JSON
            * Send `If-Modified-Since` and `If-None-Match`, handle 304.
        * Just cache for a configurable n seconds, keep if cache file
          modified time is newer, delete and ignore if file modified
          time is older.
* Investigate SHA1 performance  
  To check 808MB in `/enwiki/20230301/abstractsdump/*` takes:
    * wmd debug: 74s  
      `cargo run -- download --job abstractsdump`
    * wmd release with rustflags = `-C target-cpu=native`: 6s
    * sha1sum: 2s
    * `--release` ?
* Render with `pandoc`
    * Snippet:
```sh
wmd get-page --article-dump-file out/articles.bz2 \
    | jq --null-input 'input' \
    | tee >(jq --raw-output '.title' > ~/tmp/page.title) \
    | jq --raw-output '.revision.text' > ~/tmp/page.mediawiki \
    && < ~/tmp/page.mediawiki \
       pandoc --from mediawiki \
              --to html \
              --sandbox \
              --standalone \
              --toc \
              --number-sections \
              --number-offset "1" \
              --metadata title:"$(cat ~/tmp/page.title)" \
              --lua-filter <(echo '
                    function Link(el)
                        return pandoc.Link(el.content, "https://en.wikipedia.org/wiki/" .. el.target)
                    end
                ') \
    > ~/tmp/page.html \
    && xdg-open ~/tmp/page.html
```
* Read chunk as JSON using `flatc` in docker:
```
sudo docker run --rm \
    -v ${PWD}/fbs:/fbs:ro \
    -v ${PWD}/out:/out:ro \
    neomantra/flatbuffers:latest \
    sh -c '
        set -e
        cd /tmp
        flatc --defaults-json --json --size-prefixed \
            /fbs/wikimedia.fbs -- /out/articles.fbd
        cat /tmp/articles.json
    '
```
