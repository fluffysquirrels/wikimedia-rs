# Wikimedia downloader (aka `wmd`)

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
