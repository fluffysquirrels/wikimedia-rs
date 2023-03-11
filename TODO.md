# To do


## Must do before publishing

* Rename command to `wmd`, possibly crate too.
* Investigate SHA1 performance  
  To check 808MB in `/enwiki/20230301/abstractsdump/*` takes:
    * wmd: 74s  
      `cargo run -- download --job abstractsdump`
    * sha1sum: 2s
* Tidy up args to `operations::download_job_file`
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
* Validate dump name, job name to have no relative paths, path traversal.

## Might do

* More unit testing
* Logging to JSON
    * Possibly: Support logging both pretty format to stderr and JSON to a file
    * Document `bunyan` support.
    * Or find / write a pretty printer for the tracing-subscriber JSON stream
        * [bunyan-view in Rust](https://github.com/dekobon/bunyan-view)
        * https://crates.io/crates/tracing-bunyan-formatter
    * `thread_name` `thread_id`.
    * `function_name!` macro: <https://docs.rs/function_name/latest/function_name/>
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
* Cache metadata downloads
    * Save to `./out/cache`
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
* Separate `clap` arg definitions from value types, e.g. create new DumpName, JobName tuple structs
    * Separates concerns, creates potential for non-CLI uses.
* Unify `get_dump_versions` date validation and `VersionSpecArg` date validation
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
               (easier to read/write in parallel that just one big file)
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
