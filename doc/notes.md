# Notes

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
    * wmd `--release` with rustflags = `-C target-cpu=native`: 6s
    * sha1sum: 2s
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
* bzip2 notes
    * https://github.com/dsnet/compress/blob/master/doc/bzip2-format.pdf
    * https://en.wikipedia.org/wiki/Bzip2
    * https://www.kurokatta.org/grumble/2021/03/splittable-bzip2
    * https://github.com/ruanhuabin/pbzip2
* bitreader  
  Could be used for scanning for bzip2 magic
    * https://docs.rs/bitreader/
    * https://docs.rs/bitstream-io/
    * https://docs.rs/bitvec/
* Multistream snippets
    * Parse multistream index file names  
```
cd ~/wmd/out/job-multistream; ls -v *index*.txt* \
    | sed --regexp-extended --expression 's/^.*multistream-index([0-9]+).txt-p([0-9]+)p([0-9]+)\.bz2$/\1,\2,\3,\0/'
```
    * Scan dump indices for title
```
cd ~/wmd/out/job-multistream
ls -v *index*.txt*.bz2 \
    | xargs -n1 -I % sh -c 'bzcat % | sed --expression "s/^/%:/"' \
    | egrep --line-buffered 'The Matrix'
```
    * Recompress multistream indices
```
cd ~/wmd/out/job-multistream/
ls -v *index*.txt*.bz2 \
    | xargs -n1 -I % -t sh -c 'bzcat % | sed --expression "s/^/%:/"' \
    | lz4 --compress \
    > index.txt.lz4
```
    * Scan index.txt.lz4 for title
```
cd ~/wmd/out/job-multistream
lz4cat index.txt.lz4 \
    | egrep 'The Matrix'
```
    * Create Category index
```
cd ~/wmd/out/job-multistream
pv index.txt.lz4 \
    | lz4cat \
    | egrep '^[^:]+:[0-9]+:[0-9]+:(Category:.*)$' \
    | sort --field-separator=: --key=5 \
    | lz4 --compress \
    > index.categories.lz4
```
    * How much space do indices use?
```
cd ~/wmd/out/job-multistream
du -cm *index*.bz2 | egrep total | sed -e 's/total/index.txt.bz2 total/' \
    && du -sm index.txt.lz4
```
    * Read starting at substream, seek to ID 30007
```
cd ~/wmd/out/job-multistream
tail -c +$(( 205908774 + 1 )) \
    enwiki-20230301-pages-articles-multistream1.xml-p1p41242.bz2 \
    | bzcat \
    | less +/30007
```
    * wmd get-dump-page at substream
```
cd ~/wmd/out/job-multistream
wmd get-dump-page \
    --dump-file enwiki-20230301-pages-articles-multistream1.xml-p1p41242.bz2 \
    --compression bz2 --out json-with-body \
    --seek 205908774 --count 100 \
    | jq 'select(.id == 30007)'
```
    * Disk usage for job-multistream
```
cd ~/wmd/out/job-multistream
du -cm *index*.bz2 | egrep total | sed -e 's/total/index.txt.bz2 total/' \
    && du -sm index*txt*
```
    * Extract stream offsets from multistream index  
      `bzcat ~/wmd/out/job-multistream/index.bz2 | cut -d: -f1 | uniq`
* Recompress data files as LZ4 snippet:
```
cd ~/wmd/out/job
ls *articles*.xml*.bz2 \
    | sort --version-sort \
    | xargs -n1 -P 8 -I% bash -c \
    'set -o pipefail;
IN="%";
BASE="${IN/.bz2/}";
echo "${BASE}";
OUT="${BASE}.lz4";
if ! test -f "${OUT}"; then
echo "recompressing ${BASE}"
bzcat "${IN}" | lz4 --verbose --compress -1 - "${OUT}";
fi'
```

* Disk usage for bz2 vs lz4:
```
du -cm *articles*.bz2 | egrep total | sed -e 's/total/articles*.xml.bz2 total/'
du -cm *articles*.lz4 | egrep total | sed -e 's/total/articles*.xml.lz4 total/'
```
    * 19723 MB articles*.xml.bz2 total
    * 37328 MB articles*.xml.lz4 total
    * 86.9  GB uncompressed
* Stores
    * RocksDB
    * lmdb
    * https://crates.io/crates/jammdb lmdb port to Rust
    * No: bonsaidb https://github.com/khonsulabs/bonsaidb
    * sled
    * https://crates.io/crates/marble
* Full text search
    * List of engines: https://gist.github.com/manigandham/58320ddb24fed654b57b4ba22aceae25
    * Rust
        * https://docs.rs/tantivy/latest/tantivy/
            * https://docs.rs/summavy/latest/summavy/index.html
        * https://lib.rs/crates/sonic-server
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

* Compression for chunks
    * LZ4
        * decompression multiple times faster than snappy
        * 600 MB/s output from multistream index using lz4 C or C++ CLI
        * Write multistream index at 10MB/s with lz4 --compress -9
        * Compressed sizes for multistream indexes:
            * Uncompressed:     797 MB
            * Dump bz2 files:   238 MB
            * LZ4 default (-1): 354 MB
            * LZ4         -9:   288 MB
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
    * Multistream wikipedia archives are available from my local
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
* [XOWA](http://xowa.org/), offline viewer that reads Wikimedia database dumps
    * https://github.com/gnosygnu/xowa
    * Java
    * Last release 2021-01, downloadable files are old ~2016
* Logging to JSON
    * Possibly: Support logging both pretty format to stderr and JSON to a file
    * Document `bunyan` support with `bunyan-view`.
    * Or find / write a pretty printer for the tracing-subscriber JSON stream
        * [bunyan-view in Rust](https://github.com/dekobon/bunyan-view)
        * https://crates.io/crates/tracing-bunyan-formatter
    * `thread_name` `thread_id`.
    * `function_name!` macro: <https://docs.rs/function_name/latest/function_name/>
* wmd get-dump-page output data rate for different compression algorithms:  
  `wmd get-dump-page --article-dump-file out/job/enwiki-20230301-pages-articles1.xml-p1p41242.lz4 --compression lz4 | pv > /dev/null` etc
    * bz2:  11.1 MB/s
    * None: 89.8 MB/s
    * lz4:  67.1 MB/s

* wmd import (now with rayon) @ b6e9661d387a4c67d34dea2996125bc696780ade  
  over ~100000 pages  
  `chunk_write_rate`:
    * bz2: 19.34MiB/s
    * lz4: 38.95Mib/s

## Download steps

These are approximately the steps the `download` subcommand runs:

* Download dump html index page: <https://dumps.wikimedia.org/enwiki/>
* Scrape the links on it to subdirectories
* Choose the latest date-named link
* Under a date directory there's a `dumpstatus.json` file with some metadata  
  e.g. <https://dumps.wikimedia.org/enwiki/20230301/dumpstatus.json>  
  Under '.jobs.metacurrentdumprecombine' there is:

  ```
  {
    "status": "done",
    "updated": "2023-03-02 01:26:57",
    "files": {
      "enwiki-20230301-pages-articles.xml.bz2": {
        "size": 20680789666,
        "url": "/enwiki/20230301/enwiki-20230301-pages-articles.xml.bz2",
        "md5": "99303f65fc9783df65428320ecbd5b73",
        "sha1": "d4a615ea6d1ffa82f9071c8471d950a493fa6679"
      }
    }
  }
  ```

* Check the metadata (.status == "done") and extract the file link and sha1 hash
    * Note that some `dumpstatus.json` entries (on mirrors, for
      some jobs) do not contain hashes, so make sure there is
      one.
* Download the files  
  use a mirror:
    * [Mirrors list](https://meta.wikimedia.org/wiki/Mirroring_Wikimedia_project_XML_dumps#Current_mirrors)
    * This one seems fine: <https://ftp.acc.umu.se/mirror/wikimedia.org/dumps>
* Check the files' SHA1 hashes
* Report success or failure

## More reference links

* HTML page with links to all dumps: <https://dumps.wikimedia.org/backup-index-bydb.html>
* <https://en.wikipedia.org/wiki/Wikipedia:Database_download>
* Torrents (out of date): <https://meta.wikimedia.org/wiki/Data_dump_torrents#English_Wikipedia>
* <https://meta.wikimedia.org/wiki/Data_dumps/FAQ>
