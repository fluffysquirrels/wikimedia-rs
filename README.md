# Wikimedia downloader

Initial goals:

* Write a cronjob to run on a server that runs a tool to download the
  latest English Wikipedia database dump.
* Once a new dump is successfully downloaded older versions should be
  deleted (perhaps keep the latest 2 dumps).

## To run

```sh
# For usage and help
cargo run -- --help

# To download the latest version of a small example job
cargo run -- download --mirror-url https://ftp.acc.umu.se/mirror/wikimedia.org/dumps \
                        --dump enwiki \
                        --job namespacesv2
```

Set the environment varible `RUST_LOG` to configure logging levels and filtering. This application uses the `tracing-subscriber` crate for logging, see [their documentation for the available logging configuration directives][log-directives]. Note that many of these directives can be supplied separated by commas.

[log-directives]: https://docs.rs/tracing-subscriber/latest/tracing_subscriber/struct.EnvFilter.html#directives

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
